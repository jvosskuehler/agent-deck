use super::*;
use deck_core::claude::hook_setup::{HOOKS, Owner, commands_of, owner};
use serde_json::json;

fn load(p: &Path) -> Map<String, Value> {
    settings::load(p)
}

struct Rig {
    _d: tempfile::TempDir,
    bin: PathBuf,
    settings: PathBuf,
    saved: PathBuf,
}

impl Rig {
    fn new() -> Rig {
        let d = tempfile::tempdir().unwrap();
        let bin = d.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("deck-report.exe"), b"").unwrap();
        std::fs::write(bin.join("deck-statusline.exe"), b"").unwrap();
        let settings = d.path().join("claude").join("settings.json");
        let saved = d.path().join("deck").join("statusline.before.json");
        Rig { _d: d, bin, settings, saved }
    }

    fn install(&self, mode: StatusLine) -> Report {
        install(&self.settings, &self.bin, mode, &self.saved)
    }

    fn uninstall(&self) -> Report {
        uninstall(&self.settings, &self.saved)
    }

    fn with(&self, m: Value) {
        std::fs::create_dir_all(self.settings.parent().unwrap()).unwrap();
        settings::save(m.as_object().unwrap(), &self.settings).unwrap();
    }

    fn statusline(&self) -> Option<Value> {
        load(&self.settings).get("statusLine").cloned()
    }

    fn baks(&self) -> usize {
        let dir = self.settings.parent().unwrap();
        std::fs::read_dir(dir).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().contains(".bak-")).count()
    }
}

fn foreign() -> Value {
    json!({"type": "command", "command": "npx ccstatusline", "padding": 0})
}

#[test]
fn a_fresh_file_gets_all_six_hooks_and_the_statusline() {
    let g = Rig::new();
    let r = g.install(StatusLine::Keep);
    assert!(r.is_ok() && r.changed, "{r:?}");
    let m = load(&g.settings);
    for (event, _, _) in HOOKS {
        assert_eq!(commands_of(&m, event, Owner::Deck).len(), 1, "{event}");
    }
    assert_eq!(owner(m["statusLine"]["command"].as_str().unwrap()), Owner::Deck);
    assert!(r.backup.is_none(), "neu angelegt: nichts zu sichern");
    assert!(!g.saved.exists(), "keine fremde statusLine: nichts zu sichern");
}

#[test]
fn a_second_run_changes_nothing_and_backs_up_nothing() {
    let g = Rig::new();
    g.install(StatusLine::Replace);
    let r = g.install(StatusLine::Replace);
    assert!(!r.changed && r.backup.is_none());
    assert_eq!(g.baks(), 0);
    assert!(!g.saved.exists(), "die eigene statusLine wird nie gesichert");
}

#[test]
fn an_unreadable_settings_file_is_not_touched() {
    let g = Rig::new();
    std::fs::create_dir_all(g.settings.parent().unwrap()).unwrap();
    std::fs::write(&g.settings, "{ kaputt").unwrap();
    let r = g.install(StatusLine::Replace);
    assert_eq!(r.fails(), 1);
    assert_eq!(std::fs::read_to_string(&g.settings).unwrap(), "{ kaputt");
}

#[test]
fn the_check_is_clean_after_install_and_fails_when_the_programs_are_gone() {
    let g = Rig::new();
    g.install(StatusLine::Keep);
    let c = check(&g.settings, &g.bin);
    assert_eq!((c.fails(), c.warns()), (0, 0), "{c:?}");
    std::fs::remove_file(g.bin.join("deck-report.exe")).unwrap();
    assert_eq!(check(&g.settings, &g.bin).fails(), HOOKS.len());
}

#[test]
fn install_keeps_other_keys_and_foreign_hooks_and_uninstall_takes_out_only_ours() {
    let g = Rig::new();
    let lint = json!([{"hooks": [{"type": "command", "command": "python C:/tools/lint.py"}]}]);
    g.with(json!({"model": "opus[1m]", "hooks": {"Stop": lint}}));
    let r = g.install(StatusLine::Keep);
    assert!(r.backup.is_some(), "vorhandene Datei: Sicherung");
    let m = load(&g.settings);
    assert_eq!(m["model"], "opus[1m]", "andere Schlüssel bleiben");
    assert_eq!(commands_of(&m, "Stop", Owner::Foreign).len(), 1);
    assert_eq!(commands_of(&m, "Stop", Owner::Deck).len(), 1);
    g.uninstall();
    let m = load(&g.settings);
    assert!(commands_of(&m, "Stop", Owner::Deck).is_empty());
    assert_eq!(commands_of(&m, "Stop", Owner::Foreign).len(), 1);
    assert!(m.get("statusLine").is_none());
}

