//! Automatisches `/compact` im Takt des Decks (Python `test_ui_auto_compact.py`): wer
//! wann `/compact` bekommt. Gegen Attrappen, ohne echte Fenster und Tasten.

use deck_core::protocol::Command;
use deck_core::slot_state::write_state;
use deck_win::{Hwnd, Rect};
use serde_json::json;

use crate::auto_compact::COMPACT;
use crate::tests::{Rig, rig};

const NOW: f64 = 1_000_000.0;

fn compact(slot: &str) -> Command {
    Command::Send { slot: slot.into(), text: COMPACT.into(), execute: true, submit: false }
}

impl Rig {
    /// `live.json`, wie die Statuszeile sie schreibt: Cache läuft in `rest` s ab.
    fn live(&self, slot: &str, rest: f64, age: f64, at: f64) {
        let rec = json!({"cache_until": at + rest, "cache_ttl": 3600, "ctx_tokens": 200_000, "ts": at - age});
        std::fs::write(deck_core::paths::live_path(&self.deck.paths.state_dir, slot), rec.to_string()).unwrap();
    }

    /// Ein VS-Code-Fenster B mit einem wartenden Agenten, dessen Cache gleich abläuft.
    fn waiting_vscode_agent(&mut self) {
        self.vs.open("repo", Some("B"), &["B1"]);
        self.deck.store.bindings.insert("B".into(), "repo".into());
        write_state(&self.deck.paths.state_dir, "B1", "done", NOW - 10.0).unwrap();
        self.live("B1", 120.0, 5.0, NOW);
    }

    fn compacts(&self) -> usize {
        self.vs.sent().iter().filter(|(_, c)| *c == compact("B1")).count()
    }
}

#[test]
fn a_waiting_agent_gets_compact_once_per_cache_state() {
    let mut r = rig();
    r.waiting_vscode_agent();
    r.deck.tick(NOW);
    assert_eq!(r.vs.sent(), vec![("B".to_string(), compact("B1"))]);
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
    r.waiting_vscode_agent();
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
    r.waiting_vscode_agent();
    r.desk.set_foreground(Hwnd(9), "x - repo - Visual Studio Code", "Chrome_WidgetWin_1");
    r.desk.set_input_idle(10.0);
    r.deck.tick(NOW);
    assert_eq!(r.compacts(), 0);
    // Fenster vorn, aber seit Minuten keine Eingabe: du bist weg.
    r.desk.set_input_idle(600.0);
    r.deck.tick(NOW + 1.0);
    assert_eq!(r.compacts(), 1);
}

#[test]
fn not_without_a_running_status_line_and_not_to_a_disconnected_or_remote_window() {
    // Alte live.json: Claude ist zu, /compact landete in der Shell.
    let mut r = rig();
    r.waiting_vscode_agent();
    r.live("B1", 120.0, 600.0, NOW);
    r.deck.tick(NOW);
    assert_eq!(r.compacts(), 0);

    let mut r = rig();
    r.waiting_vscode_agent();
    r.vs.windows.lock().unwrap()[0].remote = Some("ssh-remote".into());
    r.deck.tick(NOW);
    assert_eq!(r.compacts(), 0);
}

#[test]
fn a_terminal_agent_gets_compact_through_its_console_unless_you_sit_at_it() {
    let mut r = rig();
    r.deck.hosts.wt.open_repo("A", "C:/x");
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
