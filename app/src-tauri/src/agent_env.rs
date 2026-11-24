//! Wo die Agenten laufen (Windows oder WSL), einmal beim Start bestimmt.
//!
//! Die Umgebung steht in `deck_settings.json` (`agent_env`, `wsl_distro`, …,
//! `deck_core::wsl::AgentEnv`). Fehlen bei WSL Home oder Einhängepunkt (von Hand
//! eingetragen, Distro gewechselt), fragt das Deck die Distro einmal selbst
//! (`deck_services::wsl_cli::probe`) und schreibt das Ergebnis zurück. Ein Wechsel der
//! Umgebung im Einstellungsfenster gilt ab dem nächsten Start des Decks: Claudes Ordner,
//! Usage und Zusammenfassung hängen daran.

use std::path::{Path, PathBuf};

use deck_core::deck_settings as ds;
use deck_core::paths::{SETTINGS_FILE, load_json, save_json};
use deck_core::wsl::AgentEnv;
use deck_services::wsl_cli::{WslCli, probe};
use serde_json::Value;

use crate::applog::note;

/// Die Umgebung aus `<app_dir>\deck_settings.json`; bei WSL ohne Home wird die Distro
/// gefragt (`cli`) und das Ergebnis gespeichert.
pub fn resolve(app_dir: &Path, cli: &dyn WslCli) -> AgentEnv {
    let file = app_dir.join(SETTINGS_FILE);
    let Some(Value::Object(mut s)) = load_json(&file) else { return AgentEnv::Windows };
    let env = AgentEnv::from_settings(&s);
    let Some(w) = env.wsl() else { return env };
    if !w.home.is_empty() {
        note(format!("Agenten laufen in WSL ({})", w.distro));
        return env;
    }
    match probe(cli, &w.distro) {
        Some(found) => {
            s.insert(ds::WSL_HOME.into(), found.home.clone().into());
            s.insert(ds::WSL_MOUNT_ROOT.into(), found.mount_root.clone().into());
            if let Err(e) = save_json(&file, &Value::Object(s)) {
                note(format!("WSL: Home nicht gespeichert: {e}"));
            }
            note(format!("Agenten laufen in WSL ({}), Home ermittelt", found.distro));
            AgentEnv::Wsl(found)
        }
        None => {
            // Ohne Home kein Claude-Ordner: Usage und Zusammenfassung bleiben leer, die
            // Agenten starten trotzdem (das Startskript fragt die Distro selbst).
            note(format!("WSL_DISTRO_SILENT: Distro {} antwortet nicht, Claude-Ordner unbekannt", w.distro));
            env
        }
    }
}

/// Claudes Ordner der Agenten: in WSL der in der Distro (`\\wsl.localhost\…`), sonst
/// `None` (dann gilt `deck_core::paths::claude_dir`).
pub fn claude_dir(env: &AgentEnv) -> Option<PathBuf> {
    env.wsl().and_then(|w| w.claude_dir()).map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use super::*;

    struct Fake(Mutex<u32>, &'static str);
    impl WslCli for Fake {
        fn run(&self, _: &[String], _: Duration) -> Result<(i32, Vec<u8>), String> {
            *self.0.lock().unwrap() += 1;
            Ok((0, self.1.as_bytes().to_vec()))
        }
    }

    #[test]
    fn a_missing_wsl_home_is_asked_once_and_remembered() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join(SETTINGS_FILE);
        std::fs::write(&file, r#"{"agent_env": "wsl", "wsl_distro": "Ubuntu"}"#).unwrap();
        let cli = Fake(Mutex::new(0), "/home/dev\n/mnt/c/\n");
        let env = resolve(d.path(), &cli);
        assert_eq!(env.wsl().unwrap().home, "/home/dev");
        assert_eq!(load_json(&file).unwrap()["wsl_home"], "/home/dev");
        resolve(d.path(), &cli);
        assert_eq!(*cli.0.lock().unwrap(), 1, "beim zweiten Start steht es in der Datei");
    }

    #[test]
    fn without_wsl_in_the_settings_nothing_is_asked() {
        let d = tempfile::tempdir().unwrap();
        let cli = Fake(Mutex::new(0), "");
        assert_eq!(resolve(d.path(), &cli), AgentEnv::Windows);
        std::fs::write(d.path().join(SETTINGS_FILE), r#"{"repos_root": "C:/r"}"#).unwrap();
        assert_eq!(resolve(d.path(), &cli), AgentEnv::Windows);
        assert_eq!(*cli.0.lock().unwrap(), 0);
    }
}
