use super::*;
use crate::user_system::mem::MemSystem;

struct Rig {
    _d: tempfile::TempDir,
    src: PathBuf,
    p: LookPaths,
    sys: MemSystem,
    m: Installed,
}

impl Rig {
    fn new() -> Rig {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("papier");
        std::fs::create_dir_all(src.join("fonts")).unwrap();
        for (file, _) in FONTS {
            std::fs::write(src.join("fonts").join(file), format!("ttf {file}")).unwrap();
        }
        std::fs::write(src.join(THEME), "{\"name\": \"Papier\"}").unwrap();
        std::fs::write(src.join(WT_SETTINGS), "{\"theme\": \"custom:papier\"}").unwrap();
        std::fs::write(src.join(SHADER), "float4 main() {}").unwrap();
        let p = LookPaths { claude_dir: d.path().join("claude"), app_dir: d.path().join("deck"), fonts_dir: d.path().join("Fonts") };
        Rig { _d: d, src, p, sys: MemSystem::default(), m: Installed::default() }
    }

    fn install(&mut self) -> Report {
        install(&self.src, &self.p, &self.sys, &mut self.m)
    }

    fn remove(&mut self) -> Report {
        remove(&self.p, &self.sys, &mut self.m)
    }

    fn font_value(&self, name: &str) -> Option<String> {
        self.sys.user_value(FONTS_KEY, name)
    }
}

#[test]
fn a_fresh_install_brings_fonts_theme_agent_settings_and_shader_and_records_them() {
    let mut g = Rig::new();
    let r = g.install();
    assert!(r.is_ok() && r.changed, "{r:?}");
    for (file, name) in FONTS {
        let dst = g.p.fonts_dir.join(file);
        assert!(dst.is_file(), "{file}");
        assert_eq!(g.font_value(name).as_deref(), Some(dst.to_string_lossy().as_ref()));
    }
    assert_eq!(g.sys.session.borrow().len(), 4, "in der Sitzung geladen");
    assert_eq!(std::fs::read_to_string(g.p.theme()).unwrap(), "{\"name\": \"Papier\"}");
    assert_eq!(std::fs::read_to_string(g.p.wt_settings()).unwrap(), "{\"theme\": \"custom:papier\"}");
    assert!(g.p.shader().is_file());
    assert_eq!((g.m.fonts.len(), g.m.files.len()), (4, 3));
    let again = g.install();
    assert!(!again.changed, "zweiter Lauf: nichts zu tun {again:?}");
    assert_eq!(g.sys.session.borrow().len(), 4);
}

#[test]
fn a_font_that_already_exists_for_the_user_or_the_system_is_left_alone() {
    let mut g = Rig::new();
    g.sys.set(FONTS_KEY, "IBM Plex Mono (TrueType)", &RegValue::Str(r"C:\Users\x\Fonts\IBMPlexMono-Regular.ttf".into())).unwrap();
    g.sys.machine = [(FONTS_KEY.to_lowercase(), [("Plex Fett".to_string(), RegValue::Str("IBMPlexMono-Bold.ttf".into()))].into())].into();
    g.install();
    assert!(!g.p.fonts_dir.join("IBMPlexMono-Regular.ttf").exists());
    assert!(!g.p.fonts_dir.join("IBMPlexMono-Bold.ttf").exists(), "im System schon da");
    assert!(g.p.fonts_dir.join("IBMPlexMono-Italic.ttf").exists());
    assert_eq!(g.m.fonts.len(), 2, "nur, was das Deck selbst installiert hat");
    g.remove();
    assert!(g.font_value("IBM Plex Mono (TrueType)").is_some(), "fremde Anmeldung bleibt");
}

#[test]
fn an_existing_theme_or_agent_settings_file_is_never_overwritten() {
    let mut g = Rig::new();
    std::fs::create_dir_all(g.p.theme().parent().unwrap()).unwrap();
    std::fs::write(g.p.theme(), "mein Theme").unwrap();
    std::fs::write(g.p.wt_settings(), "{\"theme\": \"custom:papier\"}").unwrap();
    g.install();
    assert_eq!(std::fs::read_to_string(g.p.theme()).unwrap(), "mein Theme");
    assert!(g.m.file(&g.p.theme()).is_none());
    assert!(g.m.file(&g.p.wt_settings()).is_none(), "gleicher Inhalt, aber nicht vom Deck angelegt");
    g.remove();
    assert_eq!(std::fs::read_to_string(g.p.theme()).unwrap(), "mein Theme");
    assert!(g.p.wt_settings().exists());
}

