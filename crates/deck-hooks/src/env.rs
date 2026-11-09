//! Umgebung eines Hook-Laufs und die Slot-Auflösung.
//!
//! Der Slot (A1 … O9) kommt auf zwei Wegen:
//! 1. `AGENT_SLOT` aus der Umgebung: von der Extension bzw. dem Windows-Terminal-
//!    Startskript gesetzt.
//! 2. Über die Prozesskette: der Hook läuft als Nachfahre des Claude-Prozesses. Der
//!    erste Vorfahre, der in einer `pidmap-<Fenster>.json` der Extension steht, nennt
//!    den Slot. So melden auch selbst gestartete Sessions.
//!
//! Alles, was nach außen greift (Ordner, Uhr, Prozesse), steckt in `HookEnv`; Tests
//! bauen sich eine eigene mit Wegwerf-Ordnern.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use deck_core::paths;
use deck_core::pystr::strip;
use serde_json::Value;

pub struct HookEnv {
    /// `%LOCALAPPDATA%\claude-agent-deck\state`.
    pub state_dir: PathBuf,
    /// `%LOCALAPPDATA%\claude-agent-deck` (tickets.json für die Statuszeile).
    pub app_dir: PathBuf,
    pub agent_slot: Option<String>,
    /// Arbeitsverzeichnis des Hooks = das des Agenten.
    pub cwd: Option<String>,
    pub now: f64,
    /// Eigene Prozesskette; wird nur gefragt, wenn eine pidmap existiert.
    pub ancestors: Box<dyn Fn() -> Vec<u32>>,
}

impl HookEnv {
    /// Die echte Umgebung dieses Prozesses.
    pub fn from_process() -> HookEnv {
        HookEnv {
            state_dir: paths::state_dir(),
            app_dir: paths::app_dir(),
            agent_slot: std::env::var("AGENT_SLOT").ok().filter(|s| !s.is_empty()),
            cwd: std::env::current_dir().ok().map(|p| p.to_string_lossy().into_owned()).filter(|s| !s.is_empty()),
            now: SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0),
            ancestors: Box::new(crate::procs::ancestor_pids),
        }
    }

    /// Slot dieses Laufs, oder `None` (dann meldet der Hook nichts).
    pub fn resolve_slot(&self) -> Option<String> {
        if let Some(s) = &self.agent_slot {
            return Some(s.clone());
        }
        slot_from_procs(&self.state_dir, &*self.ancestors)
    }
}

/// `int(k)` für einen pidmap-Schlüssel.
fn py_int(k: &str) -> Option<i64> {
    strip(k).replace('_', "").parse().ok()
}

/// Alle `pidmap-*.json` zu `{pid: slot}` vereinen (PIDs sind global eindeutig).
/// Kaputte Dateien zählen nicht; ein unlesbarer Schlüssel beendet seine Datei, die
/// Einträge davor bleiben (wie in Python, das dort mitten in der Schleife abbricht).
pub fn load_pidmap(dir: &Path) -> HashMap<i64, Value> {
    let mut out = HashMap::new();
    let Ok(rd) = fs::read_dir(dir) else { return out };
    let mut names: Vec<String> = rd.flatten().filter_map(|e| e.file_name().into_string().ok()).collect();
    names.sort();
    for name in names.iter().filter(|n| n.starts_with("pidmap-") && n.ends_with(".json")) {
        let Some(Value::Object(m)) = paths::load_json(&dir.join(name)) else { continue };
        for (k, v) in m {
            let Some(pid) = py_int(&k) else { break };
            out.insert(pid, v);
        }
    }
    out
}

/// Ersten eigenen Vorfahren finden, der in der pidmap steht: dessen Slot. Ist der dort
/// eingetragene Wert kein Text (oder leer), gibt es keinen Slot.
pub fn slot_from_procs(dir: &Path, ancestors: &dyn Fn() -> Vec<u32>) -> Option<String> {
    let pm = load_pidmap(dir);
    if pm.is_empty() {
        return None;
    }
    let hit = ancestors().into_iter().find_map(|pid| pm.get(&i64::from(pid)))?;
    hit.as_str().filter(|s| !s.is_empty()).map(String::from)
}
