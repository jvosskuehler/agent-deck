//! Der Kopf eines Blocks (Fenster schließen, vergessen, per Klick verbinden) und die
//! Herkunftszeilen des Tooltips, gegen Attrappen.

use std::fs;

use deck_core::protocol::Command;
use deck_core::slot_state::write_state;
use deck_win::Hwnd;

use crate::tests::rig;
use crate::tooltip::{content, origin_lines};

#[test]
fn closing_a_window_ends_all_its_agents_and_forgets_their_traces() {
    let mut r = rig();
    r.vs.open("repo", Some("A"), &["A1", "A2"]);
    r.deck.store.bindings.insert("A".into(), "repo".into());
    r.deck.store.tickets.insert("A2".into(), "X-2".into());
    r.deck.slot_mode.insert("A2".into(), 1);
    write_state(&r.deck.paths.state_dir, "A2", "waiting", 100.0).unwrap();
    fs::write(r.deck.paths.state_dir.join("A2.worktree"), "C:/r.wt/x-2").unwrap();
    r.deck.active_slot = Some("A1".into());
    r.deck.close_window("A");
    assert_eq!(r.vs.sent(), vec![("A".to_string(), Command::CloseWindow)]);
    assert!(!r.deck.store.tickets.contains_key("A2"));
    assert!(!r.deck.slot_mode.contains_key("A2"));
    assert!(!r.deck.paths.state_dir.join("A2.worktree").exists());
    assert_eq!(r.deck.active_slot, None);
    assert!(r.deck.store.bindings.contains_key("A"), "die Bindung räumt erst das Abräumen nach der Frist");
}

#[test]
fn a_disconnected_window_cannot_be_closed_from_the_deck() {
    let mut r = rig();
    r.deck.store.bindings.insert("B".into(), "repo".into());
    r.deck.close_window("B");
    assert!(r.vs.sent().is_empty());
}

#[test]
fn connecting_by_click_binds_the_vscode_window_that_comes_to_the_front() {
    let mut r = rig();
    r.vs.open("other", None, &[]);
    r.deck.store.bindings.insert("C".into(), "other".into());
    r.deck.start_bind("B", 100.0);
    r.desk.set_foreground(Hwnd(5), "notes.txt - Editor", "Notepad");
    r.deck.poll_bind(101.0);
    assert!(r.deck.binding("B"), "kein VS Code vorn: wartet weiter");
    r.desk.set_foreground(Hwnd(6), "main.rs - other - Visual Studio Code", "Chrome_WidgetWin_1");
    r.deck.poll_bind(102.0);
    assert!(!r.deck.binding("B"));
    assert_eq!(r.deck.store.bindings.get("B").map(String::as_str), Some("other"));
    assert!(!r.deck.store.bindings.contains_key("C"), "ein Repo hängt nie an zwei Buchstaben");
    let view = r.deck.view(103.0);
    assert!(view.blocks.iter().all(|b| !b.binding));
}

#[test]
fn connecting_shows_on_the_block_gives_up_after_twenty_seconds_and_a_second_click_cancels() {
    let mut r = rig();
    r.deck.store.bindings.insert("B".into(), "repo".into());
    r.deck.start_bind("B", 100.0);
    let view = r.deck.view(100.0);
    assert!(view.blocks.iter().find(|b| b.letter == "B").is_some_and(|b| b.binding));
    r.deck.poll_bind(121.0);
    assert!(!r.deck.binding("B"), "Frist abgelaufen");
    r.deck.start_bind("B", 130.0);
    r.deck.start_bind("B", 131.0);
    assert!(!r.deck.binding("B"), "zweiter Aufruf bricht ab");
}

#[test]
fn a_window_without_a_folder_is_not_bound() {
    let mut r = rig();
    r.deck.start_bind("B", 100.0);
    r.desk.set_foreground(Hwnd(6), "Visual Studio Code", "Chrome_WidgetWin_1");
    r.deck.poll_bind(101.0);
    assert!(!r.deck.store.bindings.contains_key("B"));
    assert!(!r.deck.binding("B"));
}

#[test]
fn forgetting_a_window_drops_its_binding_and_a_pending_connect() {
    let mut r = rig();
    r.deck.store.bindings.insert("B".into(), "repo".into());
    r.deck.start_bind("B", 100.0);
    r.deck.forget_window("B");
    assert!(!r.deck.store.bindings.contains_key("B"));
    assert!(!r.deck.binding("B"));
}

#[test]
fn the_tooltip_says_where_the_agent_works() {
    let mut r = rig();
    r.deck.store.bindings.insert("A".into(), "agent-deck".into());
    assert_eq!(origin_lines(&r.deck, "A2"), ["agent-deck · Fenster A · A2"]);
    r.deck.worktrees.insert("A2".into(), "C:\\Repos\\agent-deck.wt\\orph-4062\\".into());
    assert_eq!(origin_lines(&r.deck, "A2"), ["agent-deck · Fenster A · A2", "↳ wt/orph-4062"]);
    assert_eq!(origin_lines(&r.deck, "C1"), ["Fenster C · C1"], "ohne Bindung bleibt der Buchstabe");
    assert_eq!(content(&r.deck, "A2").origin.len(), 2);
}
