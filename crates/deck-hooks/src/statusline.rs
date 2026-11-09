//! `deck-statusline`: liefert Claude Code die Statuszeile UND schreibt die Live-Werte
//! (Modell, Effort, Kontext-% und -Tokens, Nachricht-Tokens, Kosten, Sessiontitel, Ablauf
//! des Prompt-Caches, PR, aktueller Ordner) für das Deck nach `state\<slot>.live.json`.
//! Kosten stehen nur in der Datei, nicht mehr in der Zeile.
//!
//! Claude Code ruft sie bei jeder Antwort und zusätzlich alle
//! `hook_setup::STATUSLINE_REFRESH_S` Sekunden auf: so läuft der Cache-Countdown auch im
//! Leerlauf weiter, und `live.json` bleibt frisch genug, dass das Deck vor dem Ablauf
//! kompaktieren kann (`deck_core::auto_compact`).
//!
//! Gibt IMMER eine Zeile aus (auch ohne Slot), als UTF-8-Bytes: über die
//! Konsolen-Codepage wurde aus „für“ „f◆r“. Felder mit falschem Typ gelten als fehlend
//! (`deck_core::field`). Prozent und Kosten behalten in `live.json` ihren JSON-Typ,
//! Token-Zahlen sind ganze Zahlen.

use deck_core::claude::hook_setup::STATUSLINE_REFRESH_S;
use deck_core::i18n::Lang;
use deck_core::paths::{live_path, load_json, save_json, state_path};
use deck_core::field::{number, raw_number, text};
use deck_core::prompt_cache::{CacheLevel, view, working};
use deck_core::text::{char_len, first_line, squash_ws, take_chars};
use serde_json::{Map, Value};

use crate::cache_scan::cache_until;
use crate::env::HookEnv;
use crate::status_extras::{pr_of, pr_part};

pub const SEP: &str = "  ·  ";
pub const TITLE_MAX: usize = 70;

/// Eine Token-Zahl: eine nicht negative JSON-Zahl, ganz gemacht.
fn tokens(v: Option<&Value>) -> Option<u64> {
    let v = v?;
    v.as_u64().or_else(|| v.as_f64().filter(|f| *f >= 0.0).map(|f| f.round() as u64))
}

/// Tokens der aktuellen Nachricht (Ein- plus Ausgabe, die Feldnamen wechselten über die
/// Versionen); ohne beide die Ausgabe der ganzen Session.
fn msg_tokens(cw: &Map<String, Value>) -> Option<u64> {
    let cur = cw.get("current_usage").and_then(Value::as_object);
    let first = |keys: [&str; 2]| cur.and_then(|c| keys.iter().find_map(|k| tokens(c.get(*k))));
    match (first(["input_tokens", "input"]), first(["output_tokens", "output"])) {
        (None, None) => tokens(cw.get("total_output_tokens")),
        (i, o) => Some(i.unwrap_or(0) + o.unwrap_or(0)),
    }
}

/// Wie groß der Kontext gerade ist, in Tokens: alles, was die letzte Anfrage
/// hineingegeben hat (frisch, neu gecacht, aus dem Cache gelesen); die Ausgabe zählt
/// nicht mit. Das Deck kompaktiert erst ab einer Mindestgröße; Prozent taugen dafür
/// nicht, weil das Fenster 200k oder 1M groß sein kann.
fn ctx_tokens(cw: &Map<String, Value>) -> Option<u64> {
    let cur = cw.get("current_usage").and_then(Value::as_object);
    let keys = ["input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens"];
    let vals: Vec<Option<u64>> = keys.iter().map(|k| tokens(cur.and_then(|c| c.get(*k)))).collect();
    if vals.iter().any(Option::is_some) {
        return Some(vals.into_iter().flatten().sum());
    }
    let pct = number(cw.get("used_percentage"))?;
    let size = number(cw.get("context_window_size")).filter(|s| *s > 0.0)?;
    let t = pct * size / 100.0;
    (t >= 0.0).then(|| t.round() as u64)
}

/// Text aus einem Feld, das ein Objekt (`{key: "…"}`) oder direkt ein Text sein kann.
fn text_or_in<'a>(v: Option<&'a Value>, key: &str) -> Option<&'a str> {
    match v? {
        Value::Object(m) => text(m.get(key)),
        other => text(Some(other)),
    }
}

fn or_null<T: Into<Value>>(v: Option<T>) -> Value {
    v.map_or(Value::Null, Into::into)
}

