//! Ein Ort für alle Pfade: App-Datenordner, State-Ordner, atomares JSON.
//!
//! Die Python-Fassung legte ihre Laufzeit-JSONs (Bindungen, Reihenfolge, Tickets …)
//! neben den Code. Diese Fassung legt sie nach `%LOCALAPPDATA%\claude-agent-deck\`,
//! denn eine installierte App hat kein beschreibbares Repo neben sich. Die
//! Zustandsdateien der Hooks liegen in `…\claude-agent-deck\state\`.
//!
//! Jede Funktion, die eine Datei anfasst, bekommt ihren Ordner als Parameter. Die
//! Vorgaben (`state_dir()`, `app_dir()`) ruft nur der Aufrufer ganz oben; Tests reichen
//! einen Wegwerf-Ordner herein und erreichen so nie die echten Dateien des Nutzers.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde_json::Value;
/// Name des Ordners unter `%LOCALAPPDATA%`, gemeinsam mit der Python-Fassung.
pub const APP_DIR_NAME: &str = "claude-agent-deck";

// Laufzeitdateien der App (in `app_dir()`), Namen wie in der Python-Fassung.
pub const BINDINGS_FILE: &str = "bindings.json";
pub const EFFORT_FILE: &str = "slot_effort.json";
pub const SETTINGS_FILE: &str = "deck_settings.json";
pub const TICKETS_FILE: &str = "tickets.json";
pub const ORDER_FILE: &str = "slot_order.json";
pub const WIN_ORDER_FILE: &str = "win_order.json";
pub const WT_HOSTS_FILE: &str = "wt_hosts.json";

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

/// Heimatordner wie Pythons `os.path.expanduser("~")`: unter Windows `USERPROFILE`,
/// sonst `HOMEDRIVE` + `HOMEPATH`; anderswo `HOME`.
pub fn home_dir() -> PathBuf {
    if cfg!(windows) {
        if let Some(p) = env_nonempty("USERPROFILE") {
            return PathBuf::from(p);
        }
        if let (Some(d), Some(p)) = (env_nonempty("HOMEDRIVE"), env_nonempty("HOMEPATH")) {
            return PathBuf::from(format!("{d}{p}"));
        }
    }
    PathBuf::from(env_nonempty("HOME").unwrap_or_else(|| ".".into()))
}

/// `%LOCALAPPDATA%`, ersatzweise der Heimatordner (wie `paths.STATE_DIR` in Python).
pub fn local_app_data() -> PathBuf {
    env_nonempty("LOCALAPPDATA").map_or_else(home_dir, PathBuf::from)
}

/// `%LOCALAPPDATA%\claude-agent-deck`: Laufzeitdateien der App.
pub fn app_dir() -> PathBuf {
    local_app_data().join(APP_DIR_NAME)
}

/// `%LOCALAPPDATA%\claude-agent-deck\state`: hier schreiben die Hooks.
pub fn state_dir() -> PathBuf {
    app_dir().join("state")
}

/// `state\<slot>.json`: Zustand eines Slots (report-Hook).
pub fn state_path(dir: &Path, slot: &str) -> PathBuf {
    dir.join(format!("{slot}.json"))
}

/// `state\<slot>.live.json`: Live-Werte der statusLine.
pub fn live_path(dir: &Path, slot: &str) -> PathBuf {
    dir.join(format!("{slot}.live.json"))
}

/// `state\<slot>.ticket`: vom Agenten selbst gefundene Ticket-ID (Klartext, eine Zeile),
/// geschrieben bei der Zuweisung „Im Chat suchen“, bei der das Deck die ID vorher nicht
/// kennt.
pub fn found_ticket_path(dir: &Path, slot: &str) -> PathBuf {
    dir.join(format!("{slot}.ticket"))
}

/// `state\<slot>.worktree`: absoluter Pfad des git worktree, den der Agent für sein
/// Ticket angelegt hat. Beim Schließen des Agenten räumt das Deck genau diesen auf.
pub fn worktree_marker_path(dir: &Path, slot: &str) -> PathBuf {
    dir.join(format!("{slot}.worktree"))
}

/// JSON lesen; fehlt, ist halb geschrieben oder kaputt: `None` (nie ein Fehler).
pub fn load_json(path: &Path) -> Option<Value> {
    let bytes = fs::read(path).ok()?;
    let text = std::str::from_utf8(&bytes).ok()?;
    serde_json::from_str(text).ok()
}

/// Atomar schreiben: erst `<path>.tmp`, dann ersetzen, nie halbe Dateien. Legt den
/// Zielordner bei Bedarf an.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    atomic_write_with(path, bytes, ".tmp")
}

/// Wie [`atomic_write`], aber über die Zwischendatei `<path><suffix>`. Ein eigenes
/// Anhängsel braucht, wer dieselbe Datei aus mehreren Prozessen schreibt (gemeinsamer
/// Usage-Cache) oder fremde Dateien anfasst, deren `.tmp` einem anderen gehören könnte.
/// Scheitert das Schreiben, bleibt keine Zwischendatei liegen.
pub fn atomic_write_with(path: &Path, bytes: &[u8], suffix: &str) -> io::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(suffix);
    let tmp = PathBuf::from(tmp);
    let written = fs::File::create(&tmp).and_then(|mut f| f.write_all(bytes)).and_then(|()| fs::rename(&tmp, path));
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written
}

/// JSON atomar schreiben, kompakt und als UTF-8.
pub fn save_json(path: &Path, data: &Value) -> io::Result<()> {
    atomic_write(path, &serde_json::to_vec(data)?)
}
