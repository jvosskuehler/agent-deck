//! Reihenfolge beim Nachkacheln (`arrange::keep_order`).
use deck_core::arrange::keep_order;

fn v(s: &[&str]) -> Vec<String> {
    s.iter().map(|x| x.to_string()).collect()
}

#[test]
fn closing_one_leaves_each_remaining_window_exactly_once_in_its_place() {
    let lying = v(&["A1", "A2", "A3"]);
    let live = v(&["A1", "A2", "A3"]);
    assert_eq!(keep_order(&lying, &live, Some("A2")), v(&["A1", "A3"]));
}

#[test]
fn windows_keep_the_order_they_lie_in_and_new_ones_come_last() {
    let lying = v(&["A3", "A1"]);
    let live = v(&["A1", "A2", "A3"]);
    assert_eq!(keep_order(&lying, &live, None), v(&["A3", "A1", "A2"]));
}

#[test]
fn a_window_that_lies_there_but_no_longer_lives_is_left_out() {
    let lying = v(&["A1", "A9", "A2"]);
    let live = v(&["A2", "A1"]);
    assert_eq!(keep_order(&lying, &live, None), v(&["A1", "A2"]));
}

use deck_core::arrange::{Screen, resized_in_place};

fn screen(name: &str, w: i64, h: i64, scale: f64) -> Screen {
    (name.to_string(), (0, 0, w, h), scale)
}

#[test]
fn a_new_resolution_on_the_same_monitor_retiles() {
    let before = vec![screen("D1", 1920, 1040, 1.0)];
    assert!(resized_in_place(&before, &[screen("D1", 2560, 1400, 1.0)]));
    assert!(resized_in_place(&before, &[screen("D1", 1920, 1040, 1.5)]), "andere Skalierung");
    assert!(!resized_in_place(&before, &before.clone()), "nichts geändert");
}

#[test]
fn a_monitor_coming_or_going_leaves_the_windows_to_the_user() {
    let one = vec![screen("D1", 1920, 1040, 1.0)];
    let two = vec![screen("D1", 1920, 1040, 1.0), screen("D2", 2560, 1400, 1.25)];
    assert!(!resized_in_place(&two, &one), "Monitor weg (KVM)");
    assert!(!resized_in_place(&one, &two), "Monitor dazu");
    assert!(!resized_in_place(&[], &one), "erster Blick");
}
