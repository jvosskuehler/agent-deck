//! Zustandsdateien der Slots lesen und schreiben.
//!
//! Jeder Agent meldet seinen Zustand über den report-Hook in eine kleine JSON-Datei
//! (`state\<slot>.json`), die App liest den Ordner im Poll-Takt. Dateibasiert, robust,
//! keine Sockets. Das Format ist ein VERTRAG zwischen zwei Prozessen und wird darum
//! bewusst locker gehalten (`serde_json::Map`): ein neues Feld im Hook darf die App nicht
//! brechen, und Felder, die die App nicht kennt, müssen beim Zurückschreiben bleiben.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde_json::{Map, Value};

use crate::paths::{load_json, save_json, state_path};
use crate::pystr::{py_str, splitlines, strip, take_chars};

/// Ein Slot-Zustand, wie der report-Hook ihn schreibt.
pub type SlotState = Map<String, Value>;

/// Zustand atomar setzen. Vorhandene Zusatzfelder (mode, effort, prompt, session_id …)
/// bleiben erhalten: „als gelesen markieren“ (done → idle beim Anklicken) verliert die
/// letzte Frage nicht, sondern dreht nur Status und Zeitstempel.
pub fn write_state(dir: &Path, slot: &str, status: &str, now: f64) -> std::io::Result<()> {
    let path = state_path(dir, slot);
    let mut rec = match load_json(&path) {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    };
    rec.insert("slot".into(), slot.into());
    rec.insert("status".into(), status.into());
    rec.insert("ts".into(), now.into());
    save_json(&path, &Value::Object(rec))
}

/// Zustandsdatei eines Slots entfernen (beim Schließen eines Agenten). Sonst erbt ein
/// später wiederverwendeter Slot-Name (die Extension vergibt `<Fenster><max+1>`, recycelt
/// also den Namen des geschlossenen höchsten Agenten) Status und Modus der alten Datei.
/// Fehlt sie schon: still ok.
pub fn clear_state(dir: &Path, slot: &str) {
    let _ = fs::remove_file(state_path(dir, slot));
}

/// Dateinamen im Ordner; fehlt er, gibt es noch keine Meldungen.
fn file_names(dir: &Path) -> Vec<String> {
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> =
        rd.flatten().filter_map(|e| e.file_name().into_string().ok()).collect();
    names.sort();
    names
}

/// `{slot: zustand}` aus allen `<slot>.json`; `.live.json` und `pidmap-*.json` zählen
/// nicht. Halb geschriebene oder kaputte Dateien werden übersprungen.
pub fn read_all(dir: &Path) -> HashMap<String, SlotState> {
    let mut out = HashMap::new();
    for name in file_names(dir) {
        if !name.ends_with(".json") || name.ends_with(".live.json") || name.starts_with("pidmap-") {
            continue;
        }
        if let Some(Value::Object(d)) = load_json(&dir.join(&name)) {
            let key = match d.get("slot") {
                Some(v) => py_str(v),
                None => name[..name.len() - 5].to_string(),
            };
            out.insert(key, d);
        }
    }
    out
}

/// Live-Werte der statusLine aus `<slot>.live.json`: `{slot: werte}`.
pub fn read_live(dir: &Path) -> HashMap<String, SlotState> {
    const SUFFIX: &str = ".live.json";
    let mut out = HashMap::new();
    for name in file_names(dir) {
        if let Some(slot) = name.strip_suffix(SUFFIX)
            && let Some(Value::Object(d)) = load_json(&dir.join(&name))
        {
            out.insert(slot.to_string(), d);
        }
    }
    out
}

/// Erste Zeile jeder `<slot><suffix>`-Datei (Klartext), auf `max` Zeichen gekürzt.
fn read_markers(dir: &Path, suffix: &str, max: usize) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for name in file_names(dir) {
        let Some(slot) = name.strip_suffix(suffix) else { continue };
        let Ok(bytes) = fs::read(dir.join(&name)) else { continue };
        // Python liest strikt UTF-8; eine Datei, die das nicht ist, zählt nicht.
        let Ok(text) = String::from_utf8(bytes) else { continue };
        let text = text.replace("\r\n", "\n");
        let val = strip(&text);
        if val.is_empty() {
            continue;
        }
        let first = splitlines(val).into_iter().next().unwrap_or("");
        out.insert(slot.to_string(), take_chars(strip(first), max).to_string());
    }
    out
}

/// Vom Agenten gemeldete Ticket-IDs aus `<slot>.ticket` (erste Zeile, höchstens 60
/// Zeichen; die Karte kürzt zusätzlich).
pub fn read_found_tickets(dir: &Path) -> HashMap<String, String> {
    read_markers(dir, ".ticket", 60)
}

/// Gemeldete worktree-Pfade aus `<slot>.worktree` (erste Zeile, höchstens 4096 Zeichen;
/// Pfade dürfen Leerzeichen enthalten).
pub fn read_found_worktrees(dir: &Path) -> HashMap<String, String> {
    read_markers(dir, ".worktree", 4096)
}
