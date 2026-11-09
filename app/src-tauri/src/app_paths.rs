//! Wo die App ihre Dateien findet, und was beim Start einmal eingerichtet wird.
//!
//! Alles hängt an einem [`Paths`]-Wert, den `lib.rs` ganz oben aus den Vorgaben baut.
//! Tests reichen einen Wegwerf-Ordner herein und erreichen so nie die echten Dateien.

use std::fs;
use std::path::{Path, PathBuf};

use deck_core::paths;

use crate::applog::note;

/// Startskript der Windows-Terminal-Agenten (aus `deck/ops/wt_agent.ps1`). Fest in die
/// App eingebaut und beim Start in den App-Ordner geschrieben: so liegt es im
/// Entwicklungsbuild wie in der installierten App an derselben Stelle, ohne
/// Ressourcen-Konfiguration.
pub const WT_AGENT_SCRIPT: &str = include_str!("../../../resources/wt_agent.ps1");
pub const WT_AGENT_FILE: &str = "wt_agent.ps1";
/// Taskleisten-Symbol der Terminal-Agenten (`tools/make_icons.py`), ebenso eingebaut.
/// Windows liest es aus einer Datei, darum muss es auf der Platte liegen.
pub const AGENT_ICON: &[u8] = include_bytes!("../../../resources/agent-taskbar.ico");
pub const AGENT_ICON_FILE: &str = "agent-taskbar.ico";

#[derive(Debug, Clone)]
pub struct Paths {
    /// `%LOCALAPPDATA%\claude-agent-deck`: Laufzeitdateien (bindings.json …).
    pub app_dir: PathBuf,
    /// `…\state`: hier schreiben die Hooks.
    pub state_dir: PathBuf,
    /// `~/.claude/settings.json`.
    pub claude_settings: PathBuf,
    /// `~/.claude/projects`: Transcripts.
    pub projects_dir: PathBuf,
    /// Ordner der laufenden Exe; daneben liegen deck-console, deck-report …
    pub exe_dir: PathBuf,
}

impl Paths {
    pub fn from_env() -> Paths {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."));
        Paths {
            app_dir: paths::app_dir(),
            state_dir: paths::state_dir(),
            claude_settings: deck_core::claude::settings::settings_path(),
            projects_dir: deck_core::claude::summary_cache::projects_dir(),
            exe_dir,
        }
    }

    /// Für Tests: alles unter einem Ordner.
    #[cfg(test)]
    pub fn under(root: &Path) -> Paths {
        Paths {
            app_dir: root.to_path_buf(),
            state_dir: root.join("state"),
            claude_settings: root.join("claude").join("settings.json"),
            projects_dir: root.join("claude").join("projects"),
            exe_dir: root.join("bin"),
        }
    }

    pub fn wt_agent_script(&self) -> PathBuf {
        self.app_dir.join(WT_AGENT_FILE)
    }

    pub fn agent_icon(&self) -> PathBuf {
        self.app_dir.join(AGENT_ICON_FILE)
    }
}

/// Einmal je Start: Zustandsordner anlegen und die eingebauten Dateien (Startskript der
/// Terminal-Agenten, Taskleisten-Symbol) ablegen. Geschrieben wird nur, was abweicht:
/// so ersetzt ein Update eine alte Fassung, ohne bei jedem Start die Platte anzufassen.
pub fn prepare(p: &Paths) {
    let _ = fs::create_dir_all(&p.state_dir);
    let built_in: [(PathBuf, &[u8]); 2] = [(p.wt_agent_script(), WT_AGENT_SCRIPT.as_bytes()), (p.agent_icon(), AGENT_ICON)];
    for (path, bytes) in built_in {
        if fs::read(&path).ok().as_deref() != Some(bytes)
            && let Err(e) = paths::atomic_write(&path, bytes)
        {
            note(format!("{} nicht schreibbar: {e}", path.display()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_files_land_in_the_app_folder_and_a_stale_copy_is_replaced() {
        let tmp = tempfile::tempdir().unwrap();
        let p = Paths::under(&tmp.path().join("app"));
        fs::create_dir_all(&p.app_dir).unwrap();
        fs::write(p.wt_agent_script(), "alt").unwrap();

        prepare(&p);
        assert!(p.state_dir.is_dir());
        assert_eq!(fs::read_to_string(p.wt_agent_script()).unwrap(), WT_AGENT_SCRIPT);
        assert_eq!(fs::read(p.agent_icon()).unwrap(), AGENT_ICON);
    }
}
