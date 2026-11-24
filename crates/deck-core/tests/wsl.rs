//! Agenten in WSL: Pfade, Start und die Antworten von `wsl.exe` (`deck_core::wsl`).

use deck_core::wsl::{self, AgentEnv, Wsl};
use serde_json::json;

fn ubuntu() -> Wsl {
    Wsl::new("Ubuntu", "/home/dev", "/mnt/")
}

#[test]
fn a_linux_path_lands_under_the_wsl_share_of_its_distro() {
    assert_eq!(ubuntu().to_windows("/home/dev/repos/api"), r"\\wsl.localhost\Ubuntu\home\dev\repos\api");
    assert_eq!(ubuntu().to_windows("/"), r"\\wsl.localhost\Ubuntu\");
}

#[test]
fn a_mounted_drive_is_the_windows_drive_itself() {
    assert_eq!(ubuntu().to_windows("/mnt/c/Users/Jorrit Voßkühler/Repos"), r"C:\Users\Jorrit Voßkühler\Repos");
    assert_eq!(ubuntu().to_windows("/mnt/d"), r"D:\");
    // /mnt/wsl ist kein Laufwerk, nur ein Ordner mit kurzem Namen
    assert_eq!(ubuntu().to_windows("/mnt/wsl/x"), r"\\wsl.localhost\Ubuntu\mnt\wsl\x");
}

#[test]
fn a_changed_mount_root_from_wsl_conf_is_honoured() {
    let w = Wsl::new("Ubuntu", "/home/dev", &wsl::mount_root_from_conf("[boot]\nsystemd=true\n[automount]\nroot = /\n"));
    assert_eq!(w.to_windows("/c/src"), r"C:\src");
    assert_eq!(w.to_linux(r"C:\src").as_deref(), Some("/c/src"));
    assert_eq!(wsl::mount_root_from_conf("[automount]\nenabled=true\n"), "/mnt/");
}

#[test]
fn windows_paths_and_relative_paths_pass_through_unchanged() {
    assert_eq!(ubuntu().to_windows(r"C:\x"), r"C:\x");
    assert_eq!(ubuntu().to_windows("rel/x"), "rel/x");
}

#[test]
fn a_windows_path_becomes_the_linux_path_the_agent_understands() {
    let w = ubuntu();
    assert_eq!(w.to_linux(r"\\wsl.localhost\Ubuntu\home\dev\repos\").as_deref(), Some("/home/dev/repos"));
    assert_eq!(w.to_linux(r"\\wsl$\ubuntu\home\dev").as_deref(), Some("/home/dev"), "alter Name, andere Schreibung");
    assert_eq!(w.to_linux("//wsl.localhost/Ubuntu/tmp").as_deref(), Some("/tmp"));
    assert_eq!(w.to_linux(r"C:\Users\x\AppData\Local\claude-agent-deck\state\A1.worktree").as_deref(), Some("/mnt/c/Users/x/AppData/Local/claude-agent-deck/state/A1.worktree"));
    assert_eq!(w.to_linux("/home/dev").as_deref(), Some("/home/dev"));
}

#[test]
fn a_path_in_another_distro_or_on_a_network_share_has_no_linux_path() {
    let w = ubuntu();
    assert_eq!(w.to_linux(r"\\wsl.localhost\Debian\home"), None);
    assert_eq!(w.to_linux(r"\\server\share\x"), None);
    assert_eq!(w.to_linux("relativ"), None);
}

#[test]
fn the_round_trip_keeps_every_path() {
    let w = ubuntu();
    for p in ["/home/dev/repos/wt/4116-ui", "/mnt/c/Users/dev", "/"] {
        assert_eq!(w.to_linux(&w.to_windows(p)).as_deref(), Some(p));
    }
}

#[test]
fn claudes_folder_in_the_distro_is_read_over_the_share() {
    assert_eq!(ubuntu().claude_dir().as_deref(), Some(r"\\wsl.localhost\Ubuntu\home\dev\.claude"));
    assert_eq!(ubuntu().claude_json().as_deref(), Some(r"\\wsl.localhost\Ubuntu\home\dev\.claude.json"));
    assert_eq!(Wsl::new("Ubuntu", "", "").claude_dir(), None, "ohne Home kein geratener Ordner");
}

#[test]
fn the_path_fields_of_a_hook_input_become_windows_paths_and_the_rest_stays() {
    let mut data = json!({
        "cwd": "/home/dev/repos/api",
        "transcript_path": "/home/dev/.claude/projects/-home-dev-repos-api/abc.jsonl",
        "workspace": {"current_dir": "/mnt/c/src", "project_dir": "/home/dev/repos/api"},
        "prompt": "/home/dev bleibt Text",
        "session_id": "abc"
    });
    ubuntu().localize_hook_input(data.as_object_mut().unwrap());
    assert_eq!(data["cwd"], r"\\wsl.localhost\Ubuntu\home\dev\repos\api");
    assert_eq!(data["transcript_path"], r"\\wsl.localhost\Ubuntu\home\dev\.claude\projects\-home-dev-repos-api\abc.jsonl");
    assert_eq!(data["workspace"]["current_dir"], r"C:\src");
    assert_eq!(data["prompt"], "/home/dev bleibt Text");
}

#[test]
fn a_gitdir_written_by_linux_git_is_found_beside_the_folder_it_was_read_from() {
    let base = r"\\wsl.localhost\Ubuntu\home\dev\repos\wt\4116-ui";
    assert_eq!(wsl::beside(base, "/home/dev/repos/api/.git/worktrees/4116-ui").as_deref(), Some(r"\\wsl.localhost\Ubuntu\home\dev\repos\api\.git\worktrees\4116-ui"));
    assert_eq!(wsl::beside(r"C:\src\wt\x", "/mnt/c/src/api/.git/worktrees/x").as_deref(), Some(r"C:\src\api\.git\worktrees\x"));
    assert_eq!(wsl::beside(r"C:\src\wt\x", "/home/dev/api/.git"), None, "Distro unbekannt");
    assert_eq!(wsl::beside(base, "C:/src/.git"), None, "kein Linux-Pfad");
}

#[test]
fn wslenv_keeps_what_was_there_and_adds_each_name_once() {
    assert_eq!(wsl::wslenv_with(None, &["AGENT_SLOT", "COLORTERM"]), "AGENT_SLOT:COLORTERM");
    assert_eq!(wsl::wslenv_with(Some("USERPROFILE/p:AGENT_SLOT/u"), &["AGENT_SLOT", "X"]), "USERPROFILE/p:AGENT_SLOT/u:X");
    assert_eq!(wsl::wslenv_with(Some(":"), &["A"]), "A");
}

#[test]
fn wsl_starts_the_script_in_a_login_shell_with_the_arguments_unsplit() {
    let args = vec!["--model".to_string(), "opus[1m]".to_string(), "mach weiter, bitte".to_string()];
    let v = wsl::wsl_argv("Ubuntu", "/home/dev/repos", "/mnt/c/x/wt_agent.sh", &args);
    assert_eq!(v, ["-d", "Ubuntu", "--cd", "/home/dev/repos", "-e", "bash", "-li", "/mnt/c/x/wt_agent.sh", "--model", "opus[1m]", "mach weiter, bitte"]);
}

#[test]
fn the_utf16_answer_of_wsl_exe_reads_as_text() {
    let raw: Vec<u8> = "NAME      STATE   VERSION\r\n* Ubuntu  Running 2\r\n".encode_utf16().flat_map(u16::to_le_bytes).collect();
    assert!(wsl::decode_wsl_output(&raw).contains("* Ubuntu"));
    assert_eq!(wsl::decode_wsl_output(b"/home/dev\n"), "/home/dev\n");
}

#[test]
fn the_default_distro_is_marked_and_docker_internals_are_left_out() {
    let list = "  NAME                   STATE           VERSION\n* Ubuntu-24.04           Running         2\n  docker-desktop         Stopped         2\n  Debian                 Stopped         2\n";
    assert_eq!(wsl::parse_distros(list), vec![("Ubuntu-24.04".to_string(), true), ("Debian".to_string(), false)]);
    assert!(wsl::parse_distros("").is_empty());
}

#[test]
fn the_agent_environment_is_wsl_only_with_a_distro() {
    let s = |v: serde_json::Value| v.as_object().unwrap().clone();
    assert_eq!(AgentEnv::from_settings(&s(json!({}))), AgentEnv::Windows);
    assert_eq!(AgentEnv::from_settings(&s(json!({"agent_env": "wsl"}))), AgentEnv::Windows, "ohne Distro kein WSL");
    let env = AgentEnv::from_settings(&s(json!({"agent_env": "wsl", "wsl_distro": "Ubuntu", "wsl_home": "/home/dev/"})));
    assert_eq!(env, AgentEnv::Wsl(Wsl::new("Ubuntu", "/home/dev", "")));
    assert_eq!(env.for_agent(r"\\wsl.localhost\Ubuntu\home\dev"), "/home/dev");
    assert_eq!(AgentEnv::Windows.to_windows("/home/x"), "/home/x");
}

#[test]
fn in_wsl_the_repos_folder_is_a_linux_path_read_over_the_share() {
    let s = json!({"agent_env": "wsl", "wsl_distro": "Ubuntu", "repos_root": "/home/dev/repos"});
    let local = deck_core::deck_settings::repos_root_local(s.as_object().unwrap());
    assert_eq!(local, Some(r"\\wsl.localhost\Ubuntu\home\dev\repos".into()));
}

fn spec() -> deck_core::wsl_launch::WslSpec {
    deck_core::wsl_launch::WslSpec {
        slot: "A3".into(),
        state: r"C:\Users\x\AppData\Local\claude-agent-deck\state".into(),
        distro: "Ubuntu".into(),
        mount: "/mnt/".into(),
        cwd: "/home/dev/repos".into(),
        script: "/mnt/c/Users/x/AppData/Local/claude-agent-deck/wt_agent.sh".into(),
        args: vec!["--resume".into(), "0f0e…".into()],
    }
}

#[test]
fn the_launch_spec_survives_base64_and_rejects_an_incomplete_one() {
    use deck_core::wsl_launch::WslSpec;
    let s = spec();
    let back = deck_core::wt::decode_spec(&deck_core::wt::encode_spec(&s.to_json())).unwrap();
    assert_eq!(WslSpec::from_json(&back), Some(s));
    assert_eq!(WslSpec::from_json(&json!({"slot": "A1", "state": "C:/s"})), None);
}

#[test]
fn the_launcher_marks_the_slot_with_its_own_pid_and_hands_the_hook_variables_through_wslenv() {
    let p = deck_core::wsl_launch::plan(&spec(), 4242, 133_000_000_000_000_000, Some("USERPROFILE/p"));
    assert_eq!(p.pidfile, r"C:\Users\x\AppData\Local\claude-agent-deck\state\A3.wtpid");
    let body: serde_json::Value = serde_json::from_str(&p.pidfile_body).unwrap();
    assert_eq!(deck_core::wt::parse_pidfile(&body), Some((4242, 133_000_000_000_000_000)));
    let env: std::collections::HashMap<_, _> = p.set_env.iter().cloned().collect();
    assert_eq!(env["AGENT_SLOT"], "A3");
    assert_eq!(env["AGENT_DECK_SHELL"], "4242.133000000000000000");
    assert_eq!(env["WSLENV"], "USERPROFILE/p:AGENT_SLOT:AGENT_DECK_SHELL:AGENT_DECK_DISTRO:AGENT_DECK_MNT:AGENT_DECK_STATE:COLORTERM");
    assert!(p.remove_env.contains(&"CLAUDE_CODE_CHILD_SESSION") && p.remove_env.contains(&"NO_COLOR"));
    assert_eq!(&p.argv[..4], ["-d", "Ubuntu", "--cd", "/home/dev/repos"]);
    assert_eq!(p.argv.last().unwrap(), "0f0e…");
}

#[test]
fn a_wsl_window_runs_the_launcher_instead_of_powershell() {
    let cmd = vec![r"C:\bin\deck-wsl.exe".to_string(), "SPEC".to_string()];
    let argv = deck_core::wt::wt_argv_cmd("wt.exe", "A1", r"C:\Users\x", &cmd, Some(deck_core::wt::PROFILE), None, None);
    assert_eq!(&argv[argv.len() - 4..], ["-d", r"C:\Users\x", r"C:\bin\deck-wsl.exe", "SPEC"]);
    assert!(!argv.iter().any(|a| a.contains("pwsh")));
}
