//! Die gebauten Programme als echte Prozesse: Exit 0 auf jedem Pfad (Claude Code liest
//! alles andere als Veto) und die Statuszeile als UTF-8. Jeder Lauf bekommt ein
//! Wegwerf-`LOCALAPPDATA`; ohne `AGENT_SLOT` fände er sonst über die Prozesskette den
//! Slot der Session, in der die Tests laufen, und überschriebe deren echte Dateien.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn run(exe: &str, args: &[&str], local: &Path, slot: Option<&str>, stdin: &[u8]) -> Output {
    let mut cmd = Command::new(exe);
    cmd.args(args)
        .env("LOCALAPPDATA", local)
        .env_remove("AGENT_SLOT")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(s) = slot {
        cmd.env("AGENT_SLOT", s);
    }
    let mut child = cmd.spawn().expect("Programm startet");
    let _ = child.stdin.take().unwrap().write_all(stdin);
    child.wait_with_output().unwrap()
}

const REPORT: &str = env!("CARGO_BIN_EXE_deck-report");
const STATUSLINE: &str = env!("CARGO_BIN_EXE_deck-statusline");

#[test]
fn the_report_program_writes_the_state_file_and_exits_zero() {
    let local = tempfile::tempdir().unwrap();
    let out = run(REPORT, &["thinking"], local.path(), Some("Q7"), r#"{"prompt": "Größe für Ü"}"#.as_bytes());
    assert_eq!(out.status.code(), Some(0));
    let p = local.path().join("claude-agent-deck").join("state").join("Q7.json");
    let rec: serde_json::Value = serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap();
    assert_eq!((rec["status"].as_str(), rec["prompt"].as_str()), (Some("thinking"), Some("Größe für Ü")));
}

#[test]
fn every_path_ends_with_exit_zero_even_when_writing_is_impossible() {
    let local = tempfile::tempdir().unwrap();
    // Ein Ordner kann nicht angelegt werden, wo schon eine Datei liegt.
    let blocked = local.path().join("datei");
    std::fs::write(&blocked, "x").unwrap();
    for (args, stdin) in [(vec!["done"], &b"{kaputt"[..]), (vec![], &b""[..]), (vec!["waiting"], &b"[1]"[..]), (vec!["idle"], &[0xff, 0xfe][..])] {
        assert_eq!(run(REPORT, &args, &blocked, Some("Q1"), stdin).status.code(), Some(0), "{args:?}");
        assert_eq!(run(REPORT, &args, local.path(), None, stdin).status.code(), Some(0), "{args:?}");
        assert_eq!(run(STATUSLINE, &[], &blocked, Some("Q1"), stdin).status.code(), Some(0), "{args:?}");
    }
}

#[test]
fn the_status_line_arrives_as_utf8_even_with_umlauts() {
    let local = tempfile::tempdir().unwrap();
    let data = r#"{"model": {"display_name": "Opus"}, "session_name": "Eigenes Terminal für Claude Code"}"#.as_bytes();
    let out = run(STATUSLINE, &[], local.path(), None, data);
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8(out.stdout).expect("UTF-8");
    assert_eq!(text, "\x1b[1mEigenes Terminal für Claude Code\x1b[0m\nOpus");
}
