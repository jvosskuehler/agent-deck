//! Die Hooks als Linux-Programme in der WSL-Distro: wohin sie schreiben, was sie lesen.

use deck_core::wsl::Wsl;
use deck_hooks::env::{localize, state_dir_for};
use serde_json::json;

fn ubuntu() -> Wsl {
    Wsl::new("Ubuntu", "", "/mnt/")
}

#[test]
fn as_a_linux_program_the_state_folder_is_reached_through_the_mount() {
    let state = r"C:\Users\dev\AppData\Local\claude-agent-deck\state";
    assert_eq!(state_dir_for(state, Some(&ubuntu()), true), "/mnt/c/Users/dev/AppData/Local/claude-agent-deck/state");
    assert_eq!(state_dir_for(state, Some(&ubuntu()), false), state, "als Windows-Programm bleibt der Windows-Pfad");
    assert_eq!(state_dir_for(state, None, true), state);
}

#[test]
fn as_a_linux_program_the_transcript_stays_readable_and_the_rest_becomes_windows_paths() {
    let input = json!({"cwd": "/home/dev/repos", "transcript_path": "/home/dev/.claude/projects/x/s.jsonl", "workspace": {"current_dir": "/home/dev/repos/api"}});
    let mut native = input.as_object().unwrap().clone();
    localize(&mut native, &ubuntu(), true);
    assert_eq!(native["transcript_path"], "/home/dev/.claude/projects/x/s.jsonl");
    assert_eq!(native["cwd"], r"\\wsl.localhost\Ubuntu\home\dev\repos");
    assert_eq!(native["workspace"]["current_dir"], r"\\wsl.localhost\Ubuntu\home\dev\repos\api");

    let mut interop = input.as_object().unwrap().clone();
    localize(&mut interop, &ubuntu(), false);
    assert_eq!(interop["transcript_path"], r"\\wsl.localhost\Ubuntu\home\dev\.claude\projects\x\s.jsonl", "die .exe liest über die Freigabe");
}
