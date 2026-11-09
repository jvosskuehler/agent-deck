//! Kleine Protokolldateien: eine Zeile je Ereignis, Ortszeit vorn, grob rotiert.
//!
//! Die Dienste laufen unsichtbar im Hintergrund (worktrees löschen, Wächter, Usage).
//! Ohne Protokoll ist hinterher nicht nachzuvollziehen, was passiert ist. Schreiben ist
//! best effort: ein Protokollfehler darf nie den Dienst stören.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Wie mit einer zu großen Datei verfahren wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotate {
    /// Über der Grenze neu anfangen (Python `worktree._log`: Modus `w`).
    Truncate,
    /// Über der Grenze nur die letzten Zeilen behalten (Python `usage_poller._log`).
    KeepTail(usize),
    /// Über der Grenze nach `<datei>.1` verschieben (Python `log._rotate`).
    Rename,
}

/// Eine Protokolldatei.
#[derive(Debug, Clone)]
pub struct LogFile {
    path: PathBuf,
    max_bytes: u64,
    rotate: Rotate,
    /// Steht zwischen Zeitstempel und Text (z. B. `  pid=123  `).
    prefix: String,
}

impl LogFile {
    pub fn new(path: impl Into<PathBuf>, max_bytes: u64, rotate: Rotate) -> LogFile {
        LogFile { path: path.into(), max_bytes, rotate, prefix: " ".into() }
    }

    /// Mit anderem Trenner zwischen Zeit und Text.
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> LogFile {
        self.prefix = prefix.into();
        self
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Eine Zeile anhängen: `YYYY-mm-dd HH:MM:SS<prefix><msg>`.
    pub fn note(&self, msg: &str) {
        let _ = self.try_note(msg);
    }

    fn try_note(&self, msg: &str) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        self.rotate_if_needed();
        let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let mut f = OpenOptions::new().create(true).append(true).open(&self.path)?;
        writeln!(f, "{stamp}{}{msg}", self.prefix)
    }

    fn rotate_if_needed(&self) {
        let Ok(size) = fs::metadata(&self.path).map(|m| m.len()) else { return };
        if size <= self.max_bytes {
            return;
        }
        match self.rotate {
            Rotate::Truncate => {
                let _ = fs::write(&self.path, b"");
            }
            Rotate::KeepTail(n) => {
                if let Ok(text) = fs::read(&self.path) {
                    let text = String::from_utf8_lossy(&text);
                    let lines: Vec<&str> = text.lines().collect();
                    let tail = &lines[lines.len().saturating_sub(n)..];
                    let mut out = tail.join("\n");
                    out.push('\n');
                    let _ = fs::write(&self.path, out);
                }
            }
            Rotate::Rename => {
                let mut old = self.path.as_os_str().to_owned();
                old.push(".1");
                let _ = fs::rename(&self.path, PathBuf::from(old));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_starts_with_the_local_time_and_ends_with_the_text() {
        let d = tempfile::tempdir().unwrap();
        let log = LogFile::new(d.path().join("x.log"), 1000, Rotate::Truncate).with_prefix("  pid=7  ");
        log.note("OK");
        let text = fs::read_to_string(log.path()).unwrap();
        let re = regex::Regex::new(r"^\d{4}-\d\d-\d\d \d\d:\d\d:\d\d  pid=7  OK\n$").unwrap();
        assert!(re.is_match(&text), "{text:?}");
    }

    #[test]
    fn an_oversized_log_keeps_only_its_tail() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("x.log");
        fs::write(&p, (0..100).map(|i| format!("zeile {i}\n")).collect::<String>()).unwrap();
        LogFile::new(&p, 10, Rotate::KeepTail(3)).note("neu");
        let text = fs::read_to_string(&p).unwrap();
        assert!(text.starts_with("zeile 97\nzeile 98\nzeile 99\n"), "{text}");
        assert!(text.trim_end().ends_with("neu"));
    }

    #[test]
    fn rename_rotation_keeps_the_previous_log_beside_it() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("x.log");
        fs::write(&p, "alt\n".repeat(50)).unwrap();
        LogFile::new(&p, 10, Rotate::Rename).note("neu");
        assert!(d.path().join("x.log.1").is_file());
        assert!(fs::read_to_string(&p).unwrap().ends_with("neu\n"));
    }
}
