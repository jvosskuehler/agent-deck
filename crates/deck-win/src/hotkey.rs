//! Globale Tastenkombination, die wirkt, egal welches Programm vorn ist (aus
//! `platform/hotkey.py`).
//!
//! `RegisterHotKey` bindet eine Kombination an einen THREAD; dessen Nachrichtenschleife
//! bekommt `WM_HOTKEY`. Darum läuft hier ein eigener kleiner Thread mit `GetMessageW`,
//! und jeder Tastendruck kommt als [`HotkeyEvent::Pressed`] über einen Kanal an. Die App
//! muss so nichts von Fensternachrichten wissen.
//!
//! Nebeneffekt, den man braucht: Windows erlaubt dem Prozess, der die Taste bekommt,
//! danach ein anderes Fenster nach vorn zu holen (sonst blinkt nur die Taskleiste).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey,
    UnregisterHotKey,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetMessageW, MSG, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, WM_HOTKEY, WM_QUIT,
};

use crate::guard;
use crate::types::{Error, Result};

/// Eine geparste Kombination: Modifier-Bits (`MOD_*`) und Virtual-Key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Combo {
    pub modifiers: u32,
    pub vk: u32,
}

/// Was der Hotkey-Thread meldet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyEvent {
    /// Die Kombination gehört jetzt uns.
    Registered,
    /// Auch nach allen Versuchen belegt; der Thread hat sich beendet.
    Failed,
    /// Die Kombination wurde gedrückt.
    Pressed,
}

const VK_F13: u32 = 0x7C;
const VK_F24: u32 = 0x87;

/// `"ctrl+alt+space"` → [`Combo`]. Buchstaben, Ziffern, F1 bis F24 und benannte Tasten;
/// Modifier `ctrl`/`strg`, `alt`, `shift`, `win`. Mindestens ein Modifier ist Pflicht:
/// eine nackte globale Taste nähme sie jedem anderen Programm weg. Ausnahme F13 bis F24:
/// die hat keine normale Tastatur, sie kommen nur von Makro-Tastaturen (das Makropad
/// sendet F13 bis F21), und kein Programm rechnet mit ihnen. Unbrauchbar → `None`.
pub fn parse(combo: &str) -> Option<Combo> {
    let parts: Vec<String> = combo
        .split('+')
        .map(|p| p.trim().to_lowercase())
        .filter(|p| !p.is_empty())
        .collect();
    let (key, mods) = parts.split_last()?;
    if mods.is_empty() {
        let vk = key_code(key)?;
        return (VK_F13..=VK_F24).contains(&vk).then_some(Combo { modifiers: 0, vk });
    }
    let mut modifiers = 0;
    for m in mods {
        modifiers |= match m.as_str() {
            "ctrl" | "strg" => MOD_CONTROL.0,
            "alt" => MOD_ALT.0,
            "shift" => MOD_SHIFT.0,
            "win" => MOD_WIN.0,
            _ => return None,
        };
    }
    Some(Combo {
        modifiers,
        vk: key_code(key)?,
    })
}

fn key_code(key: &str) -> Option<u32> {
    let named = match key {
        "space" | "leertaste" => Some(0x20),
        "enter" => Some(0x0D),
        "tab" => Some(0x09),
        "esc" => Some(0x1B),
        "pause" => Some(0x13),
        "insert" => Some(0x2D),
        "home" => Some(0x24),
        "end" => Some(0x23),
        // Die Taste links neben Y auf der deutschen Tastatur (<>|), VK_OEM_102.
        "<" | "kleiner" => Some(0xE2),
        _ => None,
    };
    if named.is_some() {
        return named;
    }
    let mut chars = key.chars();
    if let (Some(c), None) = (chars.next(), chars.next())
        && c.is_ascii_alphanumeric()
    {
        return Some(u32::from(c.to_ascii_uppercase()));
    }
    let n: u32 = key.strip_prefix('f')?.parse().ok()?;
    (1..=24).contains(&n).then(|| 0x70 + n - 1)
}

