//! Sprung-Taste (Python `test_domain_attention.py`) und Umsortieren
//! (`test_domain_ordering.py`).

use std::collections::HashMap;

use deck_core::attention::{next_slot, queue};
use deck_core::ordering::*;
use deck_core::slot_state::SlotState;
use serde_json::json;

fn states() -> HashMap<String, SlotState> {
    let raw = json!({
        "A1": {"status": "done", "ts": 100},
        "A2": {"status": "waiting", "ts": 300},
        "B1": {"status": "waiting", "ts": 200},
        "B2": {"status": "thinking", "ts": 50},
        "B3": {"status": "idle", "ts": 10},
    });
    raw.as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.as_object().unwrap().clone())).collect()
}
const SHOWN: [&str; 5] = ["A1", "A2", "B1", "B2", "B3"];

#[test]
fn questions_come_before_unread_answers_oldest_first() {
    assert_eq!(queue(&states(), &SHOWN), ["B1", "A2", "A1"]);
}

#[test]
fn the_first_press_leads_to_the_longest_waiting() {
    assert_eq!(next_slot(&states(), &SHOWN, None).as_deref(), Some("B1"));
    assert_eq!(next_slot(&states(), &SHOWN, Some("B2")).as_deref(), Some("B1"));
}

#[test]
fn further_presses_go_round_robin() {
    assert_eq!(next_slot(&states(), &SHOWN, Some("B1")).as_deref(), Some("A2"));
    assert_eq!(next_slot(&states(), &SHOWN, Some("A2")).as_deref(), Some("A1"));
    assert_eq!(next_slot(&states(), &SHOWN, Some("A1")).as_deref(), Some("B1"));
}

#[test]
fn only_shown_tiles_count_and_without_waiters_nothing_happens() {
    assert_eq!(next_slot(&states(), &["B2", "B3"], None), None);
    assert_eq!(next_slot(&states(), &["A1"], None).as_deref(), Some("A1"));
    let broken: HashMap<String, SlotState> =
        [("X1".to_string(), json!({"status": "waiting", "ts": "kaputt"}).as_object().unwrap().clone())].into();
    assert_eq!(next_slot(&broken, &["X1"], None).as_deref(), Some("X1"));
}

#[test]
fn the_saved_order_wins_over_the_reported_one() {
    assert_eq!(apply_order(&["C", "A"], &["A", "B", "C"]), ["C", "A", "B"]);
}

#[test]
fn unknown_entries_append_in_report_order_and_vanished_ones_drop_out() {
    assert_eq!(apply_order(&["B"], &["A", "B", "C"]), ["B", "A", "C"]);
    assert_eq!(apply_order(&["A1", "A2", "A3"], &["A1", "A3"]), ["A1", "A3"]);
}

#[test]
fn duplicate_saved_entries_line_up_only_once() {
    assert_eq!(apply_order(&["A", "A", "B"], &["A", "B"]), ["A", "B"]);
    assert_eq!(apply_order::<&str, &str>(&[], &["A", "B"]), ["A", "B"]);
    assert!(apply_order::<&str, &str>(&["A", "B"], &[]).is_empty());
}

#[test]
fn merge_visible_refills_only_the_places_of_visible_items() {
    assert_eq!(merge_visible(&["frontend", "deck", "backend"], &["backend", "deck"]), ["frontend", "backend", "deck"]);
    assert_eq!(merge_visible(&["deck"], &["deck", "neu"]), ["deck", "neu"]);
    assert_eq!(merge_visible::<&str, &str>(&[], &["c", "a", "b"]), ["c", "a", "b"]);
    assert_eq!(merge_visible(&["zu", "a", "b"], &["b", "a"]), ["zu", "b", "a"]);
}

// Drei Blöcke mit UNTERSCHIEDLICHEN Höhen: Stapel 0..10, 15..45, 50..70.
const H: [f64; 3] = [10.0, 30.0, 20.0];
const GAP: f64 = 5.0;
const TOP: f64 = 0.0;

#[test]
fn slot_edges_are_the_collapsed_stack() {
    assert_eq!(slot_tops(&H, GAP, TOP, 0), [0.0, 35.0, 60.0]);
    assert_eq!(slot_tops(&H, GAP, TOP, 2), [0.0, 15.0, 50.0]);
}

#[test]
fn far_above_is_position_zero_and_far_below_the_last() {
    assert_eq!(drop_index(&H, GAP, TOP, 2, -40.0), 0);
    assert_eq!(drop_index(&H, GAP, TOP, 0, 999.0), 2);
}

#[test]
fn the_nearest_slot_edge_wins_and_an_offset_moves_the_thresholds() {
    assert_eq!(drop_index(&H, GAP, TOP, 0, 17.4), 0);
    assert_eq!(drop_index(&H, GAP, TOP, 0, 17.6), 1);
    assert_eq!(drop_index(&H, GAP, TOP, 0, 47.4), 1);
    assert_eq!(drop_index(&H, GAP, TOP, 0, 47.6), 2);
    assert_eq!(drop_index(&H, GAP, TOP, 1, TOP), 0, "auch der höchste Block erreicht Position 0");
    assert_eq!(drop_index(&H, GAP, 100.0, 0, 117.4), 0);
    assert_eq!(drop_index(&H, GAP, 100.0, 0, 117.6), 1);
}

#[test]
fn reflow_leaves_a_gap_as_high_as_the_dragged_item() {
    assert_eq!(reflow_tops(&H, GAP, TOP, 1, 1), [(0, 0.0), (2, 50.0)].into());
    assert_eq!(reflow_tops(&H, GAP, TOP, 0, 2), [(1, 0.0), (2, 35.0)].into());
    assert_eq!(reflow_tops(&H, GAP, TOP, 2, 0), [(0, 25.0), (1, 40.0)].into());
    for tgt in 0..3 {
        assert!(!reflow_tops(&H, GAP, TOP, 1, tgt).contains_key(&1));
    }
}

#[test]
fn drop_index_and_reflow_share_one_idea_of_position() {
    for (frm, dragged) in H.iter().enumerate() {
        for tgt in 0..H.len() {
            let tops = reflow_tops(&H, GAP, TOP, frm, tgt);
            let gap_top = slot_tops(&H, GAP, TOP, frm)[tgt];
            for (i, y) in &tops {
                assert!(y + H[*i] <= gap_top || *y >= gap_top + dragged, "{frm} {tgt} {i}");
            }
            assert_eq!(drop_index(&H, GAP, TOP, frm, gap_top), tgt, "{frm} {tgt}");
        }
    }
}

#[test]
fn move_to_places_the_item_and_clamps_the_target() {
    assert_eq!(move_to(&["A", "B", "C"], "A", 2), ["B", "C", "A"]);
    assert_eq!(move_to(&["A", "B", "C"], "C", 0), ["C", "A", "B"]);
    assert_eq!(move_to(&["A", "B", "C"], "B", 1), ["A", "B", "C"]);
    assert_eq!(move_to(&["A", "B"], "A", 99), ["B", "A"]);
    assert_eq!(move_to(&["A", "B"], "B", -3), ["B", "A"]);
}

#[test]
fn the_home_block_always_stands_on_top() {
    assert_eq!(pin_first(&["A", "B", "C"], Some("C")), ["C", "A", "B"]);
    assert_eq!(pin_first(&["A", "B"], Some("A")), ["A", "B"]);
    assert_eq!(pin_first(&["A", "B"], None), ["A", "B"]);
    assert_eq!(pin_first(&["A", "B"], Some("Z")), ["A", "B"]);
}
