//! Wann der Prompt-Cache dieser Session abläuft: aus Claude Codes eigener Buchführung im
//! stdin (`prompt_cache`) oder aus dem Ende des Transcripts (`statusline.py`: `_scan`,
//! `_cache_until`, `_cache_from_transcript`).
//!
//! Nach einem Compact gibt es keinen Timer mehr, bis die nächste Anfrage rausgeht: der
//! alte Gesprächsanfang wird nie wieder gelesen. Ohne diese Regel zählte die Anzeige den
//! Cache des alten Gesprächs weiter herunter (gesehen 2026-11-09: „1 min“ direkt nach
//! `/compact`), und das Deck schickte womöglich gleich noch ein `/compact` hinterher.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use deck_core::field::flag;
use deck_core::isotime::parse_iso;
use deck_core::prompt_cache::{TTL_1H, TTL_5M, number, ttl_seconds};
use serde_json::{Map, Value};

/// So viel vom Transcript-Ende wird gelesen. Die letzte Anfrage steht fast immer in den
/// ersten 64 KB; nur wenn große Tool-Ergebnisse dazwischen liegen, wird nachgelegt.
pub const TAIL_STEPS: [u64; 3] = [64 << 10, 512 << 10, 4 << 20];

/// Was das Transcript-Ende über den Cache sagt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Scan {
    /// (Zeitpunkt der letzten Anfrage, Laufzeit in Sekunden).
    Sent(f64, u32),
    /// Hinter der letzten Anfrage steht ein Compact: kein Cache mehr, um den es ginge.
    Compacted,
}

fn obj(v: Option<&Value>) -> Option<&Map<String, Value>> {
    v.and_then(Value::as_object)
}

/// Laufzeit des Prompt-Caches, abgelesen an einer Transcript-Zeile.
///
/// `api-request` (die Anfrage selbst): ihre cache_control-Marken. Die kürzeste zählt,
/// denn mit ihr läuft der Gesprächsteil ab, der den Großteil des Caches ausmacht.
/// `assistant` (Rückfall, falls Claude Code die Anfragen nicht mehr mitschreibt): die
/// Antwort meldet, in welche Laufzeit sie den Cache geschrieben hat.
pub fn ttl_of(entry: &Map<String, Value>) -> Option<u32> {
    if entry.get("type").and_then(Value::as_str) == Some("api-request") {
        let marks = entry.get("cacheControl")?.as_array()?;
        let default = Value::from("5m");
        return marks
            .iter()
            .filter_map(Value::as_object)
            .filter_map(|m| match m.get("value") {
                Some(Value::Object(v)) => ttl_seconds(Some(v.get("ttl").unwrap_or(&default))),
                _ => ttl_seconds(Some(&default)),
            })
            .min();
    }
    let usage = obj(obj(entry.get("message")).and_then(|m| m.get("usage")));
    let made = obj(usage.and_then(|u| u.get("cache_creation")));
    let has = |k: &str| number(made.and_then(|m| m.get(k))).is_some_and(|n| n > 0.0);
    if has("ephemeral_1h_input_tokens") {
        Some(TTL_1H)
    } else if has("ephemeral_5m_input_tokens") {
        Some(TTL_5M)
    } else {
        None
    }
}