/// Die Live-Werte für `live.json`, in fester Feldreihenfolge.
pub fn extract(data: &Map<String, Value>, now: f64) -> Map<String, Value> {
    let model = match data.get("model") {
        Some(Value::Object(m)) => text(m.get("display_name")).or_else(|| text(m.get("id"))),
        other => text(other),
    };
    let empty = Map::new();
    let cw = data.get("context_window").and_then(Value::as_object).unwrap_or(&empty);
    let cost = data.get("cost").and_then(Value::as_object);
    let cache = cache_until(data);
    let mut rec = Map::new();
    rec.insert("model".into(), or_null(model));
    rec.insert("effort".into(), or_null(text_or_in(data.get("effort"), "level")));
    rec.insert("ctx_pct".into(), or_null(raw_number(cw.get("used_percentage")).cloned()));
    rec.insert("ctx_tokens".into(), or_null(ctx_tokens(cw)));
    rec.insert("msg_tokens".into(), or_null(msg_tokens(cw)));
    rec.insert("cost_usd".into(), or_null(raw_number(cost.and_then(|c| c.get("total_cost_usd"))).cloned()));
    rec.insert("session_name".into(), or_null(data.get("session_name").and_then(Value::as_str)));
    rec.insert("cache_until".into(), or_null(cache.map(|c| c.0)));
    rec.insert("cache_ttl".into(), or_null(cache.map(|c| c.1)));
    rec.insert("pr".into(), or_null(pr_of(data.get("pr"))));
    // Der Ordner, in dem der Agent JETZT arbeitet (folgt `cd` und worktrees, anders als
    // das cwd des Hook-Prozesses): daran liest das Deck Branch und worktree-Ordner ab.
    let cwd = data.get("workspace").and_then(|w| text(w.get("current_dir"))).or_else(|| text(data.get("cwd")));
    rec.insert("cwd".into(), or_null(cwd));
    rec.insert("ts".into(), now.into());
    rec
}

/// „B2 · ORPH-4116“: welche Kachel dieses Pane ist und woran es arbeitet. Ticket: im
/// Deck zugewiesen (tickets.json) oder vom Agenten gemeldet (`state\<slot>.ticket`).
pub fn label(slot: &str, env: &HookEnv) -> String {
    let tickets = load_json(&env.app_dir.join(deck_core::paths::TICKETS_FILE));
    let mut ticket = text(tickets.as_ref().and_then(|t| t.get(slot))).unwrap_or_default().to_string();
    if ticket.is_empty() {
        let path = deck_core::paths::found_ticket_path(&env.state_dir, slot);
        if let Ok(bytes) = std::fs::read(path) {
            ticket = take_chars(first_line(String::from_utf8_lossy(&bytes).trim()), 40).to_string();
        }
    }
    if ticket.is_empty() { slot.to_string() } else { format!("{slot} · {ticket}") }
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

/// Die Statuszeile: erst die Kopfzeile (Slot, Ticket, PR, Überschrift), darunter Modell,
/// Effort, Kontext und Cache-Restzeit; ohne Slot, PR und Überschrift nur diese Zeile.
pub fn line(rec: &Map<String, Value>, label: &str, lang: Lang) -> String {
    let head = header(label, rec.get("session_name").and_then(Value::as_str), rec.get("pr"), lang);
    let mut parts = Vec::new();
    if let Some(m) = text(rec.get("model")) {
        parts.push(m.to_string());
    }
    if let Some(e) = text(rec.get("effort")) {
        parts.push(format!("effort {e}"));
    }
    if let Some(p) = number(rec.get("ctx_pct")) {
        parts.push(format!("ctx {p:.0}%"));
    }
    // Ohne Ablaufzeit (auch 0.0 = schon kalt laut Claude Code) kein Countdown.
    let nonzero = |k: &str| number(rec.get(k)).filter(|v| *v != 0.0);
    let now = nonzero("ts").unwrap_or_else(crate::env::unix_now);
    if let (Some(until), Some(ttl)) = (nonzero("cache_until"), nonzero("cache_ttl")) {
        parts.push(cache_part(until, ttl, now, lang));
    }
    let body = parts.join(SEP);
    if head.is_empty() { body } else { format!("{head}\n{body}") }
}

/// Ein Lauf: Live-Datei schreiben (wenn ein Slot feststeht) und die Zeile liefern.
pub fn run(env: &HookEnv, stdin: Option<&[u8]>) -> String {
    let mut rec = extract(&crate::parse_stdin(stdin), env.now);
    // Deck-Zustand ist Beiwerk: ein Fehler hier macht nie die Statuszeile kaputt.
    let mut lbl = String::new();
    if let Some(slot) = env.resolve_slot()
        && save_json(&live_path(&env.state_dir, &slot), &Value::Object(rec.clone())).is_ok()
    {
        lbl = label(&slot, env);
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
    line(&rec, &lbl, lang)
}
