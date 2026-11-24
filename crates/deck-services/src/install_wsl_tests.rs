use std::sync::Mutex;
use std::time::Duration;

use serde_json::json;

use super::*;

/// Merkt sich die Aufrufe von `wsl.exe` und antwortet mit `code`; startet nie etwas.
struct Fake {
    code: i32,
    calls: Mutex<Vec<Vec<String>>>,
}

impl Fake {
    fn ok() -> Fake {
        Fake { code: 0, calls: Mutex::new(Vec::new()) }
    }
}

impl WslCli for Fake {
    fn run(&self, args: &[String], _: Duration) -> Result<(i32, Vec<u8>), String> {
        self.calls.lock().unwrap().push(args.to_vec());
        Ok((self.code, Vec::new()))
    }
}

/// Eine „Distro“ im Wegwerf-Ordner: `settings` dort, wo sonst `\\wsl.localhost\…` steht.
fn target(root: &Path) -> WslTarget {
    let wsl = Wsl::new("Ubuntu", "/home/dev", "/mnt/");
    WslTarget {
        settings: root.join("distro").join(".claude").join("settings.json"),
        bin_linux: "/mnt/c/Users/dev/AppData/Local/claude-agent-deck/bin".into(),
        saved: root.join("deck").join(deck_core::paths::STATUSLINE_BACKUP_WSL_FILE),
        wsl,
        linux_bin: None,
    }
}

#[test]
fn the_target_points_at_claudes_folder_in_the_distro_and_at_bin_through_the_mount() {
    let t = WslTarget::new(Wsl::new("Ubuntu", "/home/dev", "/mnt/"), Path::new(r"C:\Users\dev\AppData\Local\claude-agent-deck\bin"), Path::new(r"C:\d")).unwrap();
    assert_eq!(t.settings, PathBuf::from(r"\\wsl.localhost\Ubuntu\home\dev\.claude\settings.json"));
    assert_eq!(t.bin_linux, "/mnt/c/Users/dev/AppData/Local/claude-agent-deck/bin");
    assert!(WslTarget::new(Wsl::new("Ubuntu", "", "/mnt/"), Path::new(r"C:\b"), Path::new(r"C:\d")).is_none(), "ohne Home kein Ziel");
}

#[test]
fn the_hooks_call_the_windows_programs_by_their_linux_path_and_never_veto() {
    let d = tempfile::tempdir().unwrap();
    let t = target(d.path());
    let mut m = Installed::default();
    let r = install(&t, StatusLine::Replace, &mut m);
    assert!(r.is_ok(), "{r:?}");
    let s = deck_core::claude::settings::load(&t.settings);
    let cmd = s["hooks"]["PreToolUse"][0]["hooks"][0]["command"].as_str().unwrap();
    assert_eq!(cmd, r#""/mnt/c/Users/dev/AppData/Local/claude-agent-deck/bin/deck-report.exe" running || exit 0"#);
    assert!(s["statusLine"]["command"].as_str().unwrap().starts_with(r#""/mnt/c/Users/dev/"#));
    let w = m.wsl.as_ref().unwrap();
    assert_eq!(w.distro, "Ubuntu");
    assert!(w.claude_defaults.iter().any(|c| c.key == "permissions.defaultMode"), "Vorgaben der Distro stehen bei ihr");
    assert!(m.claude_defaults.is_empty(), "die Windows-Einträge bleiben unberührt");
}

#[test]
fn a_foreign_statusline_in_the_distro_is_saved_apart_and_comes_back_on_uninstall() {
    let d = tempfile::tempdir().unwrap();
    let t = target(d.path());
    let before = json!({"statusLine": {"type": "command", "command": "~/.claude/line.sh"}});
    deck_core::claude::settings::save(before.as_object().unwrap(), &t.settings).unwrap();
    let mut m = Installed::default();
    assert!(install(&t, StatusLine::Replace, &mut m).is_ok());
    assert!(t.saved.is_file(), "eigene Sicherung für WSL");
    let r = uninstall(&t.saved, &Fake::ok(), &mut m);
    assert!(r.is_ok(), "{r:?}");
    assert_eq!(serde_json::Value::Object(deck_core::claude::settings::load(&t.settings)), before);
    assert!(m.wsl.is_none() && !t.saved.exists());
}

#[test]
fn the_check_asks_the_windows_side_whether_the_programs_exist() {
    let d = tempfile::tempdir().unwrap();
    let t = target(d.path());
    let mut m = Installed::default();
    install(&t, StatusLine::Replace, &mut m);
    // C:\Users\dev\… gibt es auf dem Testrechner nicht: jeder Hook zeigt ins Leere.
    let r = check(&t);
    assert!(r.fails() >= 6, "{r:?}");
}

#[test]
fn with_linux_hooks_in_the_distro_they_run_first_and_the_exe_only_when_they_are_missing() {
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("bin").join("linux");
    std::fs::create_dir_all(&src).unwrap();
    let mut t = target(d.path());
    let cli = Fake::ok();
    assert!(t.place_linux_hooks(&cli, &src).is_err(), "ohne Dateien kein Kopieren");
    for f in LINUX_HOOKS {
        std::fs::write(src.join(f), b"ELF").unwrap();
    }
    // Der Wegwerf-Ordner liegt nicht unter /mnt: für den Test ein Laufwerkspfad genügt.
    t.place_linux_hooks(&cli, &src).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(t.linux_bin.as_deref(), Some("/home/dev/.local/share/agent-deck/bin"));
    let call = cli.calls.lock().unwrap()[0].clone();
    assert!(call.iter().any(|a| a.contains("install -m 755")), "ausführbar kopiert: {call:?}");

    let mut m = Installed::default();
    assert!(install(&t, StatusLine::Replace, &mut m).is_ok());
    let s = deck_core::claude::settings::load(&t.settings);
    let cmd = s["hooks"]["Stop"][0]["hooks"][0]["command"].as_str().unwrap();
    assert_eq!(
        cmd,
        r#""/home/dev/.local/share/agent-deck/bin/deck-report" done 2>/dev/null || "/mnt/c/Users/dev/AppData/Local/claude-agent-deck/bin/deck-report.exe" done || exit 0"#
    );
    let sl = s["statusLine"]["command"].as_str().unwrap();
    assert!(sl.starts_with(r#""/home/dev/.local/share/agent-deck/bin/deck-statusline" 2>/dev/null || "#), "{sl}");

    let un = Fake::ok();
    assert!(uninstall(&t.saved, &un, &mut m).is_ok());
    assert!(un.calls.lock().unwrap()[0].iter().any(|a| a.contains("rm -f")), "Linux-Fassungen wieder weg");
}
