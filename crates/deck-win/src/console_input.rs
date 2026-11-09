//! Text und Tasten in die Konsole eines fremden Prozesses schreiben, ohne Fokus (aus
//! `platform/console_input.py`).
//!
//! Windows Terminal hat kein `sendText` wie VS Code. Jede Konsole hat aber einen
//! Eingabepuffer, und `WriteConsoleInputW` legt dort Tastendrücke ab, als hätte jemand
//! getippt: der Prozess im Pane liest sie wie echte Eingabe, egal welches Fenster vorn
//! ist. Dazu hängt man sich per `AttachConsole(pid)` an dessen Konsole. Erprobt in der
//! Python-Fassung am 2026-11-06 (Claude Code in unsichtbarer Konsole und in WT-Panes).
//!
//! **Das passiert immer in einem Wegwerfprozess** (`deck-console`, siehe
//! `src/bin/deck-console.rs`), nie in der App: ein Prozess, der an einer fremden Konsole
//! hängt, bekommt deren `CTRL_CLOSE_EVENT`, wenn sie zugeht, und die Standardbehandlung
//! beendet ihn. Das soll ein Wegwerfprozess abbekommen, nicht das Deck. Die App ruft
//! [`send_via_helper`]; [`inject_into_attached_console`] ist nur für den Helfer da.
//!
//! Steuerzeichen (`\r`, Esc, Tab, Backspace) gehen mit passendem Virtual-Key-Code raus,
//! alles andere als Zeichen. Folgen wie `\x1b[Z` (Shift+Tab) oder die Bracketed-Paste-
//! Marker kommen Zeichen für Zeichen an; Claude liest daraus dieselbe Sequenz wie aus
//! VS Code. Welche Folge eine Taste ist (KEYMAP, Pausen), ist Fachlogik in `deck-core`.

use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Console::{
    AttachConsole, FreeConsole, GetStdHandle, INPUT_RECORD, INPUT_RECORD_0, KEY_EVENT,
    KEY_EVENT_RECORD, KEY_EVENT_RECORD_0, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    SetConsoleCtrlHandler, SetStdHandle, WriteConsoleInputW,
};
use windows::Win32::System::Threading::CREATE_NO_WINDOW;
use windows::core::w;

use crate::guard;
use crate::types::{Error, Result};

/// Ein Schritt der Eingabe: Text tippen oder warten (etwa das eigene Enter nach einem
/// Bracketed Paste, sonst würde es darin verschluckt).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Text(String),
    Sleep(Duration),
}

/// Name der Hilfs-Binary.
pub const HELPER_EXE: &str = "deck-console.exe";

/// So lange darf der Wegwerfprozess brauchen (wie in der Python-Fassung).
pub const HELPER_TIMEOUT: Duration = Duration::from_secs(30);

/// Wo die Hilfs-Binary liegt: neben der laufenden Exe, sonst eine Ebene höher (so liegt
/// sie bei `cargo run --example`: Beispiele in `target/<profil>/examples/`, Binaries in
/// `target/<profil>/`). `None`, wenn sie an keiner der Stellen liegt.
pub fn helper_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [Some(dir), dir.parent()]
        .into_iter()
        .flatten()
        .map(|d| d.join(HELPER_EXE))
        .find(|p| p.is_file())
}

/// Schritte in die Konsole von `pid` schreiben, über den Wegwerfprozess `helper`.
/// Liefert die Zahl der geschriebenen Eingabe-Ereignisse.
pub fn send_via_helper(helper: &Path, pid: u32, steps: &[Step]) -> Result<usize> {
    guard::check("console_input")?;
    let mut child = Command::new(helper)
        .arg(pid.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW.0)
        .spawn()
        .map_err(|e| Error::Helper(format!("{} startet nicht: {e}", helper.display())))?;
    // Die Schritte gehen über stdin statt über die Kommandozeile: die ist auf 32 K
    // Zeichen begrenzt, und ein langer Prompt als Bracketed Paste kommt da heran.
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(encode_steps(steps).as_bytes())
            .map_err(|e| Error::Helper(format!("Schritte nicht übergeben: {e}")))?;
    } // stdin zu: der Helfer weiß, dass nichts mehr kommt
    let deadline = Instant::now() + HELPER_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                return Err(Error::Helper("deck-console: Zeitüberschreitung".into()));
            }
            Err(e) => return Err(Error::Helper(format!("deck-console: {e}"))),
        }
    }
    let mut out = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = std::io::Read::read_to_string(&mut stdout, &mut out);
    }
    parse_helper_output(&out).map_err(Error::Helper)
}

