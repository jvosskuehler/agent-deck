//! Alles wieder heraus, was die Einrichtung hinzugefügt hat (`deck-setup uninstall`).
//!
//! Was am Namen eindeutig dem Deck gehört (Hooks, Infozeile, Startmenü-Verknüpfung,
//! Terminal-Fragment, Autostart, Eintrag unter „Apps“), geht immer, auch ohne
//! `installed.json` (Installation aus der Zeit davor). Was auch dem Menschen gehören
//! könnte (Claude-Vorgaben, Schrift, Theme, Einstellungsdateien), geht nur laut
//! `installed.json` und nur unverändert. Die Daten des Decks und seine Programme räumt
//! `uninstall.ps1` danach selbst.

use std::path::PathBuf;

use crate::installed::Installed;
use crate::look::{self, LookPaths};
use crate::report::Report;
use crate::user_system::UserSystem;
use crate::{apps_entry, autostart, install, shortcut, statusline_backup, wt_fragment};

/// Wo die Einrichtung ihre Spuren hat.
#[derive(Debug, Clone)]
pub struct Targets {
    pub settings: PathBuf,
    pub saved_statusline: PathBuf,
    pub shortcut: PathBuf,
    pub fragment: PathBuf,
    pub look: LookPaths,
}

impl Targets {
    pub fn real() -> Targets {
        Targets {
            settings: install::settings_path(),
            saved_statusline: statusline_backup::path(),
            shortcut: shortcut::link_path(),
            fragment: wt_fragment::fragment_path(),
            look: LookPaths::real(),
        }
    }
}

/// Alles in einem Lauf; ein Schritt, der scheitert, hält die anderen nicht auf. `m` ist
/// danach leer bis auf das, was nicht entfernt werden konnte.
pub fn uninstall_all(t: &Targets, sys: &dyn UserSystem, m: &mut Installed) -> Report {
    let mut r = install::uninstall(&t.settings, &t.saved_statusline, m);
    r.append(look::remove(&t.look, sys, m));
    let link = shortcut::remove(&t.shortcut);
    if link.is_ok() {
        m.shortcut = None;
    }
    r.append(link);
    r.append(wt_fragment::remove(&t.fragment, m));
    r.append(autostart::remove(sys, m));
    r.append(apps_entry::remove(sys, m));
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::StatusLine;
    use crate::user_system::mem::MemSystem;
    use crate::user_system::{FONTS_KEY, RUN_KEY, UNINSTALL_KEY};
    use serde_json::json;

    #[test]
    fn uninstall_takes_out_everything_the_installer_added_and_leaves_the_rest() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        let bin = root.join("deck").join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("agent-deck.exe"), b"exe").unwrap();
        let src = root.join("papier");
        std::fs::create_dir_all(src.join("fonts")).unwrap();
        for (file, _) in look::FONTS {
            std::fs::write(src.join("fonts").join(file), file).unwrap();
        }
        for f in ["claude-theme-papier.json", "agent-deck-wt.settings.json", look::SHADER] {
            std::fs::write(src.join(f), f).unwrap();
        }
        let t = Targets {
            settings: root.join("claude").join("settings.json"),
            saved_statusline: root.join("deck").join("statusline.before.json"),
            shortcut: root.join("Programs").join(shortcut::LINK_NAME),
            fragment: root.join("Fragments").join(wt_fragment::SOURCE).join(wt_fragment::FILE),
            look: LookPaths { claude_dir: root.join("claude"), app_dir: root.join("deck"), fonts_dir: root.join("Fonts") },
        };
        let foreign = json!({"type": "command", "command": "npx ccstatusline"});
        std::fs::create_dir_all(root.join("claude")).unwrap();
        let before = json!({"model": "opus", "statusLine": foreign});
        deck_core::claude::settings::save(before.as_object().unwrap(), &t.settings).unwrap();

        let sys = MemSystem::default();
        let mut m = Installed::default();
        let file = root.join("deck").join(crate::installed::FILE);
        assert!(install::install(&t.settings, &bin, StatusLine::Replace, &t.saved_statusline, &mut m).is_ok());
        assert!(look::install(&src, &t.look, &sys, &mut m).is_ok());
        let fragment = wt_fragment::fragment(None, wt_fragment::shell_commandline(false), None);
        assert!(wt_fragment::install(&t.fragment, &root.join("wt.json"), &fragment, &mut m).is_ok());
        assert!(autostart::set(&bin, &sys, &mut m).is_ok());
        assert!(apps_entry::set(&bin, "1.0.0", &sys, &mut m).is_ok());
        m.save(&file).unwrap();

        let mut m = Installed::load(&file);
        let r = uninstall_all(&t, &sys, &mut m);
        assert!(r.is_ok(), "{r:?}");
        m.save(&file).unwrap();
        assert!(!file.exists(), "nichts mehr verzeichnet: {m:?}");
        let after = deck_core::claude::settings::load(&t.settings);
        assert_eq!(serde_json::Value::Object(after), before, "Claude-Einstellungen wie vorher, samt der alten Infozeile");
        assert!(!t.saved_statusline.exists());
        assert!(sys.user_values(FONTS_KEY).is_empty() && sys.user_values(RUN_KEY).is_empty());
        assert!(!sys.has_key(UNINSTALL_KEY));
        assert!(!t.fragment.exists() && !t.look.theme().exists() && !t.look.shader().exists() && !t.look.wt_settings().exists());
        assert!(std::fs::read_dir(&t.look.fonts_dir).unwrap().next().is_none(), "Schriftordner leer");
    }
}
