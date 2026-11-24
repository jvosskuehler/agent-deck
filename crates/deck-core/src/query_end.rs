//! Wann eine Rückfrage endet, ohne dass ein Hook es meldet.
//!
//! Lehnt man eine Berechtigungsfrage ab oder bricht mit Esc ab, ruft Claude Code keinen
//! Hook auf (Hooks-Referenz: Stop läuft bei einem Abbruch nicht, PermissionDenied nur bei
//! der Auto-Modus-Prüfung; gemessen mit 2.1.331). Die Zustandsdatei bliebe auf `waiting`
//! und die Kachel orange, bis zum nächsten Prompt. Claude schreibt die Ablehnung aber
//! sofort ins Transcript (gemessen: 0,2 s nach Esc), und dort schaut das Deck nach.
//!
//! Eine offene Rückfrage darf nie verschwinden. Darum zählen nur Einträge, die es erst
//! gibt, wenn der Dialog zu ist: die Ablehnung (`toolUseResult` „User rejected tool use“
//! bzw. ihr Text), der Abbruch („[Request interrupted by user“) und ein Compact
//! (`compact_boundary`, startet nur an der Eingabezeile). Nur aus dem Hauptverlauf:
//! Unteragenten haben eigene Dialoge. Alles andere schreibt Claude auch bei offenem
//! Dialog oder kurz davor (Anhänge, Modus, file-history, Protokoll der Anfrage, das
//! Ergebnis eines parallelen Werkzeugs) und bleibt außen vor. Die Statuszeile taugt
//! nicht: sie ruht, solange der Dialog offen ist, und läuft danach ohne erkennbares
//! Zeichen weiter.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

use serde_json::Value;

use crate::field::{number, text};
use crate::isotime::parse_iso;
use crate::slot_state::SlotState;

/// Wie die Rückfrage endete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryEnd {
    Rejected,
    Interrupted,
    Compacted,
}

impl QueryEnd {
    /// Fürs Protokoll.
    pub fn label(self) -> &'static str {
        match self {
            QueryEnd::Rejected => "abgelehnt",
            QueryEnd::Interrupted => "abgebrochen",
            QueryEnd::Compacted => "kompaktiert",
        }
    }
}

/// So viel früher als die Meldung darf ein Ende liegen und gilt doch. `deck-report`
/// nimmt seine Zeit erst nach dem Start des Prozesses (in WSL bis gut 0,1 s), ein Esc in
/// dieser Lücke gehört zu dieser Rückfrage. Die Ablehnung einer früheren liegt
/// mindestens einen Modell-Durchlauf zurück.
pub const SLACK_S: f64 = 0.5;

/// Beim ersten Blick höchstens so viel vom Ende des Transcripts: ein Eintrag mit dem
/// Protokoll einer Anfrage kann einige hundert KB groß sein.
pub const TAIL_BYTES: u64 = 1 << 20;

const REJECTED_RESULT: &str = "User rejected tool use";
const REJECTED_TEXT: &str = "The user doesn't want to proceed with this tool use";
const INTERRUPTED_TEXT: &str = "[Request interrupted by user";

fn starts(v: Option<&Value>, prefix: &str) -> bool {
    match v {
        Some(Value::String(s)) => s.starts_with(prefix),
        Some(Value::Array(blocks)) => blocks.iter().any(|b| text(b.get("text")).is_some_and(|t| t.starts_with(prefix))),
        _ => false,
    }
}

fn user_end(rec: &Value) -> Option<QueryEnd> {
    if rec.get("toolUseResult").and_then(Value::as_str) == Some(REJECTED_RESULT) {
        return Some(QueryEnd::Rejected);
    }
    let content = rec.get("message")?.get("content")?;
    if starts(Some(content), INTERRUPTED_TEXT) {
        return Some(QueryEnd::Interrupted);
    }
    let rejected = content.as_array()?.iter().any(|b| {
        b.get("type").and_then(Value::as_str) == Some("tool_result")
            && b.get("is_error").and_then(Value::as_bool) == Some(true)
            && starts(b.get("content"), REJECTED_TEXT)
    });
    rejected.then_some(QueryEnd::Rejected)
}

