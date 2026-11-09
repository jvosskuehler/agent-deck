//! Restlaufzeit des Prompt-Caches und automatisches Compact (Python
//! `test_domain_prompt_cache.py`, Teil Regel von `test_ui_auto_compact.py`).

use deck_core::auto_compact::{CompactMemo, LIVE_STALE_S, due, enabled};
use deck_core::config::STALE_S;
use deck_core::prompt_cache::*;
use deck_core::slot_state::SlotState;
use serde_json::{Value, json};

const NOW: f64 = 1_000_000.0;
const BIG: Option<f64> = Some(200_000.0);
const H1: Option<f64> = Some(3600.0);

fn obj(v: Value) -> SlotState {
    v.as_object().unwrap().clone()
}

#[test]
fn the_ttl_is_read_from_the_api_notation() {
    assert_eq!(ttl_seconds(Some(&json!("1h"))), Some(3600));
    assert_eq!(ttl_seconds(Some(&json!("5m"))), Some(300));
    assert_eq!(ttl_seconds(Some(&json!(900))), Some(900));
    assert_eq!(ttl_seconds(Some(&json!("2h"))), None);
    assert_eq!(ttl_seconds(None), None);
    assert_eq!(ttl_seconds(Some(&Value::Null)), None);
    assert_eq!(ttl_seconds(Some(&json!(true))), None, "ein Wahrheitswert ist keine Laufzeit");
}

#[test]
fn minutes_round_down_never_up() {
    assert_eq!(view(42.0 * 60.0 + 59.0, 3600.0, "kalt"), ("42 min".to_string(), CacheLevel::Warm));
    assert_eq!(view(59.0, 3600.0, "kalt"), ("<1 min".to_string(), CacheLevel::Low));
    assert_eq!(view(0.0, 3600.0, "kalt"), ("kalt".to_string(), CacheLevel::Cold));
    assert_eq!(view(-5.0, 3600.0, "cold"), ("cold".to_string(), CacheLevel::Cold));
}

#[test]
fn low_from_five_minutes_before_but_with_the_short_cache_only_in_the_last_minute() {
    assert_eq!(view(5.0 * 60.0, 3600.0, "").1, CacheLevel::Warm);
    assert_eq!(view(5.0 * 60.0 - 1.0, 3600.0, "").1, CacheLevel::Low);
    assert_eq!(view(3.0 * 60.0, 300.0, "").1, CacheLevel::Warm, "5-Minuten-Cache: sonst fast immer knapp");
    assert_eq!(view(59.0, 300.0, "").1, CacheLevel::Low);
}

#[test]
fn compact_comes_in_the_last_five_minutes_of_the_hour_cache() {
    assert!(compact_due(Some("done"), Some(NOW + 300.0), H1, BIG, NOW));
    assert!(compact_due(Some("idle"), Some(NOW + 60.0), H1, BIG, NOW));
    assert!(!compact_due(Some("done"), Some(NOW + 301.0), H1, BIG, NOW), "noch zu früh");
    assert!(!compact_due(Some("done"), Some(NOW + 30.0), H1, BIG, NOW), "läse schon kalt");
    assert!(!compact_due(Some("done"), Some(NOW - 5.0), H1, BIG, NOW), "abgelaufen");
}

#[test]
fn not_with_the_five_minute_cache_and_not_without_an_expiry() {
    assert!(!compact_due(Some("done"), Some(NOW + 120.0), Some(300.0), BIG, NOW));
    assert!(!compact_due(Some("done"), None, H1, BIG, NOW), "nach Compact: kein Timer");
}

#[test]
fn only_when_the_agent_waits_for_you() {
    // Roher Hook-Status: „denkt“ kann ein langer Tool-Aufruf sein, `waiting` eine offene
    // Rückfrage. In beiden Fällen landete /compact an der falschen Stelle.
    for status in [Some("thinking"), Some("running"), Some("waiting"), None, Some("unknown")] {
        assert!(!compact_due(status, Some(NOW + 120.0), H1, BIG, NOW), "{status:?}");
    }
}

