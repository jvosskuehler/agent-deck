//! Startmodus für neu per ＋ angelegte Agenten: die Rechnung hinter
//! `_register_pending_auto` / `_apply_pending_auto` (Python `ui/windows.py`).
//!
//! Nicht feuern-und-vergessen, sondern in drei Schritten:
//! 1. Bereitschaft: erst der erste FRISCHE Hook (neuer als eine evtl. liegengebliebene
//!    Zustandsdatei desselben Slot-Namens) zeigt, dass Claude lebt; danach noch
//!    `AUTO_READY_GRACE` warten, denn die TUI nimmt Shift+Tab so früh nicht sicher an.
//! 2. Erst-Antrieb: blind ab `MODE_START` so viele Shift+Tab, wie bis zum Ziel fehlen.
//! 3. Bestätigen/Nachfassen: erst eine Hook-Meldung NACH dem Senden zählt. Im Ziel →
//!    fertig; daneben → vom gemeldeten Modus aus nachtreiben (höchstens
//!    `AUTO_MAX_TRIES`). Ohne Meldung bleibt der Slot vorgemerkt, bis
//!    `PENDING_AUTO_TTL` abläuft.

use crate::config::{AUTO_MAX_TRIES, AUTO_READY_GRACE, PENDING_AUTO_TTL};
use crate::pystr::py_float;
use crate::slot_state::SlotState;

/// Fortschritt eines vorgemerkten Slots.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingAuto {
    /// `ts` einer beim Vormerken schon vorhandenen Zustandsdatei (0 = keine).
    pub base_ts: f64,
    /// Zeitpunkt des Vormerkens (Anker für die Geduld).
    pub reg_ts: f64,
    /// Erster frischer Hook gesehen (0 = noch nicht).
    pub ready_ts: f64,
    /// Zuletzt Shift+Tab geschickt (0 = noch nie).
    pub sent_ts: f64,
    pub tries: u32,
}

impl PendingAuto {
    pub fn new(base_ts: f64, now: f64) -> PendingAuto {
        PendingAuto { base_ts, reg_ts: now, ready_ts: 0.0, sent_ts: 0.0, tries: 0 }
    }

    /// Das Senden hat geklappt: merken, damit erst eine spätere Meldung zählt.
    pub fn sent(&mut self, now: f64) {
        self.sent_ts = now;
        self.tries += 1;
    }
}

/// Was mit einem vorgemerkten Slot in diesem Takt geschieht.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoStep {
    /// Noch warten (kein Hook, Warmlaufzeit, keine neue Meldung).
    Wait,
    /// Shift+Tab ab diesem Modus-Index bis zum Ziel schicken; bei Erfolg `sent`.
    Drive { from: usize },
    /// Fertig oder aufgegeben: Vormerkung löschen.
    Forget,
}

fn ts_of(st: &SlotState) -> f64 {
    st.get("ts").and_then(py_float).unwrap_or(0.0)
}

/// Ein Takt für einen Slot. `cycle` ist die Shift+Tab-Reihenfolge, `target`/`start`
/// Indizes darin.
pub fn decide(p: &mut PendingAuto, st: Option<&SlotState>, now: f64, cycle: &[String], target: usize, start: usize) -> AutoStep {
    if now - p.reg_ts > PENDING_AUTO_TTL {
        return AutoStep::Forget;
    }
    let Some(st) = st.filter(|s| ts_of(s) > p.base_ts) else {
        return AutoStep::Wait;
    };
    if p.ready_ts == 0.0 {
        p.ready_ts = now;
    }
    if p.sent_ts == 0.0 {
        // SessionStart meldet keinen Modus, ein vorhandener könnte vom gleichnamigen
        // Vorgänger stammen: darum bewusst ab dem Startmodus rechnen.
        if now - p.ready_ts < AUTO_READY_GRACE {
            return AutoStep::Wait;
        }
        return AutoStep::Drive { from: start };
    }
    let reported = st.get("mode").and_then(|m| m.as_str()).and_then(|m| cycle.iter().position(|c| c == m));
    match reported {
        Some(i) if ts_of(st) > p.sent_ts => {
            if i == target || p.tries >= AUTO_MAX_TRIES {
                AutoStep::Forget
            } else {
                AutoStep::Drive { from: i }
            }
        }
        _ => AutoStep::Wait,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn cycle() -> Vec<String> {
        ["manual", "accept", "plan", "auto"].iter().map(|s| s.to_string()).collect()
    }

    fn st(v: Value) -> SlotState {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn an_old_state_file_of_a_recycled_slot_name_does_not_count_as_alive() {
        let mut p = PendingAuto::new(100.0, 1000.0);
        let old = st(json!({"status": "idle", "ts": 100.0}));
        assert_eq!(decide(&mut p, Some(&old), 1001.0, &cycle(), 3, 0), AutoStep::Wait);
        assert_eq!(p.ready_ts, 0.0);
    }

    #[test]
    fn first_fresh_hook_starts_the_warm_up_then_drives_from_the_start_mode() {
        let mut p = PendingAuto::new(0.0, 1000.0);
        let fresh = st(json!({"status": "idle", "ts": 1001.0, "mode": "plan"}));
        assert_eq!(decide(&mut p, Some(&fresh), 1001.0, &cycle(), 3, 0), AutoStep::Wait);
        assert_eq!(decide(&mut p, Some(&fresh), 1002.0, &cycle(), 3, 0), AutoStep::Wait);
        assert_eq!(decide(&mut p, Some(&fresh), 1002.6, &cycle(), 3, 0), AutoStep::Drive { from: 0 });
    }

    #[test]
    fn only_a_report_after_sending_confirms_or_corrects() {
        let mut p = PendingAuto::new(0.0, 1000.0);
        p.ready_ts = 1001.0;
        p.sent(1003.0);
        let before = st(json!({"ts": 1002.0, "mode": "accept"}));
        assert_eq!(decide(&mut p, Some(&before), 1004.0, &cycle(), 3, 0), AutoStep::Wait);
        let stuck = st(json!({"ts": 1005.0, "mode": "plan"}));
        assert_eq!(decide(&mut p, Some(&stuck), 1005.1, &cycle(), 3, 0), AutoStep::Drive { from: 2 });
        p.sent(1005.1);
        let there = st(json!({"ts": 1006.0, "mode": "auto"}));
        assert_eq!(decide(&mut p, Some(&there), 1006.1, &cycle(), 3, 0), AutoStep::Forget);
    }

    #[test]
    fn gives_up_after_the_maximum_number_of_tries() {
        let mut p = PendingAuto::new(0.0, 1000.0);
        p.ready_ts = 1001.0;
        for i in 0..AUTO_MAX_TRIES {
            p.sent(1002.0 + f64::from(i));
        }
        let wrong = st(json!({"ts": 1100.0, "mode": "plan"}));
        assert_eq!(decide(&mut p, Some(&wrong), 1100.1, &cycle(), 3, 0), AutoStep::Forget);
    }

    #[test]
    fn patience_ends_after_the_time_to_live() {
        let mut p = PendingAuto::new(0.0, 1000.0);
        assert_eq!(decide(&mut p, None, 1000.0 + PENDING_AUTO_TTL + 1.0, &cycle(), 3, 0), AutoStep::Forget);
    }
}
