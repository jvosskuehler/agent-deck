//! Protokolldatei der App: `%LOCALAPPDATA%\claude-agent-deck\agent-deck.log`.
//!
//! Wie `deck/ops/log.py`: eine Zeile je Ereignis mit Zeitstempel, beim Start einmal
//! rotiert (`.1`), damit die Datei klein genug bleibt, um sie im Zweifel ganz zu lesen.
//! Eigener Name statt `panel.log` der Python-Fassung, damit ein altes Log nicht mit
//! dem neuen verwechselt wird.
//!
//! Schreiben ist best effort: ein Log, das nicht schreiben kann, darf nichts kippen.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub const LOG_FILE: &str = "agent-deck.log";
const MAX_BYTES: u64 = 512 * 1024;

static SINK: OnceLock<Mutex<Option<File>>> = OnceLock::new();

/// Log öffnen (einmal je Prozess) und Panics mitschreiben.
pub fn install(dir: &Path) -> PathBuf {
    let path = dir.join(LOG_FILE);
    let _ = fs::create_dir_all(dir);
    if fs::metadata(&path).map(|m| m.len() >= MAX_BYTES).unwrap_or(false) {
        let _ = fs::rename(&path, dir.join(format!("{LOG_FILE}.1")));
    }
    let file = OpenOptions::new().create(true).append(true).open(&path).ok();
    let _ = SINK.set(Mutex::new(file));
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        note(format!("PANIC: {info}"));
        prev(info);
    }));
    path
}

/// Eine Zeile ins Log (und im Entwicklungsbuild auf stderr).
pub fn note(msg: impl AsRef<str>) {
    let line = format!("[{}] {}\n", stamp(SystemTime::now()), msg.as_ref());
    if cfg!(debug_assertions) {
        eprint!("{line}");
    }
    if let Some(m) = SINK.get()
        && let Ok(mut g) = m.lock()
        && let Some(f) = g.as_mut()
    {
        let _ = f.write_all(line.as_bytes());
        let _ = f.flush();
    }
}

/// Zeitstempel der Zeile in UTC (`2026-11-07 14:03:05Z`), gerechnet in `deck_core::isotime`.
fn stamp(t: SystemTime) -> String {
    deck_core::isotime::utc_stamp(t.duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0))
}
