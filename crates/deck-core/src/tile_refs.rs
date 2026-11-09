//! `state\<slot>.refs.json`: Ticket und PR einer Kachel, so wie das Deck sie zeigt.
//!
//! Eine Quelle der Wahrheit: welches Ticket ein Agent hat, entscheidet nur das Deck
//! (`ref_sources`, Quellen mit fester Reihenfolge). Damit die Statuszeile im Terminal
//! (`deck-statusline`) dasselbe zeigt wie die Kachel, schreibt das Deck seine Wahl je Slot in
//! diese Datei (atomar, nur bei Änderung); der Hook liest sie tolerant und erkennt selbst
//! nichts. Fehlt sie, zeigt die Statuszeile kein Ticket.
//!
//! Felder (snake_case, alle dürfen fehlen): `ticket` („ORPH-4116“), `ticket_source`
//! (`RefSource` wie in der Oberfläche: `assigned`, `branch` …), `ticket_detected` (nur
//! geraten: gedimmt zeigen), `pr` („#347“), `pr_source`, `pr_detected`.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::field::text;
use crate::view::{RefSource, RefView};

/// Endung der Datei im State-Ordner.
pub const REFS_SUFFIX: &str = ".refs.json";

/// `state\<slot>.refs.json`.
pub fn refs_path(dir: &Path, slot: &str) -> PathBuf {
    dir.join(format!("{slot}{REFS_SUFFIX}"))
}

/// Ein Bezug, wie er in der Datei steht.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefEntry {
    /// Angezeigter Text: „ORPH-4116“ bzw. „#347“.
    pub text: String,
    /// `None`, wenn die Datei eine unbekannte Quelle nennt.
    pub source: Option<RefSource>,
    /// Nur geraten: gedimmt zeigen.
    pub detected: bool,
}

/// Ticket und PR einer Kachel.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TileRefs {
    pub ticket: Option<RefEntry>,
    pub pr: Option<RefEntry>,
}

fn entry(v: Option<&RefView>) -> Option<RefEntry> {
    v.map(|r| RefEntry { text: r.text.clone(), source: Some(r.source), detected: r.detected })
}

fn source_name(s: Option<RefSource>) -> Value {
    s.and_then(|s| serde_json::to_value(s).ok()).unwrap_or(Value::Null)
}

fn read_entry(data: &Value, key: &str) -> Option<RefEntry> {
    let t = text(data.get(key))?.trim();
    if t.is_empty() {
        return None;
    }
    let source = data.get(format!("{key}_source")).and_then(|s| serde_json::from_value(s.clone()).ok());
    let detected = data.get(format!("{key}_detected")).and_then(Value::as_bool).unwrap_or(false);
    Some(RefEntry { text: t.to_string(), source, detected })
}

impl TileRefs {
    /// Aus Ticket und PR der Kachel.
    pub fn from_views(ticket: Option<&RefView>, pr: Option<&RefView>) -> TileRefs {
        TileRefs { ticket: entry(ticket), pr: entry(pr) }
    }

    /// Inhalt der Datei, Felder in fester Reihenfolge.
    pub fn to_json(&self) -> Value {
        let (t, p) = (self.ticket.as_ref(), self.pr.as_ref());
        json!({
            "ticket": t.map(|e| e.text.clone()),
            "ticket_source": source_name(t.and_then(|e| e.source)),
            "ticket_detected": t.is_some_and(|e| e.detected),
            "pr": p.map(|e| e.text.clone()),
            "pr_source": source_name(p.and_then(|e| e.source)),
            "pr_detected": p.is_some_and(|e| e.detected),
        })
    }

    /// Tolerant lesen: ein Feld mit falschem Typ gilt als fehlend, nie ein Fehler.
    pub fn parse(data: &Value) -> TileRefs {
        TileRefs { ticket: read_entry(data, "ticket"), pr: read_entry(data, "pr") }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(text: &str, source: RefSource, detected: bool) -> RefView {
        RefView { text: text.into(), source, detected, origin: String::new() }
    }

    #[test]
    fn the_file_carries_text_source_and_dimming_and_reads_back_the_same() {
        let refs = TileRefs::from_views(Some(&view("ORPH-4116", RefSource::Branch, false)), Some(&view("#347", RefSource::Chat, true)));
        let v = refs.to_json();
        assert_eq!(
            v,
            json!({"ticket": "ORPH-4116", "ticket_source": "branch", "ticket_detected": false,
                "pr": "#347", "pr_source": "chat", "pr_detected": true})
        );
        assert_eq!(TileRefs::parse(&v), refs);
        assert_eq!(TileRefs::parse(&TileRefs::default().to_json()), TileRefs::default());
    }

    #[test]
    fn a_broken_or_foreign_file_reads_as_nothing_or_as_much_as_fits() {
        assert_eq!(TileRefs::parse(&json!(["kaputt"])), TileRefs::default());
        let odd = TileRefs::parse(&json!({"ticket": "X-1", "ticket_source": "neu", "ticket_detected": "ja", "pr": 5}));
        assert_eq!(odd.ticket, Some(RefEntry { text: "X-1".into(), source: None, detected: false }));
        assert_eq!(odd.pr, None);
        assert_eq!(TileRefs::parse(&json!({"ticket": "  "})).ticket, None);
    }
}