/// Schritte als Text für stdin des Helfers: je Zeile `text <hex>` (UTF-8 als Hex, damit
/// Zeilenumbrüche und Steuerzeichen im Text das Format nicht brechen) oder `sleep <ms>`.
pub fn encode_steps(steps: &[Step]) -> String {
    let mut out = String::new();
    for s in steps {
        match s {
            Step::Text(t) => {
                out.push_str("text ");
                for b in t.as_bytes() {
                    out.push_str(&format!("{b:02x}"));
                }
            }
            Step::Sleep(d) => out.push_str(&format!("sleep {}", d.as_millis())),
        }
        out.push('\n');
    }
    out
}

/// Gegenstück zu [`encode_steps`]; leere Zeilen werden übersprungen.
pub fn decode_steps(s: &str) -> std::result::Result<Vec<Step>, String> {
    let mut out = Vec::new();
    for line in s.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let (kind, val) = line.split_once(' ').unwrap_or((line, ""));
        match kind {
            "text" => {
                if val.len() % 2 != 0 {
                    return Err(format!("ungerade Hex-Länge: {line}"));
                }
                let bytes = (0..val.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&val[i..i + 2], 16))
                    .collect::<std::result::Result<Vec<u8>, _>>()
                    .map_err(|e| format!("kein Hex ({e}): {line}"))?;
                out.push(Step::Text(
                    String::from_utf8(bytes).map_err(|e| format!("kein UTF-8: {e}"))?,
                ));
            }
            "sleep" => {
                let ms: u64 = val
                    .parse()
                    .map_err(|e| format!("keine Millisekunden ({e}): {line}"))?;
                out.push(Step::Sleep(Duration::from_millis(ms)));
            }
            _ => return Err(format!("unbekannter Schritt: {line}")),
        }
    }
    Ok(out)
}

/// Antwort des Helfers lesen: `ok <n>` oder `fail <grund>`.
pub fn parse_helper_output(out: &str) -> std::result::Result<usize, String> {
    let line = out
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    if let Some(n) = line.strip_prefix("ok ") {
        return n
            .trim()
            .parse()
            .map_err(|_| format!("unverständliche Antwort: {line}"));
    }
    match line.strip_prefix("fail ") {
        Some(why) => Err(why.to_string()),
        None if line.is_empty() => {
            Err("keine Antwort (Helfer abgestürzt oder von der Konsole beendet)".into())
        }
        None => Err(format!("unverständliche Antwort: {line}")),
    }
}

/// Ein Tastenereignis vor der Umwandlung in `INPUT_RECORD`: (gedrückt, VK, UTF-16-Einheit).
pub type KeyEvent = (bool, u16, u16);

/// Je UTF-16-Einheit ein Drücken und ein Loslassen. Zeichen außerhalb der BMP gehen als
/// zwei Hälften raus, so liest sie auch `ReadConsoleInputW`.
pub fn key_events(text: &str) -> Vec<KeyEvent> {
    let mut out = Vec::new();
    for unit in text.encode_utf16() {
        let vk = virtual_key(unit);
        out.push((true, vk, unit));
        out.push((false, vk, unit));
    }
    out
}

/// Virtual-Key-Code für Steuerzeichen, 0 für alles andere (wie die Python-Fassung).
pub fn virtual_key(unit: u16) -> u16 {
    match unit {
        0x0D => 0x0D, // Enter
        0x1B => 0x1B, // Esc
        0x09 => 0x09, // Tab
        0x08 => 0x08, // Backspace
        _ => 0,
    }
}

fn records(text: &str) -> Vec<INPUT_RECORD> {
    key_events(text)
        .into_iter()
        .map(|(down, vk, unit)| INPUT_RECORD {
            EventType: KEY_EVENT as u16,
            Event: INPUT_RECORD_0 {
                KeyEvent: KEY_EVENT_RECORD {
                    bKeyDown: down.into(),
                    wRepeatCount: 1,
                    wVirtualKeyCode: vk,
                    wVirtualScanCode: 0,
                    uChar: KEY_EVENT_RECORD_0 { UnicodeChar: unit },
                    dwControlKeyState: 0,
                },
            },
        })
        .collect()
}

