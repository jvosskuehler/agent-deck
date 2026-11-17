//! Ein Programm versteckt (ohne aufblitzendes Konsolenfenster) und mit Zeitlimit laufen
//! lassen, für alle Datenquellen: gh, git (als [`TimedGit`]), ssh, acli. Je Aufruf mit
//! eigenem Arbeitsordner und eigener Umgebung; stdout kommt auch bei Exit ≠ 0 zurück (gh
//! druckt GraphQL-Fehler und endet mit 1).
//!
//! stdout und stderr werden in eigenen Threads gelesen: ein voller Puffer auf einer
//! Seite darf den Prozess nicht festhalten, während hier auf ihn gewartet wird. Nach dem
//! Zeitlimit wird der Prozess beendet.

use std::io::Read;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::worktree::Git;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Ausgang eines Laufs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// `cmd` versteckt starten und höchstens `timeout` warten. `Err` = nicht startbar oder
/// Zeitlimit (Text kurz, für Protokoll und Hover).
pub fn run(mut cmd: Command, timeout: Duration) -> Result<Output, String> {
    let name = cmd.get_program().to_string_lossy().into_owned();
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("{name} nicht startbar: {}", e.kind()))?;
    let mut out = child.stdout.take();
    let mut err = child.stderr.take();
    let read_out = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(o) = out.as_mut() {
            let _ = o.read_to_end(&mut buf);
        }
        buf
    });
    let read_err = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(e) = err.as_mut() {
            let _ = e.read_to_end(&mut buf);
        }
        buf
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let stdout = String::from_utf8_lossy(&read_out.join().unwrap_or_default()).into_owned();
    let stderr = String::from_utf8_lossy(&read_err.join().unwrap_or_default()).into_owned();
    match status {
        Some(s) => Ok(Output { code: s.code().unwrap_or(1), stdout, stderr }),
        None => Err(format!("{name}: Zeitlimit {} s", timeout.as_secs())),
    }
}

/// Fehlertext kurz und ohne Geheimnisse: erste nicht leere Zeile, Wörter, die wie ein
/// GitHub-Token aussehen, ersetzt, höchstens 160 Zeichen.
pub fn short_error(text: &str) -> String {
    const TOKEN_PREFIXES: [&str; 6] = ["ghp_", "gho_", "ghu_", "ghs_", "ghr_", "github_pat_"];
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let clean: Vec<&str> = line
        .split(' ')
        .map(|w| if TOKEN_PREFIXES.iter().any(|p| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '_').starts_with(p)) { "***" } else { w })
        .collect();
    clean.join(" ").chars().take(160).collect()
}

/// Zeitlimit je git-Aufruf.
pub const GIT_TIMEOUT: Duration = Duration::from_secs(30);

/// git versteckt und mit Zeitlimit. Bei Erfolg nur stdout (eine Warnung auf stderr darf
/// `rev-parse HEAD` nicht verfälschen), sonst stderr und stdout. Löschende Aufrufe
/// (`worktree remove`, `branch -D`) gehen durch `guard` und sind in Tests gesperrt.
#[derive(Debug, Clone, Copy)]
pub struct TimedGit {
    pub timeout: Duration,
}

impl Default for TimedGit {
    fn default() -> TimedGit {
        TimedGit { timeout: GIT_TIMEOUT }
    }
}

impl Git for TimedGit {
    fn run(&self, args: &[&str], cwd: &Path) -> (i32, String) {
        let mutating = matches!(args, ["worktree", "remove", ..] | ["branch", "-D", ..]);
        if mutating && let Err(e) = crate::guard::check("git worktree remove / branch -D") {
            return (1, e.to_string());
        }
        let mut cmd = Command::new("git");
        cmd.args(["-c", "core.quotepath=false"]).args(args).current_dir(cwd);
        match run(cmd, self.timeout) {
            Ok(o) if o.code == 0 => (0, o.stdout),
            Ok(o) => (o.code, format!("{}{}", o.stderr, o.stdout)),
            Err(e) => (1, e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_texts_keep_one_short_line_and_never_a_token() {
        assert_eq!(short_error("\n  HTTP 401: Bad credentials\nmore"), "HTTP 401: Bad credentials");
        assert_eq!(short_error("token 'gho_abcDEF123' invalid"), "token *** invalid");
        assert_eq!(short_error(&"x".repeat(500)).len(), 160);
        assert_eq!(short_error(""), "");
    }
}
