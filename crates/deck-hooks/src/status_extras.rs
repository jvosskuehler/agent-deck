//! Kontingent und PR in der Statuszeile (Python `statusline.py`, `_limits`,
//! `_limits_part`, `_pr_part`, Stand `2856494`).
//!
//! Beides kommt im stdin von Claude Code mit und kostet keinen Abruf:
//! * `rate_limits`: Claude Code liest es aus den Antwort-Headern der API mit (anders als
//!   der Usage-Endpoint des Decks, der eng limitiert ist). Schwellen wie im Usage-Badge
//!   des Decks: ab 50 % gelb, ab 80 % rot.
//! * `pr`: der PR des Branches, mit `review_state`, wie Claude Code ihn aus GitHub
//!   ableitet (Stand 2.1.319): `draft`, `approved`, `changes_requested`, sonst `pending`
//!   (offen, Review fehlt oder ist nicht verlangt).

use deck_core::i18n::Lang;
use deck_core::field::{number, raw_number};
use serde_json::{Map, Value};

/// Die Limits, die gezeigt werden, mit Kurzname; alles andere (`spend_limit` …) nicht.
pub const LIMITS: [(&str, &str); 2] = [("five_hour", "5h"), ("seven_day", "7d")];

const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";

/// `rate_limits` → `{"five_hour": {"pct": 18, "resets_at": 1794181200}, …}`, nur was
/// lesbar ist. Die Zahlen behalten ihren JSON-Typ (eine ganze Zahl bleibt ganz).
pub fn limits(raw: Option<&Value>) -> Map<String, Value> {
    let mut out = Map::new();
    for (key, _) in LIMITS {
        let Some(lim) = raw.and_then(|r| r.get(key)).and_then(Value::as_object) else { continue };
        let Some(pct) = raw_number(lim.get("used_percentage")) else { continue };
        let resets_at = raw_number(lim.get("resets_at")).cloned().unwrap_or(Value::Null);
        let mut entry = Map::new();
        entry.insert("pct".into(), pct.clone());
        entry.insert("resets_at".into(), resets_at);
        out.insert(key.into(), Value::Object(entry));
    }
    out
}

/// „5h 18% · 7d 3%“. Ein Wert hinter seinem Reset fällt weg: Claude Code kennt nur den
/// Stand der letzten Antwort, nach Stunden Leerlauf wäre der längst überholt (dieselbe
/// Regel wie `expire_limits` im Deck).
pub fn limits_part(limits: Option<&Value>, now: f64) -> String {
    let mut parts = Vec::new();
    for (key, short) in LIMITS {
        let Some(lim) = limits.and_then(|l| l.get(key)).and_then(Value::as_object) else { continue };
        let Some(p) = number(lim.get("pct")) else { continue };
        if number(lim.get("resets_at")).is_some_and(|r| r <= now) {
            continue;
        }
        let text = format!("{short} {p:.0}%");
        parts.push(match p {
            p if p < 50.0 => text,
            p if p < 80.0 => format!("{YELLOW}{text}\x1b[0m"),
            _ => format!("{RED}{text}\x1b[0m"),
        });
    }
    parts.join(" · ")
}

/// Der PR für `live.json`: `{number, url, review_state}`, nur wenn `number` eine ganze
/// Zahl ist (sonst ist es kein PR, den man zeigen könnte). Fehlende Felder → `null`.
pub fn pr_of(raw: Option<&Value>) -> Option<Map<String, Value>> {
    let pr = raw?.as_object()?;
    pr.get("number").filter(|n| n.is_i64() || n.is_u64())?;
    Some(["number", "url", "review_state"].iter().map(|k| (k.to_string(), pr.get(*k).cloned().unwrap_or(Value::Null))).collect())
}

/// Wortlaut und Farbe je `review_state`.
fn state_words(state: &str) -> Option<((&'static str, &'static str), &'static str)> {
    Some(match state {
        "approved" => (("freigegeben", "approved"), GREEN),
        "changes_requested" => (("Änderungen gewünscht", "changes requested"), YELLOW),
        "pending" => (("offen", "open"), ""),
        "draft" => (("Entwurf", "draft"), ""),
        _ => return None,
    })
}

/// („PR #347 freigegeben“, dieselbe Fassung mit Farbe); leer ohne PR. Gefärbt wird nur
/// mit 39 (Vordergrund zurücksetzen) statt 0: die Kopfzeile ist fett, ein volles
/// Zurücksetzen machte den Rest der Zeile dünn.
pub fn pr_part(pr: Option<&Value>, lang: Lang) -> (String, String) {
    let Some(number) = pr.and_then(|p| p.get("number")).filter(|n| n.is_i64() || n.is_u64()) else {
        return (String::new(), String::new());
    };
    let head = format!("PR #{number}");
    let state = pr.and_then(|p| p.get("review_state")).and_then(Value::as_str).and_then(state_words);
    match state {
        Some(((de, en), color)) => {
            let word = lang.l(de, en);
            let colored = if color.is_empty() { format!("{head} {word}") } else { format!("{head} {color}{word}\x1b[39m") };
            (format!("{head} {word}"), colored)
        }
        None => (head.clone(), head),
    }
}
