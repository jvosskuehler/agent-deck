//! `deck-report <status>`: von den Claude-Code-Hooks aufgerufen, schreibt den Zustand
//! des Agenten nach `state\<slot>.json`.
//!
//! Felder, die nur manche Events liefern (mode, effort, session_id, prompt), werden aus
//! der vorigen Datei fortgetragen; das cwd des Hooks
//! ist das des Agenten (ein `cd` im Bash-Tool ändert es nicht). Ein Notification-Event,
//! das keine echte Rückfrage ist, und ein SessionStart, der kein echter Start ist,
//! lassen die Datei unberührt.

use std::path::PathBuf;

use deck_core::paths::{load_json, save_json, state_path};
use deck_core::field::text;
use deck_core::text::{char_len, first_line, take_chars};
use serde_json::{Map, Value};

use crate::env::HookEnv;

/// Claude-Code-`permission_mode` → Deck-Name (wie `config::MODE_CYCLE`).
fn map_mode(pm: &str) -> &str {
    match pm {
        "default" => "manual",
        "acceptedits" => "accept",
        "bypasspermissions" => "bypass",
        other => other,
    }
}

/// Notification-Typen, die WIRKLICH eine Entscheidung verlangen. Der Hook feuert auch
/// für idle_prompt, auth_success, elicitation_complete … – die sind keine Rückfrage.
const WAIT_NOTIFY: [&str; 3] = ["permission_prompt", "elicitation_dialog", "agent_needs_input"];

type Data = Map<String, Value>;

/// Deck-Name des gemeldeten Permission-Modus.
pub fn mode_of(data: &Data) -> Option<String> {
    let low = text(data.get("permission_mode"))?.to_lowercase();
    Some(map_mode(&low).to_string())
}

/// Effort aus dem Event (`effort.level` oder ein Text).
pub fn effort_of(data: &Data) -> Option<&str> {
    match data.get("effort")? {
        Value::Object(e) => text(e.get("level")),
        other => text(Some(other)),
    }
}

/// Die zuletzt abgeschickte Frage (nur bei UserPromptSubmit): getrimmt, auf 500 Zeichen
/// gekürzt, damit Zustandsdatei und Tooltip klein bleiben.
pub fn prompt_of(data: &Data) -> Option<String> {
    let p = data.get("prompt")?.as_str()?.trim();
    if p.is_empty() {
        return None;
    }
    if char_len(p) > 500 {
        return Some(format!("{}…", take_chars(p, 500).trim_end()));
    }
    Some(p.to_string())
}

/// Kurzbeschreibung des gerade genutzten Tools (nur Pre/PostToolUse).
pub fn activity_of(data: &Data) -> Option<String> {
    let tn = text(data.get("tool_name"))?;
    let ti = data.get("tool_input").and_then(Value::as_object);
    let detail = ["command", "file_path", "path", "pattern", "url", "description"]
        .iter()
        .find_map(|k| text(ti.and_then(|t| t.get(*k))))
        .map_or("", first_line);
    let detail = if char_len(detail) > 42 { format!("{}…", take_chars(detail, 42)) } else { detail.to_string() };
    Some(if detail.is_empty() { tn.to_string() } else { format!("{tn}: {detail}") })
}

/// Ist ein Notification-Event eine echte Rückfrage? Bevorzugt `notification_type`;
/// ältere Versionen ohne das Feld erkennt der Text der Permission-Meldung.
pub fn is_real_query(data: &Data) -> bool {
    if let Some(nt) = data.get("notification_type").and_then(Value::as_str).filter(|s| !s.is_empty()) {
        return WAIT_NOTIFY.contains(&nt);
    }
    let low = text(data.get("message")).unwrap_or_default().to_lowercase();
    low.contains("allow") || low.contains("permission")
}

/// Ein Lauf. `stdin` wird erst gelesen, wenn ein Slot feststeht. Rückgabe: die
/// geschriebene Datei, oder `None`, wenn nichts zu melden war.
pub fn run(status: &str, env: &HookEnv, stdin: impl FnOnce() -> Option<Vec<u8>>) -> Option<PathBuf> {
    let slot = env.resolve_slot()?;
    let data = &crate::parse_stdin(stdin().as_deref());

    // Notification meldet stur „waiting“, feuert aber auch für Leerlauf: dann den
    // Zustand nicht anfassen, statt eine fertige Kachel auf „Rückfrage“ zu kippen.
    if status == "waiting" && !is_real_query(data) {
        return None;
    }
    // SessionStart feuert auch bei resume/clear/compact; nur der echte Start setzt die
    // Kachel zurück (fehlt `source`: ältere Version, als Start behandeln).
    if data.get("hook_event_name").and_then(Value::as_str) == Some("SessionStart") {
        match data.get("source") {
            None | Some(Value::Null) => {}
            Some(Value::String(s)) if s.is_empty() || s == "startup" => {}
            _ => return None,
        }
    }

    let dst = state_path(&env.state_dir, &slot);
    let prev = load_json(&dst).unwrap_or_default();
    let carried = |key: &str| text(prev.get(key)).map(Value::from);

    let mut rec = Map::new();
    rec.insert("slot".into(), slot.clone().into());
    rec.insert("status".into(), status.into());
    rec.insert("ts".into(), env.now.into());
    if let Some(mode) = mode_of(data).map(Value::from).or_else(|| carried("mode")) {
        rec.insert("mode".into(), mode);
    }
    if let Some(eff) = effort_of(data).map(Value::from).or_else(|| carried("effort")) {
        rec.insert("effort".into(), eff);
    }
    let stop = data.get("hook_event_name").and_then(Value::as_str) == Some("Stop") || status == "done";
    let act = if stop {
        Value::from("")
    } else {
        match activity_of(data) {
            Some(a) => a.into(),
            None => text(prev.get("activity")).unwrap_or_default().into(),
        }
    };
    rec.insert("activity".into(), act);
    if let Some(sid) = text(data.get("session_id")).map(Value::from).or_else(|| carried("session_id")) {
        rec.insert("session_id".into(), sid);
    }
    if let Some(cwd) = env.cwd.clone().map(Value::from).or_else(|| carried("cwd")) {
        rec.insert("cwd".into(), cwd);
    }
    if let Some(prompt) = prompt_of(data).map(Value::from).or_else(|| carried("prompt")) {
        rec.insert("prompt".into(), prompt);
    }
    save_json(&dst, &Value::Object(rec)).ok()?;
    Some(dst)
}
