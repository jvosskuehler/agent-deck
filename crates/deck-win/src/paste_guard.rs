//! Sperre für simulierte Tasten ins Terminal, während ORPHEUS diktiert (warum: siehe
//! `deck_core::speechmike::OrpheusDictation`).
//!
//! Ein Low-Level-Tastaturhook sieht jeden Tastendruck, bevor ein Fenster ihn bekommt, und
//! Windows markiert per `SendInput` erzeugte mit `LLKHF_INJECTED`. Verworfen wird nur, was
//! alle Bedingungen erfüllt: Sperre aktiv, simuliert, nicht vom Deck selbst (Marke
//! [`DECK_INPUT_TAG`] in `dwExtraInfo`), ein Drücken (kein Loslassen) einer Nicht-Modifier-
//! Taste, und Windows Terminal vorn. So kommt weder Strg+V noch Shift+Einfg noch getippter
//! Text an. Loslassen und Modifier laufen immer durch: ein verschlucktes Loslassen ließe
//! Strg im System hängen.
//!
//! Der Hook muss schnell sein (Windows wirft langsame Hooks still hinaus), darum keine
//! Dateiarbeit hier; verworfene Tasten werden nur gezählt ([`take_blocked`]).

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN,
    VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, SetWindowsHookExW, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_SYSKEYDOWN,
};

use crate::guard;
use crate::types::{Error, Result};

/// Marke für Tasten, die das Deck selbst schickt; die Sperre lässt sie durch.
pub const DECK_INPUT_TAG: usize = 0x4445_434B; // "DECK"

static GUARD_UNTIL: AtomicU64 = AtomicU64::new(0);
static BLOCKED: AtomicU32 = AtomicU32::new(0);

/// Millisekunden seit Systemstart, die Uhr für [`arm_until`].
pub fn now_ms() -> u64 {
    // SAFETY: reine Abfrage.
    unsafe { GetTickCount64() }
}

/// Sperre bis `until` (ms nach [`now_ms`]); ein früherer Wert verkürzt sie.
pub fn arm_until(until: u64) {
    GUARD_UNTIL.store(until, Ordering::Relaxed);
}

/// Wie viele Tasten seit dem letzten Aufruf verworfen wurden.
pub fn take_blocked() -> u32 {
    BLOCKED.swap(0, Ordering::Relaxed)
}

/// Hook in einem eigenen Thread setzen; er lebt so lange wie der Prozess.
pub fn start() -> Result<()> {
    guard::check("keyboard_hook")?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("paste-guard".into())
        .spawn(move || {
            // SAFETY: Hook-Prozedur ist eine statische Funktion dieses Moduls; der Thread
            // pumpt Nachrichten, wie WH_KEYBOARD_LL es verlangt.
            let hook = unsafe {
                let module = GetModuleHandleW(None).ok();
                SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), module.map(Into::into), 0)
            };
            let ok = hook.is_ok();
            let _ = tx.send(hook.map(|_| ()).map_err(|e| Error::win("SetWindowsHookExW", e)));
            if ok {
                let mut msg = MSG::default();
                // SAFETY: Standard-Nachrichtenschleife.
                while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {}
            }
        })
        .map_err(|e| Error::Helper(e.to_string()))?;
    rx.recv().map_err(|e| Error::Helper(e.to_string()))?
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY: bei HC_ACTION zeigt lparam auf ein gültiges KBDLLHOOKSTRUCT.
        let k = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        if should_block(k, wparam.0 as u32) {
            BLOCKED.fetch_add(1, Ordering::Relaxed);
            return LRESULT(1);
        }
    }
    // SAFETY: Weitergabe an die nächste Hook-Stufe, unverändert.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn should_block(k: &KBDLLHOOKSTRUCT, msg: u32) -> bool {
    decide(k, msg, GUARD_UNTIL.load(Ordering::Relaxed) > now_ms(), crate::speechmike::terminal_in_front)
}

/// Die Regel; der Blick auf den Vordergrund kommt zuletzt, weil er am meisten kostet.
fn decide(k: &KBDLLHOOKSTRUCT, msg: u32, armed: bool, terminal_in_front: impl Fn() -> bool) -> bool {
    armed
        && (k.flags.0 & LLKHF_INJECTED.0) != 0
        && k.dwExtraInfo != DECK_INPUT_TAG
        && (msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN)
        && !is_modifier(k.vkCode)
        && terminal_in_front()
}

fn is_modifier(vk: u32) -> bool {
    [VK_SHIFT, VK_LSHIFT, VK_RSHIFT, VK_CONTROL, VK_LCONTROL, VK_RCONTROL, VK_MENU, VK_LMENU, VK_RMENU, VK_LWIN, VK_RWIN]
        .iter()
        .any(|m| u32::from(m.0) == vk)
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::{KBDLLHOOKSTRUCT_FLAGS, WM_KEYUP};

    fn key(vk: u32, injected: bool, tag: usize) -> KBDLLHOOKSTRUCT {
        KBDLLHOOKSTRUCT {
            vkCode: vk,
            flags: KBDLLHOOKSTRUCT_FLAGS(if injected { LLKHF_INJECTED.0 } else { 0 }),
            dwExtraInfo: tag,
            ..Default::default()
        }
    }

    #[test]
    fn installing_the_hook_is_blocked_in_tests() {
        assert_eq!(start(), Err(Error::Forbidden("keyboard_hook")));
    }

    const V: u32 = 0x56;

    #[test]
    fn a_simulated_paste_into_the_terminal_is_dropped_while_armed() {
        assert!(decide(&key(V, true, 0), WM_KEYDOWN, true, || true));
    }

    #[test]
    fn real_keys_own_keys_releases_and_modifiers_always_pass() {
        assert!(!decide(&key(V, false, 0), WM_KEYDOWN, true, || true), "echte Taste");
        assert!(!decide(&key(0x4B, true, DECK_INPUT_TAG), WM_SYSKEYDOWN, true, || true), "Alt+K vom Deck");
        assert!(!decide(&key(V, true, 0), WM_KEYUP, true, || true), "Loslassen");
        assert!(!decide(&key(u32::from(VK_LCONTROL.0), true, 0), WM_KEYDOWN, true, || true), "Strg");
    }

    #[test]
    fn nothing_is_dropped_when_unarmed_or_another_window_is_in_front() {
        assert!(!decide(&key(V, true, 0), WM_KEYDOWN, false, || true));
        assert!(!decide(&key(V, true, 0), WM_KEYDOWN, true, || false));
    }
}