/// Die letzte Anfrage des Hauptgesprächs, von hinten gesucht.
///
/// Jede Anfrage, die den Cache liest, setzt seine Laufzeit neu; maßgeblich ist der
/// Zeitpunkt, zu dem sie RAUSGING, nicht der ihrer Antwort. Den schreibt Claude Code als
/// Zeile `api-request` mit (Stand 2.1.319, nicht dokumentiert). Fehlt sie, gilt die
/// letzte Antwort: die kommt etwas später, die Zeit ist also etwas zu großzügig.
/// Steht hinter der letzten Anfrage ein `compact_boundary` (von Hand, vom Deck oder von
/// Claude Code bei vollem Kontext), heißt das [`Scan::Compacted`]. Die Compact-Anfrage
/// selbst steht nicht als `api-request` im Transcript.
pub fn scan<'a>(lines: impl DoubleEndedIterator<Item = &'a str>) -> Option<Scan> {
    let mut fallback = None;
    for line in lines.rev() {
        if !line.contains("\"api-request\"") && !line.contains("\"assistant\"") && !line.contains("\"compact_boundary\"") {
            continue;
        }
        // Auch die angeschnittene erste Zeile fällt hier heraus.
        let Ok(Value::Object(entry)) = serde_json::from_str::<Value>(line) else { continue };
        if flag(entry.get("isSidechain")) {
            continue;
        }
        let kind = entry.get("type").and_then(Value::as_str);
        if kind == Some("system") && entry.get("subtype").and_then(Value::as_str) == Some("compact_boundary") {
            // Eine Antwort dahinter: es lief schon wieder eine Anfrage.
            return Some(fallback.unwrap_or(Scan::Compacted));
        }
        if kind == Some("api-request")
            && entry.get("querySource").map_or(Some("repl_main_thread"), Value::as_str) != Some("repl_main_thread")
        {
            continue;
        }
        if !matches!(kind, Some("api-request" | "assistant")) {
            continue;
        }
        let ts = entry.get("timestamp").and_then(Value::as_str).and_then(parse_iso).filter(|t| *t != 0.0);
        let (Some(ttl), Some(ts)) = (ttl_of(&entry), ts) else { continue };
        if kind == Some("api-request") {
            return Some(Scan::Sent(ts, ttl));
        }
        fallback.get_or_insert(Scan::Sent(ts, ttl));
    }
    fallback
}

/// (Ablaufzeit, Laufzeit) aus dem Ende des Transcripts, [`Scan::Compacted`] oder `None`.
pub fn from_transcript(path: Option<&Value>) -> Option<Scan> {
    let path = path?.as_str().filter(|p| !p.is_empty())?;
    let mut f = File::open(path).ok()?;
    let size = f.seek(SeekFrom::End(0)).ok()?;
    let mut hit = None;
    for step in TAIL_STEPS {
        f.seek(SeekFrom::Start(size.saturating_sub(step))).ok()?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).ok()?;
        let text = String::from_utf8_lossy(&buf);
        hit = scan(text.lines());
        if hit.is_some() || step >= size {
            break;
        }
    }
    match hit? {
        Scan::Sent(ts, ttl) => Some(Scan::Sent(ts + f64::from(ttl), ttl)),
        Scan::Compacted => Some(Scan::Compacted),
    }
}

/// (Ablaufzeit, Laufzeit) des Prompt-Caches dieser Session, oder `None`.
///
/// Erste Quelle ist `prompt_cache` aus dem stdin: Claude Code führt selbst Buch über
/// seine Anfragen und kennt dabei auch Fehlgriffe (Stand 2.1.319, nicht dokumentiert).
/// Das Feld fehlt aber, solange die Session in DIESEM Prozess noch keine Anfrage
/// geschickt hat, also auch direkt nach `claude --resume`; dann gilt das Transcript.
/// Ein Compact hinter der letzten Anfrage schlägt beide, darum wird das Transcript immer
/// gelesen.
pub fn cache_until(data: &Map<String, Value>) -> Option<(f64, u32)> {
    let scanned = from_transcript(data.get("transcript_path"));
    if scanned == Some(Scan::Compacted) {
        return None;
    }
    if let Some(raw) = obj(data.get("prompt_cache"))
        && let Some(ttl) = ttl_seconds(raw.get("ttl"))
    {
        if raw.get("warm") == Some(&Value::Bool(false)) {
            return Some((0.0, ttl));
        }
        if let Some(until) = number(raw.get("expires_at")).filter(|u| *u != 0.0) {
            return Some((until, ttl));
        }
    }
    match scanned {
        Some(Scan::Sent(until, ttl)) => Some((until, ttl)),
        _ => None,
    }
}
