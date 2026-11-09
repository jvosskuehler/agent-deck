//! Automatisches `/compact` im Takt des Decks (Python `test_ui_auto_compact.py`): wer
//! wann `/compact` bekommt. Gegen Attrappen, ohne echte Fenster und Tasten.

use deck_core::slot_state::write_state;
use deck_win::{Hwnd, Rect};
use serde_json::json;

use crate::auto_compact::COMPACT;
use crate::tests::{Rig, rig};

const NOW: f64 = 1_000_000.0;

/// Was in der Konsole von B1 (PID 21) ankommt.
const B1: u32 = 21;

impl Rig {
    /// `live.json`, wie die Statuszeile sie schreibt: Cache läuft in `rest` s ab.
    fn live(&self, slot: &str, rest: f64, age: f64, at: f64) {
        let rec = json!({"cache_until": at + rest, "cache_ttl": 3600, "ctx_tokens": 200_000, "ts": at - age});
        std::fs::write(deck_core::paths::live_path(&self.deck.paths.state_dir, slot), rec.to_string()).unwrap();
    }

    /// Ein Block B mit einem wartenden Agenten B1, dessen Cache gleich abläuft.
    fn waiting_agent(&mut self) {
        self.wt_block("B", "repo");
        self.wt_agent("B1", B1);
        self.desk.add_wt(Hwnd(B1 as isize), "B1", Rect::new(0, 0, 800, 600), false);
        write_state(&self.deck.paths.state_dir, "B1", "done", NOW - 10.0).unwrap();
        self.live("B1", 120.0, 5.0, NOW);
    }

    fn compacts(&self) -> usize {
        let want = (B1, format!("{COMPACT}\r"));
        self.typed().into_iter().filter(|t| *t == want).count()
    }
}

#[test]
fn a_waiting_agent_gets_compact_once_per_cache_state() {
    let mut r = rig();
    r.waiting_agent();
    r.deck.tick(NOW);
    assert_eq!(r.typed(), vec![(B1, format!("{COMPACT}\r"))]);
    // Derselbe Cache-Stand: kein zweites Mal, auch wenn das Compact nicht durchging.
    r.deck.tick(NOW + 10.0);
    assert_eq!(r.compacts(), 1);
    // Neuer Stand (du hast geschrieben, der Timer lief neu und wieder fast ab): wieder.
    let later = NOW + 3600.0;
    r.live("B1", 120.0, 5.0, later);
    r.deck.tick(later);
    assert_eq!(r.compacts(), 2);
}

#[test]
fn switched_off_means_nothing_is_sent() {
    let mut r = rig();
    r.waiting_agent();
    r.deck.set_setting("autoCompact", &json!(false)).unwrap();
    assert!(!r.deck.settings_view().auto_compact);
    r.deck.tick(NOW);
    assert_eq!(r.compacts(), 0);
    r.deck.set_setting("autoCompact", &json!(true)).unwrap();
    r.deck.tick(NOW + 1.0);
    assert_eq!(r.compacts(), 1);
}

#[test]
fn not_while_you_type_in_this_window() {
    let mut r = rig();
    r.waiting_agent();
    r.desk.set_foreground(Hwnd(B1 as isize), "B1", deck_win::WT_WINDOW_CLASS);
    r.desk.set_input_idle(10.0);
    r.deck.tick(NOW);
    assert_eq!(r.compacts(), 0);
    // Fenster vorn, aber seit Minuten keine Eingabe: du bist weg.
    r.desk.set_input_idle(600.0);
    r.deck.tick(NOW + 1.0);
    assert_eq!(r.compacts(), 1);
}

#[test]
fn not_without_a_running_status_line_and_not_to_a_forgotten_block() {
    // Alte live.json: Claude ist zu, /compact landete in der Shell.
    let mut r = rig();
    r.waiting_agent();
    r.live("B1", 120.0, 600.0, NOW);
    r.deck.tick(NOW);
    assert_eq!(r.compacts(), 0);

    let mut r = rig();
    r.waiting_agent();
    r.deck.forget_window("B");
    r.deck.tick(NOW);
    assert_eq!(r.compacts(), 0);
}

#[test]
fn a_terminal_agent_gets_compact_through_its_console_unless_you_sit_at_it() {
    let mut r = rig();
    r.deck.wt.open_repo("A", "C:/x");
    r.deck.store.bindings.insert("A".into(), "x".into());
    r.wt_agent("A1", 4242);
    r.desk.add_wt(Hwnd(31), "A1", Rect::new(0, 0, 800, 600), false);
    write_state(&r.deck.paths.state_dir, "A1", "idle", NOW - 10.0).unwrap();
    r.live("A1", 200.0, 5.0, NOW);
    r.desk.set_foreground(Hwnd(31), "A1", deck_win::WT_WINDOW_CLASS);
    r.desk.set_input_idle(3.0);
    r.deck.tick(NOW);
    assert!(r.console.0.lock().unwrap().is_empty(), "du tippst gerade in A1");
    // Ein anderes Fenster vorn: A1 bekommt sein /compact über die Konsole.
    r.desk.set_foreground(Hwnd(32), "A2", deck_win::WT_WINDOW_CLASS);
    r.deck.tick(NOW + 1.0);
    let sent = r.console.0.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, 4242);
}
