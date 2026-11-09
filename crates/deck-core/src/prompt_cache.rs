//! Wie lange der Prompt-Cache einer Session noch warm ist, und wann das Deck einen
//! wartenden Agenten deshalb kompaktiert (`deck/domain/prompt_cache.py`).
//!
//! Warum es zählt: läuft der Cache ab, schreibt die nächste Nachricht den ganzen
//! Gesprächsanfang neu in den Cache, statt ihn günstig zu lesen. Die Statuszeile zeigt
//! die Restzeit (`view`), das Deck schickt kurz vor dem Ablauf `/compact`
//! ([`compact_due`], Ablauf in `auto_compact.rs`). Die Farben bleiben bei den Anzeigen.

use serde_json::Value;

use crate::config::STALE_S;
use crate::slot_state::SlotState;

/// Laufzeit des 5-Minuten-Caches in Sekunden.
pub const TTL_5M: u32 = 300;
/// Laufzeit des 1-Stunden-Caches in Sekunden.
pub const TTL_1H: u32 = 3600;

/// Wie warm der Cache noch ist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheLevel {
    Warm,
    /// Knapp: gleich weiterschreiben, sonst wird es teuer.
    Low,
    Cold,
}

/// Eine JSON-Zahl, aber kein Wahrheitswert (Python: `isinstance(v, (int, float))` und
/// nicht `bool`). Der Wert kommt aus einer Datei eines anderen Prozesses.
pub fn number(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
}

/// `"1h"` / `"5m"` (so schreiben API und Claude Code die Laufzeit) oder eine positive
/// Zahl → Sekunden. Alles andere → `None`.
pub fn ttl_seconds(value: Option<&Value>) -> Option<u32> {
    match value? {
        Value::String(s) => match s.as_str() {
            "1h" => Some(TTL_1H),
            "5m" => Some(TTL_5M),
            _ => None,
        },
        v => number(Some(v)).filter(|n| *n >= 1.0).map(|n| n as u32),
    }
}

/// (Text, Stufe) für die restliche Laufzeit in Sekunden.
///
/// Knapp ist es ab 5 Minuten vor Ablauf, beim 5-Minuten-Cache erst in der letzten
/// Minute (sonst stünde er fast die ganze Zeit auf knapp). Abgerundet wird nach unten:
/// `1 min` heißt mindestens eine Minute, nie weniger.
pub fn view(left_s: f64, ttl_s: f64, cold_word: &str) -> (String, CacheLevel) {
    if left_s <= 0.0 {
        return (cold_word.to_string(), CacheLevel::Cold);
    }
    let mins = (left_s / 60.0).floor() as u64;
    let text = if mins > 0 { format!("{mins} min") } else { "<1 min".to_string() };
    let level = if left_s < f64::min(300.0, ttl_s / 5.0) { CacheLevel::Low } else { CacheLevel::Warm };
    (text, level)
}

// ── Automatisch kompaktieren, kurz bevor der Cache abläuft ─────────────────
// Grob in API-Preisen bei 200k Kontext: kommt die nächste Nachricht nach dem Ablauf,
// schreibt sie 200k neu in den 1-h-Cache (Faktor 2, also ~400k Input-Tokens). Ein
// Compact davor liest sie günstig (Faktor 0,1), schreibt eine Zusammenfassung und
// lässt einen kurzen Stand zurück: zusammen ~100k bis 150k. Kommt niemand zurück,
// war es umsonst (~50k). Darum nur bei großem Kontext und nur beim 1-h-Cache.

/// So viel Restzeit → `/compact`.
pub const COMPACT_BEFORE_S: f64 = 300.0;
/// Knapper: das Compact läse womöglich schon kalt.
pub const COMPACT_MIN_LEFT_S: f64 = 30.0;
/// Darunter spart es kaum etwas und kostet nur Details.
pub const COMPACT_MIN_TOKENS: f64 = 60_000.0;
/// So lange nach deiner letzten Eingabe sitzt du noch dran.
pub const TYPING_GRACE_S: f64 = 120.0;
/// Agent ist fertig und wartet auf dich (roher Hook-Status).
pub const COMPACT_READY: [&str; 2] = ["done", "idle"];

/// Ist jetzt der Moment, diesen Agenten zu kompaktieren?
///
/// `status` ist der ROHE Hook-Status, nicht der für die Anzeige geglättete: ein „denkt“
/// ohne frische Meldung zeigt die Kachel als idle, der Agent kann aber in einem langen
/// Tool-Aufruf stecken. Dann landete `/compact` als Zwischennachricht. Eine offene
/// Rückfrage (`waiting`) ist ebenso tabu: der Text ginge in die Antwort.
pub fn compact_due(status: Option<&str>, until: Option<f64>, ttl: Option<f64>, ctx_tokens: Option<f64>, now: f64) -> bool {
    if !status.is_some_and(|s| COMPACT_READY.contains(&s)) {
        return false;
    }
    let (Some(until), Some(ttl)) = (until, ttl) else { return false };
    if ttl != f64::from(TTL_1H) {
        return false;
    }
    let left = until - now;
    if !(COMPACT_MIN_LEFT_S < left && left <= COMPACT_BEFORE_S) {
        return false;
    }
    ctx_tokens.is_some_and(|t| t >= COMPACT_MIN_TOKENS)
}

/// Statuswerte, bei denen der Agent arbeitet.
pub const WORKING: [&str; 2] = ["thinking", "running"];

/// Arbeitet der Agent gerade? Dann zeigen Statuszeile und Kachel keinen Timer: jede
/// Anfrage setzt ihn ohnehin neu, und auch nach einem langen Tool-Aufruf geht die
/// nächste sofort raus. Der Timer zählt erst, wenn der Agent auf dich wartet.
/// Ein „denkt“ ohne frische Meldung (z. B. mit Esc abgebrochen, dann feuert kein
/// Stop-Hook) gilt nach `STALE_S` nicht mehr, sonst bliebe der Timer für immer weg.
pub fn working(st: Option<&SlotState>, now: f64) -> bool {
    let Some(st) = st else { return false };
    if !st.get("status").and_then(Value::as_str).is_some_and(|s| WORKING.contains(&s)) {
        return false;
    }
    number(st.get("ts")).is_some_and(|ts| now - ts <= STALE_S)
}

/// Sitzt du gerade an genau diesem Agenten? Dann kein `/compact`: in VS Code holt das
/// Senden den Terminal-Tab nach vorn, und was du tippst, mischte sich in den Befehl.
/// Das Deck versucht es im nächsten Takt wieder, solange Restzeit bleibt.
pub fn user_busy(front: bool, input_idle_s: f64) -> bool {
    front && input_idle_s < TYPING_GRACE_S
}
