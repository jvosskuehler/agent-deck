//! Umgebung eines Hook-Laufs und die Slot-Auflösung.
//!
//! Der Slot (A1 … O9) kommt aus `AGENT_SLOT` in der Umgebung: das Startskript im
//! Windows-Terminal-Fenster des Agenten (`wt_agent.ps1`) setzt ihn. Ohne ihn meldet der
//! Hook nichts: eine selbst gestartete Session gehört nicht zum Deck.
//!
//! Alles, was nach außen greift (Ordner, Uhr), steckt in `HookEnv`; Tests bauen sich
//! eine eigene mit Wegwerf-Ordnern.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use deck_core::paths;

pub struct HookEnv {
    /// `%LOCALAPPDATA%\claude-agent-deck\state`.
    pub state_dir: PathBuf,
    /// `%LOCALAPPDATA%\claude-agent-deck` (tickets.json für die Statuszeile).
    pub app_dir: PathBuf,
    /// `~/.claude/settings.json`: die Sprache der Statuszeile („kalt“/„cold“).
    pub claude_settings: PathBuf,
    pub agent_slot: Option<String>,
    /// Arbeitsverzeichnis des Hooks = das des Agenten.
    pub cwd: Option<String>,
    pub now: f64,
}

impl HookEnv {
    /// Die echte Umgebung dieses Prozesses.
    pub fn from_process() -> HookEnv {
        HookEnv {
            state_dir: paths::state_dir(),
            app_dir: paths::app_dir(),
            claude_settings: deck_core::claude::settings::settings_path(),
            agent_slot: std::env::var("AGENT_SLOT").ok().filter(|s| !s.is_empty()),
            cwd: std::env::current_dir().ok().map(|p| p.to_string_lossy().into_owned()).filter(|s| !s.is_empty()),
            now: unix_now(),
        }
    }

    /// Slot dieses Laufs, oder `None` (dann meldet der Hook nichts).
    pub fn resolve_slot(&self) -> Option<String> {
        self.agent_slot.clone()
    }
}

/// Jetzt, in Unix-Sekunden.
pub fn unix_now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}
