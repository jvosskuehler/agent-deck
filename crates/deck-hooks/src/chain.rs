//! Die Infozeile, die vor dem Deck da war, läuft weiter: hinter der des Decks.
//!
//! `deck-setup install` setzt immer die Infozeile des Decks ein (Claude Code kennt nur
//! EINE `statusLine`) und sichert eine fremde vorher nach
//! `%LOCALAPPDATA%\claude-agent-deck\statusline.before.json`. Damit niemand vor der
//! Wahl steht, ruft `deck-statusline` dieses alte Kommando bei jedem Lauf mit derselben
//! Eingabe auf und hängt seine Ausgabe an die eigene Zeile an, getrennt durch
//! [`crate::statusline::SEP`].
//!
//! Gestartet wird es so, wie Claude Code es gestartet hätte: unter Windows in Git Bash
//! (dorthin schreibt man solche Zeilen), ersatzweise mit `cmd`. Es bekommt höchstens
//! [`TIMEOUT`]; was länger braucht, scheitert oder nichts ausgibt, fällt still weg. Die
//! Zeile des Decks kommt immer.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use deck_core::claude::hook_setup::{Owner, owner};
use deck_core::paths::{STATUSLINE_BACKUP_FILE, load_json};

/// So lange darf die alte Zeile brauchen. Claude Code ruft die Statuszeile in jedem
/// Pane alle paar Sekunden auf; länger warten hieße, die eigene Zeile hängt mit.
pub const TIMEOUT: Duration = Duration::from_millis(1000);

/// Womit das alte Kommando läuft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shell {
    /// Git Bash (`bash.exe -c <kommando>`), wie Claude Code unter Windows.
    Bash(PathBuf),
    /// `cmd.exe /d /s /c "<kommando>"`, wenn es kein Git Bash gibt.
    Cmd,
}

/// Das gesicherte Kommando aus `<app_dir>\statusline.before.json`; `None`, wenn es keins
/// gibt, es leer ist oder (Schutz vor einer Schleife) dem Deck selbst gehört.
pub fn saved_command(app_dir: &Path) -> Option<String> {
    let v = load_json(&app_dir.join(STATUSLINE_BACKUP_FILE))?;
    let cmd = v.get("statusLine")?.get("command")?.as_str()?.trim();
    (!cmd.is_empty() && owner(cmd) == Owner::Foreign).then(|| cmd.to_string())
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(name)).find(|p| p.is_file())
}

/// Git Bash wie Claude Code: `CLAUDE_CODE_GIT_BASH_PATH`, sonst `bin\bash.exe` neben
/// dem `git.exe` auf dem PATH, sonst der übliche Installationsort.
pub fn find_shell() -> Shell {
    let configured = std::env::var_os("CLAUDE_CODE_GIT_BASH_PATH").map(PathBuf::from);
    let beside_git = which("git.exe").and_then(|g| Some(g.parent()?.parent()?.join("bin").join("bash.exe")));
    let usual = std::env::var_os("ProgramFiles").map(|p| PathBuf::from(p).join("Git").join("bin").join("bash.exe"));
    [configured, beside_git, usual].into_iter().flatten().find(|p| p.is_file()).map_or(Shell::Cmd, Shell::Bash)
}

fn command_for(shell: &Shell, cmd: &str) -> Command {
    match shell {
        Shell::Bash(bash) => {
            let mut c = Command::new(bash);
            c.arg("-c").arg(cmd);
            c
        }
        Shell::Cmd => {
            use std::os::windows::process::CommandExt;
            let mut c = Command::new("cmd.exe");
            c.raw_arg(format!("/d /s /c \"{cmd}\""));
            c
        }
    }
}

