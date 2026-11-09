//! `deck-statusline`: liefert Claude Code die Statuszeile UND schreibt die Live-Werte
//! (Modell, Effort, Kontext-%, Nachricht-Tokens, Kosten, Sessiontitel) für das Deck nach
//! `state\<slot>.live.json`.
//!
//! Gibt IMMER eine Zeile aus (auch ohne Slot), Zeichen für Zeichen wie `statusline.py`,
//! als UTF-8-Bytes: über die Konsolen-Codepage wurde aus „für“ „f◆r“. Zahlen behalten
//! ihren JSON-Typ (eine ganze Zahl bleibt ganz), gerundet wird wie Pythons `round`.

use deck_core::paths::{live_path, load_json, save_json};
use deck_core::pystr::{PyNum, char_len, py_str, splitlines, squash_ws, strip, take_chars, truthy, truthy_opt};
use serde_json::{Map, Value};

use crate::env::HookEnv;

pub const SEP: &str = "  ·  ";
pub const TITLE_MAX: usize = 70;

/// `_num(*vals)`: der erste Wert, der eine Zahl ist (ein Wahrheitswert zählt mit).
fn num(vals: &[Option<&Value>]) -> Option<Value> {
    vals.iter().flatten().find(|v| PyNum::from_value(v).is_some()).map(|v| (*v).clone())
}

/// Tokens der aktuellen Nachricht (best effort über wechselnde Feldnamen). `None` im
/// äußeren Option heißt: Python wäre hier gescheitert (`current_usage` kein Objekt).
fn msg_tokens(cw: &Map<String, Value>) -> Option<Option<Value>> {
    let empty = Map::new();
    let cur = match truthy_opt(cw.get("current_usage")) {
        None => &empty,
        Some(Value::Object(m)) => m,
        Some(_) => return None,
    };
    let i = num(&[cur.get("input_tokens"), cur.get("input")]);
    let o = num(&[cur.get("output_tokens"), cur.get("output")]);
    if i.is_none() && o.is_none() {
        return Some(num(&[cw.get("total_output_tokens")]));
    }
    // `(i or 0) + (o or 0)`: ein falscher Wert wird zur ganzen 0.
    let part = |v: Option<Value>| v.filter(truthy).and_then(|v| PyNum::from_value(&v)).unwrap_or(PyNum::Int(0));
    Some(Some(part(i).py_add(part(o)).to_value()))
}

/// Die Live-Werte in der Feldreihenfolge der Python-Fassung. `None`: Python wäre an
/// einem kaputten Feld gescheitert und hätte gar nichts ausgegeben.
pub fn extract(data: &Map<String, Value>, now: f64) -> Option<Map<String, Value>> {
    let model = match data.get("model") {
        Some(Value::Object(m)) => truthy_opt(m.get("display_name")).or(m.get("id")).cloned(),
        Some(v @ Value::String(_)) => Some(v.clone()),
        _ => None,
    };
    let effort = match data.get("effort") {
        Some(Value::Object(e)) => e.get("level").cloned(),
        Some(v @ Value::String(_)) => Some(v.clone()),
        _ => None,
    };
    let empty = Map::new();
    let cw = data.get("context_window").and_then(Value::as_object).unwrap_or(&empty);
    let cost = data.get("cost").and_then(Value::as_object).unwrap_or(&empty);
    let mut rec = Map::new();
    rec.insert("model".into(), model.unwrap_or(Value::Null));
    rec.insert("effort".into(), effort.unwrap_or(Value::Null));
    rec.insert("ctx_pct".into(), num(&[cw.get("used_percentage")]).unwrap_or(Value::Null));
    rec.insert("msg_tokens".into(), msg_tokens(cw)?.unwrap_or(Value::Null));
    rec.insert("cost_usd".into(), num(&[cost.get("total_cost_usd")]).unwrap_or(Value::Null));
    rec.insert("session_name".into(), data.get("session_name").filter(|v| v.is_string()).cloned().unwrap_or(Value::Null));
    rec.insert("ts".into(), now.into());
    Some(rec)
}

