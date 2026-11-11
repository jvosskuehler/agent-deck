//! Alle nach vorn, und mit demselben Druck wieder zurück (`deck/domain/front.py`).
//!
//! Die Taste (`config::FRONT_HOTKEYS`) holt jedes Agentenfenster in Windows Terminal nach
//! vorn und legt sie als ein Raster auf den Monitor unter der Maus. Hier steht die
//! Rechnung dahinter, ohne Fenster:
//!
//! - WER den Fokus bekommt: der Agent, der am längsten wartet (dieselbe Warteschlange wie
//!   die Sprung-Taste), sonst der zuletzt gewählte, sonst der erste. Anders als die
//!   Sprung-Taste geht es nicht reihum: wer alle holt, will beim dringendsten landen.
//! - WOHIN ein zweiter Druck führt ([`BackToggle`]): zurück in das Fenster, aus dem man
//!   kam, wie Win+D für den Desktop. Die Agenten werden dabei minimiert (seit
//!   11.11.2026), damit der Bildschirm für anderes frei ist; der nächste Druck stellt sie
//!   wieder her, in der Lage von vorher.

use std::collections::HashMap;

use serde_json::Value;

use crate::attention::queue;
use crate::slot_state::SlotState;

/// Die Einstellung `front_hotkeys` als Liste: ein Text ist eine Kombination, eine Liste
/// mehrere; Leeres und Doppeltes fällt weg. Fehlt der Schlüssel, gilt `default`.
pub fn combos(value: Option<&Value>, default: &[&str]) -> Vec<String> {
    let raw: Vec<String> = match value {
        None => default.iter().map(|s| s.to_string()).collect(),
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) => a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
        Some(_) => Vec::new(),
    };
    let mut out: Vec<String> = Vec::new();
    for item in raw {
        let t = item.trim();
        if !t.is_empty() && !out.iter().any(|o| o.eq_ignore_ascii_case(t)) {
            out.push(t.to_string());
        }
    }
    out
}

/// Wer nach dem Holen den Fokus bekommt: der am längsten Wartende, sonst der zuletzt
/// gewählte (sofern er unter den geholten ist), sonst der erste.
pub fn focus_target<S: AsRef<str>>(
    states: &HashMap<String, SlotState>,
    shown: &[S],
    active: Option<&str>,
) -> Option<String> {
    if let Some(first) = queue(states, shown).into_iter().next() {
        return Some(first);
    }
    if let Some(a) = active.filter(|a| shown.iter().any(|s| s.as_ref() == *a)) {
        return Some(a.to_string());
    }
    shown.first().map(|s| s.as_ref().to_string())
}

/// Merkt sich das Fenster, aus dem die Taste gedrückt wurde.
#[derive(Debug, Default)]
pub struct BackToggle {
    back_to: Option<isize>,
}

impl BackToggle {
    /// `fg` = Vordergrundfenster (`None` = keins, eines des Decks selbst oder ein Agent,
    /// dorthin will niemand zurück), `on_agent` = man steht bei einem Agenten und die
    /// Agenten werden jetzt versteckt. Antwort: das Fenster, in das zurückgesprungen wird.
    ///
    /// Zurück geht es nur beim Verstecken UND wenn das gemerkte Fenster noch lebt. Sonst
    /// ist `fg` der neue Ausgangspunkt.
    pub fn press(&mut self, fg: Option<isize>, on_agent: bool, alive: impl Fn(isize) -> bool) -> Option<isize> {
        if on_agent {
            return self.back_to.take().filter(|h| alive(*h));
        }
        self.back_to = fg;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn st(status: &str, ts: f64) -> SlotState {
        json!({"status": status, "ts": ts}).as_object().unwrap().clone()
    }

    #[test]
    fn the_setting_may_be_text_or_list_and_missing_means_default() {
        let d = ["ctrl+shift+<", "f20"];
        assert_eq!(combos(None, &d), ["ctrl+shift+<", "f20"]);
        assert_eq!(combos(Some(&json!("alt+<")), &d), ["alt+<"]);
        assert_eq!(combos(Some(&json!(["a+b", " f20 ", "", "F20"])), &d), ["a+b", "f20"]);
        assert!(combos(Some(&json!("")), &d).is_empty());
        assert!(combos(Some(&json!([])), &d).is_empty());
    }

    #[test]
    fn focus_goes_to_who_waits_longest_and_does_not_rotate() {
        let states = HashMap::from([
            ("A1".to_string(), st("done", 5.0)),
            ("A2".to_string(), st("waiting", 9.0)),
            ("A3".to_string(), st("waiting", 3.0)),
        ]);
        assert_eq!(focus_target(&states, &["A1", "A2", "A3"], Some("A3")).as_deref(), Some("A3"));
    }

    #[test]
    fn without_waiters_the_last_chosen_stays_else_the_first() {
        let states = HashMap::from([("A1".to_string(), st("idle", 1.0))]);
        assert_eq!(focus_target(&states, &["A1", "A2"], Some("A2")).as_deref(), Some("A2"));
        assert_eq!(focus_target(&states, &["A1", "A2"], Some("C7")).as_deref(), Some("A1"));
        assert_eq!(focus_target::<&str>(&states, &[], None), None);
    }

    #[test]
    fn a_second_press_at_an_agent_leads_back_once() {
        let mut t = BackToggle::default();
        assert_eq!(t.press(Some(500), false, |_| true), None);
        assert_eq!(t.press(Some(101), true, |_| true), Some(500));
        assert_eq!(t.press(Some(101), true, |_| true), None, "Merker verbraucht: neu holen");
    }

    #[test]
    fn a_closed_origin_or_the_deck_itself_gives_no_way_back() {
        let mut t = BackToggle::default();
        t.press(Some(500), false, |_| true);
        assert_eq!(t.press(Some(101), true, |_| false), None);
        t.press(None, false, |_| true);
        assert_eq!(t.press(Some(101), true, |_| true), None);
    }

    #[test]
    fn whoever_went_elsewhere_sets_a_new_origin_there() {
        let mut t = BackToggle::default();
        t.press(Some(500), false, |_| true);
        t.press(Some(600), false, |_| true);
        assert_eq!(t.press(Some(101), true, |_| true), Some(600));
    }
}
