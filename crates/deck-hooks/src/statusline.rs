//! `deck-statusline`: liefert Claude Code die Statuszeile UND schreibt die Live-Werte
//! (Modell, Effort, Kontext-% und -Tokens, Nachricht-Tokens, Kosten, Sessiontitel, Ablauf
//! des Prompt-Caches, Kontingent, PR) für das Deck nach `state\<slot>.live.json`.
//!
//! Claude Code ruft sie bei jeder Antwort und zusätzlich alle
//! `hook_setup::STATUSLINE_REFRESH_S` Sekunden auf: so läuft der Cache-Countdown auch im
//! Leerlauf weiter, und `live.json` bleibt frisch genug, dass das Deck vor dem Ablauf
//! kompaktieren kann (`deck_core::auto_compact`).
//!
//! Gibt IMMER eine Zeile aus (auch ohne Slot), Zeichen für Zeichen wie `statusline.py`,
//! als UTF-8-Bytes: über die Konsolen-Codepage wurde aus „für“ „f◆r“. Zahlen behalten
//! ihren JSON-Typ (eine ganze Zahl bleibt ganz), gerundet wird wie Pythons `round`.

use deck_core::claude::hook_setup::STATUSLINE_REFRESH_S;
use deck_core::i18n::Lang;
use deck_core::paths::{live_path, load_json, save_json, state_path};
use deck_core::prompt_cache::{CacheLevel, number, view, working};
use deck_core::pystr::{PyNum, char_len, py_str, splitlines, squash_ws, strip, take_chars, truthy, truthy_opt};
use serde_json::{Map, Value};

use crate::cache_scan::cache_until;
use crate::env::HookEnv;
pub use crate::status_extras::limits_part;
use crate::status_extras::{limits, pr_of, pr_part};

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