/// „B2 · ORPH-4116“: welche Kachel dieses Pane ist und woran es arbeitet. Ticket: im
/// Deck zugewiesen (tickets.json) oder vom Agenten gemeldet (`state\<slot>.ticket`).
/// `None`: Python wäre gescheitert (dann bleibt das Etikett leer).
pub fn label(slot: &str, env: &HookEnv) -> Option<String> {
    let tickets = match load_json(&env.app_dir.join(deck_core::paths::TICKETS_FILE)) {
        Some(Value::Object(m)) => m,
        Some(v) if truthy(&v) => return None,
        _ => Map::new(),
    };
    let mut ticket = truthy_opt(tickets.get(slot)).map(py_str).unwrap_or_default();
    if ticket.is_empty() {
        let path = deck_core::paths::found_ticket_path(&env.state_dir, slot);
        if let Ok(bytes) = std::fs::read(path) {
            // Python liest strikt UTF-8; ein Dekodierfehler bricht das Etikett ab.
            let text = String::from_utf8(bytes).ok()?.replace("\r\n", "\n");
            let first = splitlines(strip(&text)).first().copied().unwrap_or("");
            ticket = take_chars(first, 40).to_string();
        }
    }
    Some(if ticket.is_empty() { slot.to_string() } else { format!("{slot} · {ticket}") })
}

/// Kopfzeile des Chats, fett: „B2 · ORPH-4116 · Überschrift“. Claude Code hat oben
/// keinen Platz für eigene Zeilen, die Statuszeile steht dagegen in JEDEM Pane.
pub fn header(label: &str, title: Option<&str>) -> String {
    let mut t = squash_ws(title.unwrap_or(""));
    if char_len(&t) > TITLE_MAX {
        t = format!("{}…", take_chars(&t, TITLE_MAX - 1));
    }
    let text = [label, t.as_str()].iter().filter(|p| !p.is_empty()).copied().collect::<Vec<_>>().join(" · ");
    if text.is_empty() { String::new() } else { format!("\x1b[1m{text}\x1b[0m") }
}

/// Die Statuszeile: erst die Kopfzeile, darunter die Zahlen; ohne Slot und ohne
/// Überschrift nur die Zahlenzeile.
pub fn line(rec: &Map<String, Value>, label: &str) -> String {
    let head = header(label, rec.get("session_name").and_then(Value::as_str));
    let mut parts = Vec::new();
    if let Some(m) = truthy_opt(rec.get("model")) {
        parts.push(py_str(m));
    }
    if let Some(e) = truthy_opt(rec.get("effort")) {
        parts.push(format!("effort {}", py_str(e)));
    }
    if let Some(p) = rec.get("ctx_pct").and_then(PyNum::from_value) {
        parts.push(format!("ctx {}%", p.round_str()));
    }
    if let Some(c) = rec.get("cost_usd").and_then(PyNum::from_value) {
        parts.push(format!("${:.2}", c.as_f64()));
    }
    let body = parts.join(SEP);
    if head.is_empty() { body } else { format!("{head}\n{body}") }
}

/// Ein Lauf: Live-Datei schreiben (wenn ein Slot feststeht) und die Zeile liefern.
/// `None` = keine Ausgabe (Python wäre gescheitert).
pub fn run(env: &HookEnv, stdin: Option<&[u8]>) -> Option<String> {
    let parsed = crate::parse_stdin(stdin);
    let rec = extract(parsed.as_object()?, env.now)?;
    // Deck-Zustand ist Beiwerk: ein Fehler hier macht nie die Statuszeile kaputt.
    let mut lbl = String::new();
    if let Some(slot) = env.resolve_slot()
        && save_json(&live_path(&env.state_dir, &slot), &Value::Object(rec.clone())).is_ok()
    {
        lbl = label(&slot, env).unwrap_or_default();
    }
    Some(line(&rec, &lbl))
}
