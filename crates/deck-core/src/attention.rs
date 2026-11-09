//! Wer braucht mich als Nächstes? Die Rechnung hinter der Sprung-Taste.
//!
//! Mit fünf, sechs Agenten nebeneinander ist nicht das Lesen der Engpass, sondern die
//! Frage, wer gerade auf dich wartet. Die Sprung-Taste holt das Pane nach vorn, das am
//! längsten wartet: erst Rückfragen (`waiting`, der Agent steht), dann fertige, noch
//! ungelesene Antworten (`done`), innerhalb jeder Gruppe die älteste zuerst. Wer schon
//! vorn ist, gibt beim nächsten Druck an den Nächsten weiter (reihum).

use std::collections::HashMap;

use crate::pystr::py_float;
use crate::slot_state::SlotState;

pub const PRIORITY: [&str; 2] = ["waiting", "done"];

/// Die wartenden Slots unter den angezeigten, in der Reihenfolge ihres Drankommens.
pub fn queue<S: AsRef<str>>(states: &HashMap<String, SlotState>, shown: &[S]) -> Vec<String> {
    let mut cands: Vec<(usize, f64, String)> = Vec::new();
    for slot in shown {
        let slot = slot.as_ref();
        let Some(st) = states.get(slot) else { continue };
        let Some(status) = st.get("status").and_then(|v| v.as_str()) else { continue };
        let Some(prio) = PRIORITY.iter().position(|p| *p == status) else { continue };
        // `float(st.get("ts") or 0)`, ein kaputter Wert zählt als 0.
        let ts = st.get("ts").and_then(py_float).unwrap_or(0.0);
        cands.push((prio, ts, slot.to_string()));
    }
    cands.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2)));
    cands.into_iter().map(|(_, _, s)| s).collect()
}

/// Wohin die Sprung-Taste führt: der erste Wartende, oder (steht man schon bei einem
/// Wartenden) der nächste nach ihm. `None`, wenn niemand wartet.
pub fn next_slot<S: AsRef<str>>(
    states: &HashMap<String, SlotState>,
    shown: &[S],
    current: Option<&str>,
) -> Option<String> {
    let q = queue(states, shown);
    if q.is_empty() {
        return None;
    }
    if let Some(i) = current.and_then(|c| q.iter().position(|s| s == c)) {
        return Some(q[(i + 1) % q.len()].clone());
    }
    Some(q[0].clone())
}
