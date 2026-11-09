//! Ein Ort für alle Pfade: App-Datenordner, State-Ordner, atomares JSON.
//!
//! Die Python-Fassung legte ihre Laufzeit-JSONs (Bindungen, Reihenfolge, Tickets …)
//! neben den Code. Diese Fassung legt sie nach `%LOCALAPPDATA%\claude-agent-deck\`,
//! denn eine installierte App hat kein beschreibbares Repo neben sich. Die
//! Zustandsdateien der Hooks liegen wie bisher in `…\claude-agent-deck\state\`, damit
//! Python- und Rust-Fassung während der Umstellung dieselben Dateien lesen.
//!
//! Jede Funktion, die eine Datei anfasst, bekommt ihren Ordner als Parameter. Die
//! Vorgaben (`state_dir()`, `app_dir()`) ruft nur der Aufrufer ganz oben; Tests reichen
//! einen Wegwerf-Ordner herein und erreichen so nie die echten Dateien des Nutzers.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::pyjson;

/// Name des Ordners unter `%LOCALAPPDATA%`, gemeinsam mit der Python-Fassung.
pub const APP_DIR_NAME: &str = "claude-agent-deck";

// Laufzeitdateien der App (in `app_dir()`), Namen wie in der Python-Fassung.
pub const BINDINGS_FILE: &str = "bindings.json";
pub const EFFORT_FILE: &str = "slot_effort.json";
pub const SETTINGS_FILE: &str = "deck_settings.json";
pub const TICKETS_FILE: &str = "tickets.json";
pub const ORDER_FILE: &str = "slot_order.json";
pub const WIN_ORDER_FILE: &str = "win_order.json";
pub const PLAN_FILE: &str = "restart_plan.json";
pub const WT_HOSTS_FILE: &str = "wt_hosts.json";

/// Was beim ersten Start aus dem Python-Repo übernommen wird.
pub const RUNTIME_FILES: [&str; 8] = [
    BINDINGS_FILE,
    EFFORT_FILE,
    SETTINGS_FILE,
    TICKETS_FILE,
    ORDER_FILE,
    WIN_ORDER_FILE,
    PLAN_FILE,
    WT_HOSTS_FILE,
];

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

/// `state\<slot>.wtpid`: Shell eines Windows-Terminal-Agenten samt Startzeit.
pub fn wtpid_path(dir: &Path, slot: &str) -> PathBuf {
    dir.join(format!("{slot}.wtpid"))
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
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
    }
    fs::rename(&tmp, path)
}

/// JSON atomar schreiben, Byte für Byte wie Pythons `json.dump(data, f)`.
pub fn save_json(path: &Path, data: &Value) -> io::Result<()> {
    atomic_write(path, pyjson::dumps(data).as_bytes())
}

/// Die Laufzeitdateien aus dem alten Python-Repo übernehmen, die es im App-Ordner noch
/// nicht gibt. Vorhandene werden nie überschrieben: wer die neue Fassung schon benutzt
/// hat, verliert nichts. Rückgabe: die Namen der übernommenen Dateien.
pub fn adopt_legacy_files(legacy_root: &Path, app_dir: &Path) -> io::Result<Vec<&'static str>> {
    let mut adopted = Vec::new();
    for name in RUNTIME_FILES {
        let src = legacy_root.join(name);
        let dst = app_dir.join(name);
        if src.is_file() && !dst.exists() {
            let bytes = fs::read(&src)?;
            atomic_write(&dst, &bytes)?;
            adopted.push(name);
        }
    }
    Ok(adopted)
}
