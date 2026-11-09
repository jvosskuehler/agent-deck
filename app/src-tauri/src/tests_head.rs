//! Der Kopf eines Blocks (vergessen, Repo-Name holt die Fenster) und die
//! Herkunftszeilen des Tooltips, gegen Attrappen.

use deck_win::{Hwnd, Rect};

use crate::tests::rig;
use crate::tooltip::{content, origin_lines};

#[test]
fn forgetting_a_block_drops_binding_and_terminal_entry_but_closes_nothing() {
    let mut r = rig();
    r.wt_block("B", "repo");
    r.wt_agent("B1", 11);
    r.deck.forget_window("B");
    assert!(!r.deck.store.bindings.contains_key("B"));
    assert!(!r.deck.wt.has("B"));
    let view = r.deck.tick(100.0);
    assert!(view.blocks.is_empty());
    assert!(r.console.0.lock().unwrap().is_empty(), "kein Agent wird angefasst");
}

#[test]
fn clicking_the_repo_name_raises_all_windows_of_the_block_first_agent_on_top() {
    let mut r = rig();
    r.wt_block("B", "repo");
    r.wt_agent("B1", 11);
    r.wt_agent("B2", 12);
    r.desk.add_wt(Hwnd(21), "B1", Rect::new(0, 0, 800, 600), false);
    r.desk.add_wt(Hwnd(22), "B2", Rect::new(800, 0, 1600, 600), false);
    let before = r.desk.focused().len();
    r.deck.show_window("B");
    assert_eq!(r.desk.focused()[before..], [Hwnd(22), Hwnd(21)]);
    assert_eq!(r.deck.active_slot, None, "der Kopf wählt keinen Agenten aus");
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