/// `cmd` mit `stdin` als Eingabe ausführen und seine Ausgabe ohne abschließende
/// Zeilenumbrüche liefern. `None` bei Fehler, Exit ≠ 0, leerer Ausgabe oder wenn es
/// länger als `timeout` braucht (dann wird es beendet).
pub fn run_command(shell: &Shell, cmd: &str, stdin: &[u8], timeout: Duration) -> Option<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut child = command_for(shell, cmd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .ok()?;
    // Ein- und Ausgabe in eigenen Fäden: ein Kommando, das seine Eingabe nicht liest
    // oder viel ausgibt, darf uns nicht festhalten.
    if let Some(mut input) = child.stdin.take() {
        let data = stdin.to_vec();
        std::thread::spawn(move || {
            let _ = input.write_all(&data);
        });
    }
    let mut out = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                return None;
            }
        }
    };
    if !status.success() {
        return None;
    }
    // Ein Enkelprozess kann die Pipe noch offen halten: nicht länger warten als erlaubt.
    let rest = deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(50));
    let bytes = rx.recv_timeout(rest).ok()?;
    let text = String::from_utf8_lossy(&bytes).trim_end_matches(['\r', '\n']).to_string();
    (!text.trim().is_empty()).then_some(text)
}

/// Die alte Zeile hinter die letzte Zeile des Decks hängen.
pub fn append(own: &str, extra: Option<&str>) -> String {
    match extra {
        Some(x) if own.is_empty() => x.to_string(),
        Some(x) => format!("{own}{}{x}", crate::statusline::SEP),
        None => own.to_string(),
    }
}

/// Die gesicherte Zeile ausführen, falls es eine gibt.
pub fn previous_line(app_dir: &Path, stdin: &[u8]) -> Option<String> {
    let cmd = saved_command(app_dir)?;
    run_command(&find_shell(), &cmd, stdin, TIMEOUT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(dir: &Path, command: &str) {
        let v = serde_json::json!({"statusLine": {"type": "command", "command": command}});
        std::fs::write(dir.join(STATUSLINE_BACKUP_FILE), v.to_string()).unwrap();
    }

    #[test]
    fn only_a_foreign_non_empty_command_is_chained() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(saved_command(d.path()), None, "keine Sicherung");
        saved(d.path(), "  ");
        assert_eq!(saved_command(d.path()), None);
        saved(d.path(), r#""C:/x/bin/deck-statusline.exe" || exit 0"#);
        assert_eq!(saved_command(d.path()), None, "nie sich selbst aufrufen");
        saved(d.path(), "npx ccstatusline");
        assert_eq!(saved_command(d.path()).as_deref(), Some("npx ccstatusline"));
        std::fs::write(d.path().join(STATUSLINE_BACKUP_FILE), "{ kaputt").unwrap();
        assert_eq!(saved_command(d.path()), None);
    }

    #[test]
    fn the_old_command_gets_the_same_input_and_loses_its_trailing_newline() {
        // Großzügige Frist: hier geht es um Eingabe und Zeilenende, nicht ums Tempo. Auf dem
        // GitHub-Runner braucht schon der Start von cmd mal über eine Sekunde (18.11.2026).
        let slow_ok = Duration::from_secs(15);
        let out = run_command(&Shell::Cmd, "findstr meine", b"andere\r\nmeine-zeile\r\n", slow_ok);
        assert_eq!(out.as_deref(), Some("meine-zeile"));
        let out = run_command(&Shell::Cmd, "cmd /c echo meine-zeile", b"", slow_ok);
        assert_eq!(out.as_deref(), Some("meine-zeile"));
    }

    #[test]
    fn a_failing_silent_or_slow_command_is_dropped() {
        assert_eq!(run_command(&Shell::Cmd, "exit 3", b"", TIMEOUT), None);
        assert_eq!(run_command(&Shell::Cmd, "rem nichts", b"", TIMEOUT), None);
        let t = Instant::now();
        assert_eq!(run_command(&Shell::Cmd, "ping -n 6 127.0.0.1 >nul & echo spaet", b"", Duration::from_millis(300)), None);
        assert!(t.elapsed() < Duration::from_secs(3), "nicht auf das Ende gewartet: {:?}", t.elapsed());
        assert_eq!(run_command(&Shell::Bash(PathBuf::from(r"C:\gibt\es\nicht\bash.exe")), "echo x", b"", TIMEOUT), None);
    }

    #[test]
    fn the_old_line_follows_the_last_line_of_the_deck() {
        assert_eq!(append("B2 · Titel\nOpus  ·  ctx 3%", Some("meine-zeile")), "B2 · Titel\nOpus  ·  ctx 3%  ·  meine-zeile");
        assert_eq!(append("", Some("meine-zeile")), "meine-zeile");
        assert_eq!(append("Opus", None), "Opus");
    }
}
