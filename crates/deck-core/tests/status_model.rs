//! status_model: Statusinterpretation ohne Anzeige (Python `test_domain_status_model.py`).

use deck_core::status_model::*;
use serde_json::{Value, json};

const CYCLE: [&str; 4] = ["manual", "accept", "plan", "auto"];
const GLOW: [&str; 6] = ["idle", "done", "thinking", "running", "waiting", "none"];

fn st(v: Value) -> deck_core::slot_state::SlotState {
    v.as_object().unwrap().clone()
}

#[test]
fn a_report_is_fresh_only_within_the_stale_window() {
    assert!(is_fresh(Some(&st(json!({"ts": 100}))), 100.0, 900.0));
    assert!(!is_fresh(Some(&st(json!({"ts": 100}))), 1001.0, 900.0));
    assert!(!is_fresh(None, 100.0, 900.0));
    assert!(!is_fresh(Some(&st(json!({}))), 100.0, 900.0), "ein leerer Zustand zählt wie keiner");
}

#[test]
fn thinking_without_fresh_report_falls_asleep_and_unknown_status_is_idle() {
    assert_eq!(normalize_status("thinking", false, &GLOW), "idle");
    assert_eq!(normalize_status("thinking", true, &GLOW), "thinking");
    assert_eq!(normalize_status("running", false, &GLOW), "idle");
    assert_eq!(normalize_status("waiting", false, &GLOW), "waiting");
    assert_eq!(normalize_status("bogus", true, &GLOW), "idle");
    assert_eq!(normalize_status("none", true, &GLOW), "none");
}

#[test]
fn a_remote_tile_without_report_is_unknown_not_idle() {
    assert_eq!(initial_status(None, true), "unknown");
    assert_eq!(initial_status(None, false), "idle");
    assert_eq!(initial_status(Some(&st(json!({"status": "waiting"}))), true), "waiting");
    assert_eq!(initial_status(Some(&st(json!({"ts": 1}))), false), "idle");
}

#[test]
fn lost_needs_a_fresh_active_agent_without_connection() {
    assert!(is_lost("thinking", true, false));
    assert!(!is_lost("thinking", true, true));
    assert!(!is_lost("none", true, false));
    assert!(!is_lost("idle", false, false));
}

#[test]
fn the_handle_shows_the_most_urgent_status() {
    assert_eq!(dominant_status(&["idle", "thinking", "done", "waiting"]), "waiting");
    assert_eq!(dominant_status(&["idle", "thinking", "done"]), "done");
    assert_eq!(dominant_status(&["idle", "thinking", "lost"]), "lost");
    assert_eq!(dominant_status(&["idle", "running"]), "thinking");
    assert_eq!(dominant_status(&["idle", "idle"]), "idle");
    assert_eq!(dominant_status::<&str>(&[]), "none");
    assert_eq!(dominant_status(&["none"]), "none");
    assert_eq!(dominant_status(&["bogus"]), "none");
}

#[test]
fn only_a_more_urgent_state_flashes_the_handle() {
    assert!(escalated("idle", "waiting"));
    assert!(escalated("thinking", "done"));
    assert!(!escalated("done", "idle"));
    assert!(!escalated("waiting", "done"));
    assert!(!escalated("done", "done"));
    assert!(!escalated("thinking", "running"));
    assert!(!escalated("running", "thinking"));
}

#[test]
fn remembered_effort_wins_only_where_the_statusline_cannot_tell_xhigh_from_ultracode() {
    assert_eq!(resolve_effort("", Some("ultracode")), "ultracode");
    assert_eq!(resolve_effort("xhigh", Some("ultracode")), "ultracode");
    assert_eq!(resolve_effort("high", Some("ultracode")), "high");
    assert_eq!(resolve_effort("xhigh", None), "xhigh");
    assert_eq!(resolve_effort("", None), "");
}

#[test]
fn a_hook_mode_is_adopted_only_from_a_newer_event_with_a_valid_mode() {
    assert_eq!(adopt_hook_mode(0.0, &st(json!({"mode": "plan", "ts": 5})), &CYCLE), Some((2, 5.0)));
    assert_eq!(adopt_hook_mode(9.0, &st(json!({"mode": "plan", "ts": 5})), &CYCLE), None);
    assert_eq!(adopt_hook_mode(0.0, &st(json!({"mode": "bogus", "ts": 5})), &CYCLE), None);
    assert_eq!(adopt_hook_mode(0.0, &st(json!({"ts": 5})), &CYCLE), None);
}

#[test]
fn bypass_sits_directly_behind_plan_in_the_cycle() {
    assert_eq!(mode_cycle(&CYCLE, true), ["manual", "accept", "plan", "bypass", "auto"]);
    assert_eq!(mode_cycle(&CYCLE, false), CYCLE);
    assert_eq!(mode_cycle(&mode_cycle(&CYCLE, true), true).iter().filter(|m| *m == "bypass").count(), 1);
    assert_eq!(mode_cycle(&["manual", "auto"], true), ["manual", "auto"]);
}

#[test]
fn from_bypass_auto_is_one_step_and_manual_two() {
    let cyc = mode_cycle(&CYCLE, true);
    let idx = |m: &str| cyc.iter().position(|c| c == m);
    assert_eq!(mode_steps(idx("bypass"), "auto", &cyc, "manual"), Some((1, 4)));
    assert_eq!(mode_steps(idx("bypass"), "manual", &cyc, "manual"), Some((2, 0)));
    assert_eq!(mode_steps(idx("plan"), "bypass", &cyc, "manual"), Some((1, 3)));
}

#[test]
fn mode_steps_count_shift_tab_cyclically_from_the_assumed_mode() {
    assert_eq!(mode_steps(None, "auto", &CYCLE, "manual"), Some((3, 3)));
    assert_eq!(mode_steps(None, "plan", &CYCLE, "manual"), Some((2, 2)));
    assert_eq!(mode_steps(None, "manual", &CYCLE, "manual"), Some((0, 0)));
    assert_eq!(mode_steps(Some(2), "auto", &CYCLE, "manual"), Some((1, 3)));
    assert_eq!(mode_steps(Some(3), "accept", &CYCLE, "manual"), Some((2, 1)));
    assert_eq!(mode_steps(None, "bogus", &CYCLE, "manual"), None);
    assert_eq!(mode_steps(None, "plan", &CYCLE, "weird"), Some((2, 2)));
}
