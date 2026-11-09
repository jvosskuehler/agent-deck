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
/// Stil des Rings um den aktiven Chat (VS Code), ebenso eingebaut.
pub const GLOW_CSS: &str = include_str!("../../../resources/agent-deck-glow.css");
pub const GLOW_CSS_FILE: &str = "agent-deck-glow.css";
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

    pub fn glow_css(&self) -> PathBuf {
        self.app_dir.join(GLOW_CSS_FILE)
    }

    pub fn agent_icon(&self) -> PathBuf {
        self.app_dir.join(AGENT_ICON_FILE)
    }
}

/// Wo das Python-Deck seine Laufzeitdateien hatte. `DECK_LEGACY_ROOT` überschreibt;
/// sonst `~/Repos/agent-deck`, dann der alte Ort auf dem Desktop.
pub fn legacy_roots() -> Vec<PathBuf> {
    if let Ok(p) = std::env::var("DECK_LEGACY_ROOT")
        && !p.trim().is_empty()
    {
        return vec![PathBuf::from(p)];
    }
    let home = paths::home_dir();
    vec![home.join("Repos").join("agent-deck"), home.join("Desktop").join("Programmchen").join("agent-deck")]
}

/// Merker im App-Ordner: ab hier gehören die Laufzeitdateien dieser Fassung.
pub const TAKEOVER_MARKER: &str = "python-adopted";

/// Einmal je Start: Laufzeitdateien aus dem Python-Repo übernehmen und das Startskript
/// der Terminal-Agenten ablegen.
///
/// Solange das Python-Deck die Dateien führt (kein Merker), ist SEIN Stand der richtige:
/// dann wird bei jedem Start neu übernommen, auch über vorhandene Dateien hinweg (ein
/// abweichender Stand bleibt als `<name>.bak` liegen; im App-Ordner lagen schon Reste
/// früherer Versuche). `take_over` (das Deck läuft mit dem echten Broker, das alte ist
/// also aus) übernimmt ein letztes Mal und setzt den Merker; danach wird nur noch
/// ergänzt, was fehlt, und nie überschrieben.
pub fn prepare(p: &Paths, legacy: &[PathBuf], take_over: bool) {
    let _ = fs::create_dir_all(&p.state_dir);
    let marker = p.app_dir.join(TAKEOVER_MARKER);
    let python_owns = !marker.exists();
    for root in legacy.iter().filter(|r| r.is_dir()) {
        if python_owns {
            refresh_from(root, &p.app_dir);
        }
        match paths::adopt_legacy_files(root, &p.app_dir) {
            Ok(names) if !names.is_empty() => note(format!("aus {} übernommen: {}", root.display(), names.join(", "))),
            Ok(_) => {}
            Err(e) => note(format!("Übernahme aus {} fehlgeschlagen: {e}", root.display())),
        }
    }
    if python_owns && take_over && legacy.iter().any(|r| r.is_dir()) {
        let _ = paths::atomic_write(&marker, crate::now().to_string().as_bytes());
        note("Laufzeitdateien gehören ab jetzt dieser Fassung");
    }
    let built_in: [(PathBuf, &[u8]); 3] =
        [(p.wt_agent_script(), WT_AGENT_SCRIPT.as_bytes()), (p.glow_css(), GLOW_CSS.as_bytes()), (p.agent_icon(), AGENT_ICON)];
    for (path, bytes) in built_in {
        if fs::read(&path).ok().as_deref() != Some(bytes)
            && let Err(e) = paths::atomic_write(&path, bytes)
        {
            note(format!("{} nicht schreibbar: {e}", path.display()));
        }
    }
}

/// Abweichende Dateien im App-Ordner beiseitelegen, damit `adopt_legacy_files` den
/// Stand des Python-Decks übernimmt.
fn refresh_from(root: &Path, app_dir: &Path) {
    for name in paths::RUNTIME_FILES {
        let (src, dst) = (root.join(name), app_dir.join(name));
        let Ok(new) = fs::read(&src) else { continue };
        match fs::read(&dst) {
            Ok(old) if old != new => {
                let _ = fs::rename(&dst, app_dir.join(format!("{name}.bak")));
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_files_win_until_the_takeover_and_are_never_overwritten_after() {
        let tmp = tempfile::tempdir().unwrap();
        let p = Paths::under(&tmp.path().join("app"));
        let legacy = tmp.path().join("legacy");
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("bindings.json"), r#"{"A": "repo"}"#).unwrap();
        fs::create_dir_all(&p.app_dir).unwrap();
        fs::write(p.app_dir.join("tickets.json"), "{}").unwrap();
        fs::write(legacy.join("tickets.json"), r#"{"A1": "X-1"}"#).unwrap();

        // Probebetrieb: das Python-Deck führt die Dateien, sein Stand gewinnt.
        prepare(&p, &[legacy.clone(), tmp.path().join("missing")], false);
        assert_eq!(fs::read_to_string(p.app_dir.join("bindings.json")).unwrap(), r#"{"A": "repo"}"#);
        assert_eq!(fs::read_to_string(p.app_dir.join("tickets.json")).unwrap(), r#"{"A1": "X-1"}"#);
        assert_eq!(fs::read_to_string(p.app_dir.join("tickets.json.bak")).unwrap(), "{}", "alter Stand bleibt liegen");
        assert_eq!(fs::read_to_string(p.wt_agent_script()).unwrap(), WT_AGENT_SCRIPT);
        assert_eq!(fs::read(p.agent_icon()).unwrap(), AGENT_ICON);
        assert!(p.state_dir.is_dir());
        assert!(!p.app_dir.join(TAKEOVER_MARKER).exists());

        // Übernahme: ein letztes Mal, danach gehören die Dateien dieser Fassung.
        fs::write(legacy.join("tickets.json"), r#"{"A1": "X-2"}"#).unwrap();
        prepare(&p, std::slice::from_ref(&legacy), true);
        assert_eq!(fs::read_to_string(p.app_dir.join("tickets.json")).unwrap(), r#"{"A1": "X-2"}"#);
        assert!(p.app_dir.join(TAKEOVER_MARKER).exists());
        fs::write(p.app_dir.join("tickets.json"), "{}").unwrap();
        fs::write(legacy.join("tickets.json"), r#"{"A1": "X-3"}"#).unwrap();
        prepare(&p, &[legacy], true);
        assert_eq!(fs::read_to_string(p.app_dir.join("tickets.json")).unwrap(), "{}", "nie mehr überschreiben");
    }
}