#[test]
fn an_update_replaces_an_untouched_file_but_keeps_one_the_user_changed() {
    let mut g = Rig::new();
    g.install();
    std::fs::write(g.p.wt_settings(), "{\"theme\": \"light\"}").unwrap();
    std::fs::write(g.src.join(THEME), "{\"name\": \"Papier 2\"}").unwrap();
    std::fs::write(g.src.join(WT_SETTINGS), "{\"theme\": \"custom:papier2\"}").unwrap();
    g.install();
    assert_eq!(std::fs::read_to_string(g.p.theme()).unwrap(), "{\"name\": \"Papier 2\"}", "unberührt: neue Fassung");
    assert_eq!(std::fs::read_to_string(g.p.wt_settings()).unwrap(), "{\"theme\": \"light\"}", "geändert: bleibt");
}

#[test]
fn remove_takes_out_exactly_what_install_added_and_only_if_unchanged() {
    let mut g = Rig::new();
    let themes = g.p.theme().parent().unwrap().to_path_buf();
    g.install();
    std::fs::write(g.p.wt_settings(), "{\"theme\": \"light\"}").unwrap();
    let r = g.remove();
    assert!(r.is_ok(), "{r:?}");
    for (file, name) in FONTS {
        assert!(g.font_value(name).is_none(), "{name}");
        assert!(!g.p.fonts_dir.join(file).exists());
    }
    assert!(g.sys.session.borrow().iter().filter(|(_, load)| !load).count() == 4, "aus der Sitzung entladen");
    assert!(!g.p.theme().exists() && !themes.exists(), "Theme und leerer Ordner weg");
    assert!(!g.p.shader().exists());
    assert!(g.p.wt_settings().exists(), "geändert: gehört jetzt dem Menschen");
    assert!(g.m.is_empty());
    assert!(g.remove().is_ok(), "zweimal ist harmlos");
}

#[test]
fn a_font_still_in_use_is_deleted_at_the_next_login() {
    let mut g = Rig::new();
    g.install();
    // Ein Ordner an Stelle der Datei: löschen scheitert wie bei einer geladenen Schrift.
    let dst = g.p.fonts_dir.join("IBMPlexMono-Regular.ttf");
    std::fs::remove_file(&dst).unwrap();
    std::fs::create_dir_all(dst.join("x")).unwrap();
    let r = g.remove();
    assert_eq!(r.warns(), 1, "{r:?}");
    let RegValue::Str(cmd) = g.sys.get(RUN_ONCE_KEY, "AgentDeck-IBMPlexMono-Regular.ttf").unwrap() else { panic!() };
    assert!(cmd.contains("del /f /q") && cmd.contains("IBMPlexMono-Regular.ttf"), "{cmd}");
    assert!(g.font_value("IBM Plex Mono (TrueType)").is_none(), "abgemeldet ist sie sofort");
}

#[test]
fn without_the_papier_folder_nothing_changes() {
    let mut g = Rig::new();
    let r = install(&g.src.join("fehlt"), &g.p, &g.sys, &mut g.m);
    assert!(r.is_ok() && !r.changed && r.warns() == 1);
    assert!(g.m.is_empty());
}

#[test]
fn the_real_targets_are_locked_in_tests() {
    let g = Rig::new();
    let mut m = Installed::default();
    assert_eq!(install(&g.src, &LookPaths::real(), &g.sys, &mut m).fails(), 1);
    assert_eq!(remove(&LookPaths::real(), &g.sys, &mut m).fails(), 1);
}

#[test]
fn removing_takes_the_emptied_themes_folder_in_the_distro_along() {
    let mut r = Rig::new();
    // Wie `look --wsl`: Claudes Ordner liegt woanders als der von Windows.
    let windows = r.p.clone();
    r.p.claude_dir = r._d.path().join("distro").join(".claude");
    assert!(r.install().is_ok());
    let wsl_themes = r.p.theme().parent().unwrap().to_path_buf();
    assert!(wsl_themes.is_dir());
    let mut m = std::mem::take(&mut r.m);
    assert!(remove(&windows, &r.sys, &mut m).is_ok());
    assert!(!wsl_themes.exists(), "der leere themes-Ordner in der Distro geht mit");
}