#[test]
fn a_small_context_is_not_worth_a_compact() {
    assert!(!compact_due(Some("done"), Some(NOW + 120.0), H1, Some(COMPACT_MIN_TOKENS - 1.0), NOW));
    assert!(compact_due(Some("done"), Some(NOW + 120.0), H1, Some(COMPACT_MIN_TOKENS), NOW));
    assert!(!compact_due(Some("done"), Some(NOW + 120.0), H1, None, NOW));
}

#[test]
fn whoever_is_typing_in_this_window_is_not_disturbed() {
    assert!(user_busy(true, 5.0));
    assert!(!user_busy(true, TYPING_GRACE_S), "vorn, aber lange nichts getippt");
    assert!(!user_busy(false, 0.0), "du tippst woanders");
}

#[test]
fn no_timer_runs_while_the_agent_works() {
    let st = |v: Value| obj(v);
    assert!(working(Some(&st(json!({"status": "thinking", "ts": NOW - 30.0}))), NOW));
    assert!(working(Some(&st(json!({"status": "running", "ts": NOW - 600.0}))), NOW), "langer Tool-Aufruf");
    assert!(!working(Some(&st(json!({"status": "done", "ts": NOW}))), NOW));
    assert!(!working(Some(&st(json!({"status": "waiting", "ts": NOW}))), NOW));
    // Mit Esc abgebrochen: kein Stop-Hook, „denkt“ bliebe stehen. Nach der Frist zählt es nicht mehr.
    assert!(!working(Some(&st(json!({"status": "thinking", "ts": NOW - STALE_S - 1.0}))), NOW));
    assert!(!working(None, NOW) && !working(Some(&st(json!({}))), NOW));
}

// ── Woher die Zahlen kommen (auto_compact) ──────────────────────────────────

fn live(rest: f64, ttl: Value, tokens: f64, age: f64) -> SlotState {
    obj(json!({"cache_until": NOW + rest, "cache_ttl": ttl, "ctx_tokens": tokens, "ts": NOW - age}))
}

fn done() -> SlotState {
    obj(json!({"status": "done"}))
}

#[test]
fn a_waiting_agent_with_a_fresh_live_file_is_due_with_its_expiry() {
    let lv = live(120.0, json!(3600), 200_000.0, 5.0);
    assert_eq!(due(Some(&done()), Some(&lv), NOW), Some(NOW + 120.0));
    assert_eq!(due(Some(&obj(json!({"status": "thinking"}))), Some(&lv), NOW), None);
    assert_eq!(due(None, Some(&lv), NOW), None, "ohne Hook-Status kein Compact");
    assert_eq!(due(Some(&done()), None, NOW), None);
}

#[test]
fn an_old_live_file_means_claude_is_closed_and_compact_would_land_in_the_shell() {
    let old = live(120.0, json!(3600), 200_000.0, LIVE_STALE_S + 1.0);
    assert_eq!(due(Some(&done()), Some(&old), NOW), None);
    let no_ts = obj(json!({"cache_until": NOW + 120.0, "cache_ttl": 3600, "ctx_tokens": 200_000}));
    assert_eq!(due(Some(&done()), Some(&no_ts), NOW), None);
}

#[test]
fn the_memo_sends_once_per_cache_state() {
    let mut memo = CompactMemo::default();
    assert!(!memo.already("A1", NOW + 120.0));
    memo.mark("A1", NOW + 120.0);
    assert!(memo.already("A1", NOW + 120.0));
    assert!(!memo.already("A1", NOW + 3720.0), "neuer Stand: wieder");
    assert!(!memo.already("B1", NOW + 120.0));
}

#[test]
fn the_switch_is_on_unless_it_is_turned_off() {
    assert!(enabled(&obj(json!({}))));
    assert!(enabled(&obj(json!({"auto_compact": true}))));
    assert!(!enabled(&obj(json!({"auto_compact": false}))));
}