/// Hängt DIESEN Prozess an die Konsole von `pid` und schreibt die Schritte hinein.
///
/// **Nur im Wegwerfprozess aufrufen** (siehe Modulkopf): geht die fremde Konsole
/// währenddessen zu, endet der aufrufende Prozess mit ihr. Die Standard-Handles werden
/// vorher gesichert und danach zurückgesetzt, damit die Antwort an den Aufrufer wieder
/// in dessen Rohr geht und nicht in die fremde Konsole.
pub fn inject_into_attached_console(pid: u32, steps: &[Step]) -> Result<usize> {
    guard::check("console_input")?;
    // SAFETY: Konsolen-APIs mit Handles als Werten; CONIN$-Handle und Anhängen werden
    // in jedem Fall wieder gelöst, die Standard-Handles wiederhergestellt.
    unsafe {
        let saved = [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE]
            .map(|k| (k, GetStdHandle(k).unwrap_or_default()));
        let _ = FreeConsole();
        AttachConsole(pid).map_err(|e| Error::win("AttachConsole", e))?;
        // Ein Ctrl+C in der fremden Konsole (oder ein mitgeschicktes \x03) soll den
        // Helfer nicht mitten im Schreiben beenden. CTRL_CLOSE bleibt davon unberührt.
        let _ = SetConsoleCtrlHandler(None, true);
        let result = write_steps(steps);
        let _ = FreeConsole();
        for (k, h) in saved {
            let _ = SetStdHandle(k, h);
        }
        result
    }
}

unsafe fn write_steps(steps: &[Step]) -> Result<usize> {
    // SAFETY: Aufrufer hängt an einer Konsole; Handle wird geschlossen.
    unsafe {
        let h = CreateFileW(
            w!("CONIN$"),
            (GENERIC_READ | GENERIC_WRITE).0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
        .map_err(|e| Error::win("CreateFileW(CONIN$)", e))?;
        let mut total = 0usize;
        let mut result = Ok(());
        for step in steps {
            match step {
                Step::Sleep(d) => std::thread::sleep(*d),
                Step::Text(t) => {
                    let recs = records(t);
                    if recs.is_empty() {
                        continue;
                    }
                    let mut n = 0u32;
                    if let Err(e) = WriteConsoleInputW(h, &recs, &mut n) {
                        result = Err(Error::win("WriteConsoleInputW", e));
                        break;
                    }
                    total += n as usize;
                }
            }
        }
        let _ = CloseHandle(h);
        result.map(|()| total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_survive_the_trip_through_the_helper_pipe() {
        let steps = vec![
            Step::Text("\x1b[200~Zeile 1\nZeile 2 ä 🙂\x1b[201~".into()),
            Step::Sleep(Duration::from_millis(450)),
            Step::Text("\r".into()),
            Step::Text(String::new()),
        ];
        let wire = encode_steps(&steps);
        assert_eq!(
            wire.lines().count(),
            4,
            "ein Schritt je Zeile, auch mit \\n im Text"
        );
        assert_eq!(decode_steps(&wire).unwrap(), steps);
    }

    #[test]
    fn garbage_on_the_helper_pipe_is_rejected() {
        assert!(decode_steps("text 4").is_err());
        assert!(decode_steps("text zz").is_err());
        assert!(decode_steps("sleep lang").is_err());
        assert!(decode_steps("tippe abc").is_err());
        assert_eq!(decode_steps("\n\n").unwrap(), vec![]);
    }

    #[test]
    fn helper_answer_is_count_or_reason() {
        assert_eq!(parse_helper_output("ok 12\n"), Ok(12));
        assert_eq!(
            parse_helper_output("fail AttachConsole: Zugriff verweigert"),
            Err("AttachConsole: Zugriff verweigert".into())
        );
        assert!(
            parse_helper_output("")
                .unwrap_err()
                .contains("keine Antwort")
        );
        assert!(parse_helper_output("ok viele").is_err());
    }

    #[test]
    fn control_characters_carry_their_virtual_key() {
        assert_eq!(
            key_events("\r"),
            vec![(true, 0x0D, 0x0D), (false, 0x0D, 0x0D)]
        );
        assert_eq!(key_events("a")[0], (true, 0, 'a' as u16));
        // Shift+Tab ist Esc [ Z: nur das Esc bekommt einen VK.
        let vks: Vec<u16> = key_events("\x1b[Z")
            .iter()
            .step_by(2)
            .map(|e| e.1)
            .collect();
        assert_eq!(vks, vec![0x1B, 0, 0]);
    }

    #[test]
    fn characters_outside_the_bmp_go_out_as_surrogate_pairs() {
        let ev = key_events("🙂");
        assert_eq!(ev.len(), 4);
        assert_eq!((ev[0].2, ev[2].2), (0xD83D, 0xDE42));
    }

    #[test]
    fn sending_keys_is_blocked_in_tests() {
        assert!(matches!(
            send_via_helper(Path::new("gibt-es-nicht.exe"), 1, &[]),
            Err(Error::Forbidden(_))
        ));
        assert!(matches!(
            inject_into_attached_console(1, &[]),
            Err(Error::Forbidden(_))
        ));
    }
}