/// Eine registrierte globale Tastenkombination auf eigenem Thread. Beim `Drop` wird sie
/// freigegeben.
pub struct GlobalHotkey {
    thread_id: Arc<AtomicU32>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl GlobalHotkey {
    /// Startet den Thread und versucht, `combo` zu belegen. Ist sie noch belegt
    /// (typisch beim Neustart, wenn die alte Instanz sie kurz hält), wird `retries` mal
    /// im Abstand `retry` erneut versucht. Ereignisse kommen über den Empfänger.
    pub fn start(
        combo: &str,
        retries: u32,
        retry: Duration,
    ) -> Result<(Self, Receiver<HotkeyEvent>)> {
        guard::check("hotkey")?;
        let c = parse(combo)
            .ok_or_else(|| Error::Rejected(format!("Tastenkombination unbrauchbar: {combo:?}")))?;
        let (tx, rx) = channel();
        let thread_id = Arc::new(AtomicU32::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (tid, st) = (thread_id.clone(), stop.clone());
        let handle = std::thread::Builder::new()
            .name("hotkey".into())
            .spawn(move || run(c, retries.max(1), retry, tx, tid, st))
            .map_err(|e| Error::Helper(format!("Hotkey-Thread: {e}")))?;
        Ok((
            GlobalHotkey {
                thread_id,
                stop,
                handle: Some(handle),
            },
            rx,
        ))
    }

    /// Freigeben und den Thread beenden.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let tid = self.thread_id.load(Ordering::SeqCst);
        if tid != 0 {
            // SAFETY: Nachricht ohne Zeiger an unseren eigenen Thread.
            let _ = unsafe { PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0)) };
        }
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for GlobalHotkey {
    fn drop(&mut self) {
        self.stop();
    }
}

const ID: i32 = 1;

fn run(
    c: Combo,
    retries: u32,
    retry: Duration,
    tx: Sender<HotkeyEvent>,
    tid: Arc<AtomicU32>,
    stop: Arc<AtomicBool>,
) {
    // SAFETY: Nachrichten-APIs mit lokalem MSG; registriert wird nur für diesen Thread.
    unsafe {
        // Erst die Nachrichtenschlange anlegen, dann die Thread-Id verraten: vorher
        // liefe ein PostThreadMessageW aus stop() ins Leere.
        let mut msg = MSG::default();
        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
        tid.store(GetCurrentThreadId(), Ordering::SeqCst);
        let mods = HOT_KEY_MODIFIERS(c.modifiers | MOD_NOREPEAT.0);
        let mut ok = false;
        for attempt in 0..retries {
            if stop.load(Ordering::SeqCst) {
                return;
            }
            if RegisterHotKey(None, ID, mods, c.vk).is_ok() {
                ok = true;
                break;
            }
            if attempt + 1 < retries {
                sleep_unless_stopped(retry, &stop);
            }
        }
        if !ok {
            let _ = tx.send(HotkeyEvent::Failed);
            return;
        }
        let _ = tx.send(HotkeyEvent::Registered);
        while !stop.load(Ordering::SeqCst) && GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            // Empfänger weg = niemand hört mehr zu: Taste freigeben.
            if msg.message == WM_HOTKEY && tx.send(HotkeyEvent::Pressed).is_err() {
                break;
            }
        }
        let _ = UnregisterHotKey(None, ID);
    }
}

fn sleep_unless_stopped(total: Duration, stop: &AtomicBool) {
    let end = Instant::now() + total;
    while Instant::now() < end && !stop.load(Ordering::SeqCst) {
        std::thread::sleep(
            Duration::from_millis(50).min(end.saturating_duration_since(Instant::now())),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn german_less_than_key_is_vk_oem_102() {
        assert_eq!(
            parse("ctrl+<"),
            Some(Combo {
                modifiers: MOD_CONTROL.0,
                vk: 0xE2
            })
        );
        assert_eq!(
            parse("Strg + kleiner"),
            Some(Combo {
                modifiers: MOD_CONTROL.0,
                vk: 0xE2
            })
        );
    }

    #[test]
    fn modifiers_combine_and_letters_become_uppercase_vk() {
        let c = parse("ctrl+alt+shift+win+k").unwrap();
        assert_eq!(
            c.modifiers,
            MOD_CONTROL.0 | MOD_ALT.0 | MOD_SHIFT.0 | MOD_WIN.0
        );
        assert_eq!(c.vk, u32::from(b'K'));
        assert_eq!(parse("alt+7").unwrap().vk, u32::from(b'7'));
    }

    #[test]
    fn function_keys_go_from_f1_to_f24() {
        assert_eq!(parse("ctrl+f1").unwrap().vk, 0x70);
        assert_eq!(parse("ctrl+F24").unwrap().vk, 0x87);
        assert_eq!(parse("ctrl+f25"), None);
        assert_eq!(parse("ctrl+f0"), None);
    }

    #[test]
    fn named_keys_are_known() {
        assert_eq!(parse("ctrl+alt+space").unwrap().vk, 0x20);
        assert_eq!(parse("alt+leertaste").unwrap().vk, 0x20);
        assert_eq!(parse("win+esc").unwrap().vk, 0x1B);
    }

    #[test]
    fn a_bare_key_or_unknown_part_is_refused() {
        assert_eq!(parse("k"), None);
        assert_eq!(parse("ctrl+"), None);
        assert_eq!(parse(""), None);
        assert_eq!(parse("hyper+k"), None);
        assert_eq!(parse("ctrl+ä"), None);
        assert_eq!(parse("ctrl+kk"), None);
    }

    #[test]
    fn f13_to_f24_work_without_a_modifier() {
        assert_eq!(parse("f13"), Some(Combo { modifiers: 0, vk: 0x7C }));
        assert_eq!(parse("F20"), Some(Combo { modifiers: 0, vk: 0x83 }));
        assert_eq!(parse("f24"), Some(Combo { modifiers: 0, vk: 0x87 }));
        assert_eq!(parse("f12"), None);
        assert_eq!(parse("<"), None);
    }

    #[test]
    fn registering_is_blocked_in_tests() {
        assert!(matches!(
            GlobalHotkey::start("ctrl+<", 1, Duration::ZERO),
            Err(Error::Forbidden(_))
        ));
    }
}
