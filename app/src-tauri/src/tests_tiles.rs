//! Kacheln der Agentenfenster gegen Attrappen: Nachlegen nach einem Wechsel im Raster.

use deck_win::{Hwnd, Rect};

use crate::tests::rig;

#[test]
fn a_new_window_is_placed_again_after_it_settled_and_then_left_alone() {
    let mut r = rig();
    r.wt_block("A", "repo");
    r.wt_agent("A1", 4242);
    r.desk.add_wt(Hwnd(31), "A1", Rect::new(0, 0, 800, 600), false);
    let placed = |r: &crate::tests::Rig| r.desk.placed().iter().filter(|(h, _)| *h == Hwnd(31)).count();
    r.deck.wt.terminals("A");
    let first = placed(&r);
    assert!(first > 0, "gleich beim Auftauchen gekachelt");
    let target = r.desk.placed().last().unwrap().1;
    // Jede Abfrage ist eine Sekunde später (Uhr des Rigs): die Nachlege-Durchgänge kommen.
    for _ in 0..6 {
        r.deck.wt.terminals("A");
    }
    let settled = placed(&r);
    // Die Uhr teilen sich alle Tests: laufen andere parallel, springt sie weiter, und
    // beide Durchgänge können im selben Blick fällig werden.
    assert!((first + 1..=first + 2).contains(&settled), "nachgelegt: {first} → {settled}");
    assert!(r.desk.placed().iter().filter(|(h, _)| *h == Hwnd(31)).all(|(_, t)| *t == target), "immer dieselbe Kachel");
    for _ in 0..10 {
        r.deck.wt.terminals("A");
    }
    assert_eq!(placed(&r), settled, "danach Ruhe");
}

#[test]
fn a_new_resolution_retiles_the_block_on_the_same_monitor() {
    let mut r = rig();
    r.wt_block("A", "repo");
    r.wt_agent("A1", 4242);
    r.wt_agent("A2", 4243);
    r.desk.add_wt(Hwnd(31), "A1", Rect::new(1, 1, 959, 1039), false);
    r.desk.add_wt(Hwnd(32), "A2", Rect::new(961, 1, 1919, 1039), false);
    for _ in 0..8 {
        r.deck.wt.terminals("A");
    }
    let before = r.desk.placed().len();
    r.desk.resize_primary(Rect::new(0, 0, 2560, 1440), Rect::new(0, 0, 2560, 1400));
    r.deck.wt.terminals("A");
    let placed = r.desk.placed();
    assert!(placed.len() > before, "nachgekachelt");
    let a2 = placed.iter().rev().find(|(h, _)| *h == Hwnd(32)).unwrap().1;
    assert_eq!(a2.right, 2559, "die zweite Kachel reicht bis an den neuen Rand (Ring frei)");
}
