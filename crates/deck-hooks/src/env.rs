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
use deck_core::wsl::{self, Wsl};

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
    /// Läuft der Agent in WSL (`deck-wsl.exe` setzt `AGENT_DECK_DISTRO`), kommen Pfade im
    /// Hook-Eingang als Linux-Pfade; sie werden vor allem anderen umgerechnet.
    pub wsl: Option<Wsl>,
}

impl HookEnv {
    /// Die echte Umgebung dieses Prozesses.
    pub fn from_process() -> HookEnv {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        // Aus WSL heraus gestartet sieht ein Windows-Programm nur die Umgebung, die WSL ihm
        // mitgibt: den Zustandsordner und Claudes Ordner in der Distro (von WSL schon in
        // einen Windows-Pfad übersetzt) reicht darum `deck-wsl.exe` bzw. das Startskript.
        // Als Linux-Programm in der Distro (`deck-report` für WSL-Agenten) liegt derselbe
        // Ordner unter `/mnt/c/…`, Claudes Ordner kommt als Linux-Pfad (`/p` wirkt nur auf dem
        // Weg zu Windows-Programmen).
        let wsl = var(wsl::ENV_DISTRO).map(|d| Wsl::new(&d, "", &var(wsl::ENV_MOUNT).unwrap_or_default()));
        let state_dir = var(wsl::ENV_STATE)
            .map(|s| PathBuf::from(state_dir_for(&s, wsl.as_ref(), NATIVE_LINUX)))
            .unwrap_or_else(paths::state_dir);
        let app_dir = state_dir.parent().map_or_else(paths::app_dir, std::path::Path::to_path_buf);
        let claude_settings = var(wsl::ENV_CLAUDE_DIR)
            .map_or_else(deck_core::claude::settings::settings_path, |d| PathBuf::from(d).join("settings.json"));
        // Das cwd des Prozesses: unter Windows schon übersetzt (`\\wsl.localhost\…`), als
        // Linux-Programm ein Linux-Pfad, der in die Zustandsdatei als Windows-Pfad kommt.
        let cwd = std::env::current_dir().ok().map(|p| p.to_string_lossy().into_owned()).filter(|s| !s.is_empty());
        let cwd = cwd.map(|c| wsl.as_ref().map_or_else(|| c.clone(), |w| w.to_windows(&c)));
        HookEnv {
            state_dir,
            app_dir,
            claude_settings,
            wsl,
            agent_slot: std::env::var("AGENT_SLOT").ok().filter(|s| !s.is_empty()),
            cwd,
            now: unix_now(),
        }
    }

    /// Das Hook-JSON (wie [`crate::parse_stdin`]), in WSL mit Windows-Pfaden.
    pub fn input(&self, raw: Option<&[u8]>) -> serde_json::Map<String, serde_json::Value> {
        let mut data = crate::parse_stdin(raw);
        if let Some(w) = &self.wsl {
            localize(&mut data, w, NATIVE_LINUX);
        }
        data
    }

    /// Slot dieses Laufs, oder `None` (dann meldet der Hook nichts).
    pub fn resolve_slot(&self) -> Option<String> {
        self.agent_slot.clone()
    }
}

/// Läuft dieser Hook als Linux-Programm (in der WSL-Distro)?
pub const NATIVE_LINUX: bool = cfg!(not(windows));

/// Der Zustandsordner aus `AGENT_DECK_STATE` (Windows-Pfad): als Linux-Programm unter
/// dem Einhängepunkt der Distro (`/mnt/c/…`), sonst wie er ist.
pub fn state_dir_for(state: &str, wsl: Option<&Wsl>, native_linux: bool) -> String {
    match wsl.filter(|_| native_linux).and_then(|w| w.to_linux(state)) {
        Some(linux) => linux,
        None => state.to_string(),
    }
}

/// Pfade im Hook-Eingang in Windows-Pfade umrechnen (so stehen sie in den Zustandsdateien).
/// Das Transcript liest der Hook selbst (`cache_scan`): als Linux-Programm bleibt es darum
/// beim Linux-Pfad; geschrieben wird es nirgends.
pub fn localize(data: &mut serde_json::Map<String, serde_json::Value>, w: &Wsl, native_linux: bool) {
    let keep: Vec<(String, serde_json::Value)> = if native_linux {
        ["transcript_path", "agent_transcript_path"].iter().filter_map(|k| data.get(*k).map(|v| (k.to_string(), v.clone()))).collect()
    } else {
        Vec::new()
    };
    w.localize_hook_input(data);
    data.extend(keep);
}

/// Jetzt, in Unix-Sekunden.
pub fn unix_now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}
