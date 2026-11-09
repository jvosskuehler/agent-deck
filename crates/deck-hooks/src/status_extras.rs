//! Der PR in der Statuszeile (Python `statusline.py`, `_pr_part`, Stand `2856494`).
//!
//! `pr` kommt im stdin von Claude Code mit und kostet keinen Abruf: der PR des Branches,
//! mit `review_state`, wie Claude Code ihn aus GitHub ableitet (Stand 2.1.319): `draft`,
//! `approved`, `changes_requested`, sonst `pending` (offen, Review fehlt oder ist nicht
//! verlangt). Das Deck liest ihn aus `live.json` als maßgeblichen PR der Kachel.
//!
//! Kontingent (`5h 18% · 7d 3%`) und Kosten standen hier früher auch; sie sind
//! herausgeflogen (Nov. 2026): das Deck zeigt die Nutzung selbst, und in jedem Pane war
//! es nur Rauschen.

use deck_core::i18n::Lang;
use serde_json::{Map, Value};

const YELLOW: &str = "\x1b[33m";
const GREEN: &str = "\x1b[32m";

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