#[test]
fn a_foreign_statusline_stays_unless_replace_is_asked_for() {
    let g = Rig::new();
    g.with(json!({"statusLine": foreign()}));
    let r = g.install(StatusLine::Keep);
    assert_eq!(g.statusline(), Some(foreign()));
    assert!(!g.saved.exists());
    assert_eq!(r.warns(), 1, "{r:?}");
    assert!(r.lines.iter().any(|l| l.text.contains("--statusline replace")), "Hinweis nennt den Schalter von deck-setup");
}

#[test]
fn replacing_a_foreign_statusline_saves_it_and_uninstall_puts_it_back() {
    let g = Rig::new();
    g.with(json!({"statusLine": foreign()}));
    let r = g.install(StatusLine::Replace);
    assert!(r.is_ok(), "{r:?}");
    assert_eq!(owner(g.statusline().unwrap()["command"].as_str().unwrap()), Owner::Deck);
    let kept = deck_core::paths::load_json(&g.saved).unwrap();
    assert_eq!(kept["statusLine"], foreign());

    let r = g.uninstall();
    assert!(r.is_ok(), "{r:?}");
    assert_eq!(g.statusline(), Some(foreign()), "genau der alte Wert, samt eigener Felder");
    assert!(!g.saved.exists(), "zurückgelegt: Sicherung weg");
}

#[test]
fn a_statusline_that_already_is_the_decks_is_left_as_it_is() {
    let g = Rig::new();
    g.install(StatusLine::Keep);
    let mut ours = g.statusline().unwrap();
    ours["padding"] = 2.into();
    g.with(json!({"statusLine": ours.clone()}));
    g.install(StatusLine::Replace);
    let now = g.statusline().unwrap();
    assert_eq!(now["padding"], 2, "eigenes Anhängsel bleibt");
    assert!(!g.saved.exists());
}

#[test]
fn uninstall_does_not_overwrite_a_statusline_set_after_the_install() {
    let g = Rig::new();
    g.with(json!({"statusLine": foreign()}));
    g.install(StatusLine::Replace);
    let mut m = load(&g.settings);
    let newer = json!({"type": "command", "command": "starship statusline"});
    m.insert("statusLine".into(), newer.clone());
    settings::save(&m, &g.settings).unwrap();

    let r = g.uninstall();
    assert_eq!(g.statusline(), Some(newer));
    assert!(g.saved.exists(), "die Sicherung bleibt, bis jemand entscheidet");
    assert_eq!(r.warns(), 1, "{r:?}");
}

#[test]
fn a_failed_backup_leaves_the_settings_untouched() {
    let g = Rig::new();
    g.with(json!({"statusLine": foreign()}));
    // Ein Ordner am Platz der Sicherung: sie lässt sich nicht schreiben.
    std::fs::create_dir_all(&g.saved).unwrap();
    let r = g.install(StatusLine::Replace);
    assert_eq!(r.fails(), 1, "{r:?}");
    assert_eq!(g.statusline(), Some(foreign()));
    assert!(commands_of(&load(&g.settings), "Stop", Owner::Deck).is_empty(), "gar nichts geschrieben");
}

#[test]
fn the_mode_is_parsed_from_its_two_words_only() {
    assert_eq!(StatusLine::parse("keep"), Some(StatusLine::Keep));
    assert_eq!(StatusLine::parse("replace"), Some(StatusLine::Replace));
    assert_eq!(StatusLine::parse("force"), None);
    assert_eq!(StatusLine::default(), StatusLine::Keep);
}

#[test]
fn the_real_settings_file_is_locked_in_tests() {
    assert!(check_target(&settings_path()).is_err());
    assert!(check_target(Path::new(r"C:\tmp\x\settings.json")).is_ok());
}