/// Beendet dieser Transcript-Eintrag eine Rückfrage, die um `since` gemeldet wurde?
pub fn end_of(rec: &Value, since: f64) -> Option<QueryEnd> {
    if rec.get("isSidechain").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let ts = parse_iso(text(rec.get("timestamp"))?)?;
    if ts < since - SLACK_S {
        return None;
    }
    match rec.get("type").and_then(Value::as_str)? {
        "user" => user_end(rec),
        "system" if rec.get("subtype").and_then(Value::as_str) == Some("compact_boundary") => Some(QueryEnd::Compacted),
        _ => None,
    }
}

/// Das erste Ende in einem Stück Transcript (JSON-Zeilen, kaputte Zeilen zählen nicht).
pub fn scan(chunk: &str, since: f64) -> Option<QueryEnd> {
    chunk.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).find_map(|rec| end_of(&rec, since))
}

/// Eine beobachtete Rückfrage: ab wann, in welchem Transcript, bis wohin gelesen.
#[derive(Debug)]
struct Watch {
    since: f64,
    path: PathBuf,
    /// Bis hierhin sind ganze Zeilen gelesen; `None` = noch nie.
    read_to: Option<u64>,
    end: Option<QueryEnd>,
}

impl Watch {
    /// Neue ganze Zeilen lesen und prüfen. Gelesen wird nur, was dazukam.
    fn poll(&mut self) {
        let Ok(mut f) = File::open(&self.path) else { return };
        let Ok(len) = f.metadata().map(|m| m.len()) else { return };
        let (start, partial) = match self.read_to {
            Some(r) if r <= len => (r, false),
            // Erster Blick oder Datei kürzer geworden: nur das Ende, die erste
            // angeschnittene Zeile fällt weg.
            _ => {
                let s = len.saturating_sub(TAIL_BYTES);
                (s, s > 0)
            }
        };
        if len == start {
            self.read_to = Some(start);
            return;
        }
        let mut buf = Vec::new();
        if f.seek(SeekFrom::Start(start)).is_err() || f.take(len - start).read_to_end(&mut buf).is_err() {
            return;
        }
        // Eine halb geschriebene letzte Zeile kommt beim nächsten Mal ganz.
        let Some(last_nl) = buf.iter().rposition(|b| *b == b'\n') else {
            self.read_to = Some(start);
            return;
        };
        self.read_to = Some(start + last_nl as u64 + 1);
        let mut lines = &buf[..=last_nl];
        if partial {
            let first = lines.iter().position(|b| *b == b'\n').unwrap_or(0);
            lines = &lines[first + 1..];
        }
        self.end = scan(&String::from_utf8_lossy(lines), self.since);
    }
}

/// Beobachtet die Transcripts der Slots, die `waiting` melden.
#[derive(Debug, Default)]
pub struct QueryWatch {
    slots: HashMap<String, Watch>,
}

impl QueryWatch {
    /// Für jeden Slot mit `waiting` nachsehen, ob die Rückfrage im Transcript schon
    /// erledigt ist; dann gilt er als `idle`, nur in `states` (die Datei gehört den
    /// Hooks, ihre nächste Meldung gilt wieder). Ohne `transcript` in der Meldung bleibt
    /// alles, wie es ist. Rückgabe: die Slots, die eben erst als erledigt erkannt wurden.
    pub fn settle(&mut self, states: &mut HashMap<String, SlotState>) -> Vec<(String, QueryEnd)> {
        self.slots.retain(|slot, _| states.get(slot).is_some_and(is_waiting));
        let mut fresh = Vec::new();
        for (slot, st) in states.iter_mut().filter(|(_, st)| is_waiting(st)) {
            let (Some(since), Some(path)) = (number(st.get("ts")), text(st.get("transcript")).map(PathBuf::from)) else {
                self.slots.remove(slot);
                continue;
            };
            let w = self.slots.entry(slot.clone()).or_insert_with(|| Watch { since, path: path.clone(), read_to: None, end: None });
            if w.since != since || w.path != path {
                *w = Watch { since, path, read_to: None, end: None };
            }
            if w.end.is_none() {
                w.poll();
                if let Some(end) = w.end {
                    fresh.push((slot.clone(), end));
                }
            }
            if w.end.is_some() {
                st.insert("status".into(), "idle".into());
            }
        }
        fresh.sort_by(|a, b| a.0.cmp(&b.0));
        fresh
    }
}

fn is_waiting(st: &SlotState) -> bool {
    st.get("status").and_then(Value::as_str) == Some("waiting")
}
