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

/// `2026-11-07 14:03:05Z`: UTC, ohne Zeitzonen-Bibliothek.
fn stamp(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}Z", rest / 3600, rest % 3600 / 60, rest % 60)
}

/// Tage seit 1970-01-01 → (Jahr, Monat, Tag) im gregorianischen Kalender
/// (Howard Hinnant, `civil_from_days`).
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn timestamps_are_readable_utc() {
        let t = UNIX_EPOCH + Duration::from_secs(1_794_060_185);
        assert_eq!(stamp(t), "2026-11-07 14:03:05Z");
        assert_eq!(stamp(UNIX_EPOCH), "1970-01-01 00:00:00Z");
    }
}