/// Wie groß der Kontext gerade ist, in Tokens: alles, was die letzte Anfrage
/// hineingegeben hat (frisch, neu gecacht, aus dem Cache gelesen); die Ausgabe zählt
/// nicht mit. Das Deck kompaktiert erst ab einer Mindestgröße; Prozent taugen dafür
/// nicht, weil das Fenster 200k oder 1M groß sein kann.
fn ctx_tokens(cw: &Map<String, Value>) -> Option<Value> {
    let cur = cw.get("current_usage").and_then(Value::as_object);
    let keys = ["input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens"];
    let vals: Vec<Option<PyNum>> = keys.iter().map(|k| cur.and_then(|c| c.get(*k)).and_then(PyNum::from_value)).collect();
    if vals.iter().any(Option::is_some) {
        let sum = vals.into_iter().fold(PyNum::Int(0), |acc, v| acc.py_add(v.unwrap_or(PyNum::Int(0))));
        return Some(sum.to_value());
    }
    let pct = num(&[cw.get("used_percentage")]).and_then(|v| PyNum::from_value(&v))?;
    let size = num(&[cw.get("context_window_size")]).and_then(|v| PyNum::from_value(&v)).filter(|s| s.as_f64() != 0.0)?;
    PyNum::Float(pct.as_f64() * size.as_f64() / 100.0).to_value().into()
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
    let msg = msg_tokens(cw)?;
    rec.insert("ctx_tokens".into(), ctx_tokens(cw).unwrap_or(Value::Null));
    rec.insert("msg_tokens".into(), msg.unwrap_or(Value::Null));
    rec.insert("cost_usd".into(), num(&[cost.get("total_cost_usd")]).unwrap_or(Value::Null));
    rec.insert("session_name".into(), data.get("session_name").filter(|v| v.is_string()).cloned().unwrap_or(Value::Null));
    let cache = cache_until(data);
    rec.insert("cache_until".into(), cache.map_or(Value::Null, |c| c.0.into()));
    rec.insert("cache_ttl".into(), cache.map_or(Value::Null, |c| c.1.into()));
    rec.insert("limits".into(), Value::Object(limits(data.get("rate_limits"))));
    rec.insert("pr".into(), pr_of(data.get("pr")).map_or(Value::Null, Value::Object));
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

/// Kopfzeile des Chats, fett: „B2 · ORPH-4116 · PR #347 freigegeben · Überschrift“.
/// Claude Code hat oben keinen Platz für eigene Zeilen, die Statuszeile steht dagegen in
/// JEDEM Pane. Der PR steht vor der Überschrift und kürzt sie: die Zeile soll nicht
/// länger werden als ohne PR, sonst bricht sie im schmalen Pane um.
pub fn header(label: &str, title: Option<&str>, pr: Option<&Value>, lang: Lang) -> String {
    let (pr_plain, pr_text) = pr_part(pr, lang);
    let budget = if pr_plain.is_empty() { TITLE_MAX } else { TITLE_MAX.saturating_sub(char_len(&pr_plain) + 3) };
    let mut t = squash_ws(title.unwrap_or(""));
    if char_len(&t) > budget {
        t = format!("{}…", take_chars(&t, budget.saturating_sub(1)));
    }
    let text = [label, pr_text.as_str(), t.as_str()].iter().filter(|p| !p.is_empty()).copied().collect::<Vec<_>>().join(" · ");
    if text.is_empty() { String::new() } else { format!("\x1b[1m{text}\x1b[0m") }
}

/// „cache 42 min“. Gezählt wird bis zum NÄCHSTEN Takt der Statuszeile: so zeigt die
/// Zeile nie mehr Restzeit an, als bis zu ihrer Erneuerung tatsächlich bleibt. Wortlaut
/// und Schwelle kommen aus `deck_core::prompt_cache`; normal, knapp gelb, kalt gedimmt.
pub fn cache_part(until: f64, ttl: f64, now: f64, lang: Lang) -> String {
    let (text, level) = view(until - now - STATUSLINE_REFRESH_S as f64, ttl, lang.l("kalt", "cold"));
    match level {
        CacheLevel::Warm => format!("cache {text}"),
        CacheLevel::Low => format!("[33mcache {text}[0m"),
        CacheLevel::Cold => format!("[2mcache {text}[0m"),
    }
}

/// Die Statuszeile: erst die Kopfzeile (Slot, Ticket, PR, Überschrift), darunter die
/// Zahlen; ohne Slot, PR und Überschrift nur die Zahlenzeile.
pub fn line(rec: &Map<String, Value>, label: &str, lang: Lang) -> String {
    let head = header(label, rec.get("session_name").and_then(Value::as_str), rec.get("pr"), lang);
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
    // Ohne Ablaufzeit (auch 0.0 = schon kalt laut Claude Code) kein Countdown.
    let nonzero = |k: &str| number(rec.get(k)).filter(|v| *v != 0.0);
    let now = nonzero("ts").unwrap_or_else(crate::env::unix_now);
    if let (Some(until), Some(ttl)) = (nonzero("cache_until"), nonzero("cache_ttl")) {
        parts.push(cache_part(until, ttl, now, lang));
    }
    let quota = limits_part(rec.get("limits"), now);
    if !quota.is_empty() {
        parts.push(quota);
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
    let mut rec = extract(parsed.as_object()?, env.now)?;
    // Deck-Zustand ist Beiwerk: ein Fehler hier macht nie die Statuszeile kaputt.
    let mut lbl = String::new();
    if let Some(slot) = env.resolve_slot()
        && save_json(&live_path(&env.state_dir, &slot), &Value::Object(rec.clone())).is_ok()
    {
        lbl = label(&slot, env).unwrap_or_default();
        // Arbeitet der Agent, kein Timer (`prompt_cache::working`). Nur die Anzeige:
        // live.json behält die Ablaufzeit, das Deck entscheidet selbst.
        let st = load_json(&state_path(&env.state_dir, &slot));
        if working(st.as_ref().and_then(Value::as_object), env.now) {
            rec.insert("cache_until".into(), Value::Null);
        }
    }
    // Die Sprache nur lesen, wenn ein Wort sie braucht (Cache „kalt“, PR-Zustand): die
    // Statuszeile läuft alle paar Sekunden in jedem Pane.
    let shows_cache = !rec.get("cache_until").is_none_or(Value::is_null);
    let shows_pr = !rec.get("pr").is_none_or(Value::is_null);
    let lang = if shows_cache || shows_pr { Lang::from_settings(&env.claude_settings) } else { Lang::German };
    Some(line(&rec, &lbl, lang))
}
