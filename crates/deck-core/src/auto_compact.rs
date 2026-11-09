//! Automatisch `/compact`, kurz bevor der Prompt-Cache eines Agenten abläuft: woher die
//! Zahlen kommen und wie oft gesendet wird (`deck/ui/auto_compact.py`, ohne Senden).
//!
//! Ob es soweit ist, entscheidet [`crate::prompt_cache::compact_due`]. Die Ablaufzeit
//! und die Kontextgröße schreibt die Statuszeile in `state\<slot>.live.json`, den Status
//! die Hooks. Gesendet wird in der App (`app/src-tauri/src/auto_compact.rs`), denn dafür
//! braucht es die Hosts und den Blick auf das Fenster vorn.
//!
//! Je Cache-Stand wird höchstens einmal gesendet ([`CompactMemo`]). Klappt das Compact,
//! verschwindet die Ablaufzeit aus `live.json` (die Statuszeile sieht im Transcript den
//! `compact_boundary`); klappt es nicht, bleibt sie stehen, und dann soll das Deck nicht
//! im Takt nachlegen.

use std::collections::HashMap;

use serde_json::{Map, Value};

use crate::prompt_cache::{compact_due, number};
use crate::slot_state::SlotState;

/// Schlüssel in `deck_settings.json`; fehlt er, ist die Automatik an.
pub const SETTING: &str = "auto_compact";
/// `live.json` älter: die Statuszeile läuft nicht mehr, Claude ist zu. Ein `/compact`
/// landete dann in der Shell. Die Statuszeile zeichnet alle 15 s neu
/// (`hook_setup::STATUSLINE_REFRESH_S`), eine laufende Session bleibt also frisch.
pub const LIVE_STALE_S: f64 = 60.0;

/// Ist die Automatik eingeschaltet? Standard an; nur ein ausdrückliches `false` schaltet ab.
pub fn enabled(settings: &Map<String, Value>) -> bool {
    settings.get(SETTING).and_then(Value::as_bool).unwrap_or(true)
}

/// Die Ablaufzeit, für die jetzt `/compact` fällig ist, oder `None`.
pub fn due(st: Option<&SlotState>, lv: Option<&SlotState>, now: f64) -> Option<f64> {
    let lv = lv?;
    let ts = number(lv.get("ts"))?;
    if now - ts > LIVE_STALE_S {
        return None;
    }
    let status = st.and_then(|s| s.get("status")).and_then(Value::as_str);
    let until = number(lv.get("cache_until"));
    let due = compact_due(status, until, number(lv.get("cache_ttl")), number(lv.get("ctx_tokens")), now);
    if due { until } else { None }
}

/// Für welchen Cache-Stand je Slot schon gesendet wurde.
#[derive(Debug, Clone, Default)]
pub struct CompactMemo {
    sent: HashMap<String, f64>,
}

impl CompactMemo {
    /// Ging für genau diesen Stand schon ein `/compact` raus? Ein neuer Stand (du hast
    /// geschrieben, der Timer lief neu und wieder fast ab) zählt als neu.
    pub fn already(&self, slot: &str, until: f64) -> bool {
        self.sent.get(slot) == Some(&until)
    }

    pub fn mark(&mut self, slot: &str, until: f64) {
        self.sent.insert(slot.to_string(), until);
    }
}
