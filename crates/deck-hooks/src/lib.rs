//! Die Claude-Code-Hooks als kleine Programme: `deck-report <status>` und
//! `deck-statusline`. Gleiche Dateien, gleiche Felder, gleiche Ausgabe wie die
//! Python-Fassung (`deck/claude/hooks/report.py`, `statusline.py`, `resolve.py`).
//!
//! **Ein Hook endet nie mit Fehler.** Bei UserPromptSubmit und PreToolUse liest Claude
//! Code einen Exit ≠ 0 als Veto gegen Prompt bzw. Tool-Aufruf. Darum laufen beide
//! Programme in `run_guarded`: jede Panik endet mit Exit 0 (auch im Release-Profil mit
//! `panic = "abort"`, wo `catch_unwind` nichts fängt: dort beendet der Panik-Hook den
//! Prozess, bevor abgebrochen wird).
//!
//! Die Logik steht hier in der Bibliothek, damit Tests sie mit Wegwerf-Ordnern und
//! eigener Eingabe aufrufen können; die Programme in `src/bin` sind nur Hüllen.

pub mod cache_scan;
pub mod env;
pub mod procs;
pub mod report;
pub mod status_extras;
pub mod statusline;

use std::io::{IsTerminal, Read};

use serde_json::{Map, Value};

/// `f` ausführen und den Prozess IMMER mit Exit 0 beenden, auch bei einer Panik.
pub fn run_guarded(f: impl FnOnce() + std::panic::UnwindSafe) -> ! {
    std::panic::set_hook(Box::new(|_| std::process::exit(0)));
    let _ = std::panic::catch_unwind(f);
    std::process::exit(0)
}

/// stdin roh lesen. Keine Pipe (Konsole) oder ein Lesefehler → `None`.
pub fn read_stdin() -> Option<Vec<u8>> {
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return None;
    }
    let mut buf = Vec::new();
    stdin.lock().read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// Das Hook-JSON aus den rohen Bytes. Claude Code liefert IMMER UTF-8; wer die
/// Konsolen-Codepage nimmt, macht aus „ö“ Mojibake, das dann in der Zustandsdatei und
/// im Tooltip landet. Kein, leeres oder kaputtes stdin → leeres Objekt. Ein gültiges
/// JSON, das kein Objekt ist, kommt unverändert zurück (die Aufrufer brechen dann ab,
/// wie die Python-Fassung an `data.get` scheitert).
pub fn parse_stdin(raw: Option<&[u8]>) -> Value {
    let empty = Value::Object(Map::new());
    let Some(raw) = raw else { return empty };
    let text = String::from_utf8_lossy(raw);
    if deck_core::pystr::strip(&text).is_empty() {
        return empty;
    }
    serde_json::from_str(&text).unwrap_or(empty)
}
