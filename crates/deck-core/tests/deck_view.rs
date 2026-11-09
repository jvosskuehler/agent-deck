//! Das fertige Anzeigemodell aus Zuständen, Bindungen und Reihenfolge (Python
//! `ui/refresh.py`, `ui/tiles.py`; Griff-Farbe aus `test_ui_refresh.py`).

use std::collections::HashMap;

use deck_core::bind_store::BindStore;
use deck_core::claude::refs::Refs;
use deck_core::deck_view::*;
use deck_core::i18n::Lang;
use deck_core::slot_state::SlotState;
use deck_core::view::{Host, Status};
use serde_json::{Value, json};

const NOW: f64 = 10_000.0;

fn map(v: Value) -> HashMap<String, SlotState> {
    v.as_object().unwrap().iter().map(|(k, x)| (k.clone(), x.as_object().unwrap().clone())).collect()
}

fn win(letter: &str, host: Host, connected: bool, slots: &[&str]) -> WindowInfo {
    WindowInfo { letter: letter.into(), host, connected, remote: false, terminals: slots.iter().map(|s| s.to_string()).collect(), screen: Vec::new(), screen_cols: 0 }
}

struct Fixture {
    windows: Vec<WindowInfo>,
    store: BindStore,
    states: HashMap<String, SlotState>,
    live: HashMap<String, SlotState>,
    found: HashMap<String, String>,
    worktrees: HashMap<String, String>,
    refs: HashMap<String, Refs>,
    slot_mode: HashMap<String, usize>,
    cycle: Vec<String>,
    home: Option<String>,
    active: Option<String>,
}

impl Fixture {
    fn new() -> Self {
        let mut store = BindStore::default();
        store.bindings.insert("A".into(), "agent-deck".into());
        store.bindings.insert("B".into(), "Repos".into());
        Fixture {
            windows: vec![win("A", Host::Vscode, true, &["A1", "A2"]), win("B", Host::Terminal, true, &["B1"])],
            store,
            states: HashMap::new(),
            live: HashMap::new(),
            found: HashMap::new(),
            worktrees: HashMap::new(),
            refs: HashMap::new(),
            slot_mode: HashMap::new(),
            cycle: ["manual", "accept", "plan", "auto"].map(String::from).to_vec(),
            home: Some("B".into()),
            active: None,
        }
    }

    fn view(&self, mem: &mut StatusMemory) -> deck_core::view::DeckView {
        let input = DeckInput {
            windows: &self.windows,
            store: &self.store,
            home: self.home.as_deref(),
            states: &self.states,
            live: &self.live,
            found_tickets: &self.found,
            worktrees: &self.worktrees,
            detected_refs: &self.refs,
            slot_mode: &self.slot_mode,
            mode_cycle: &self.cycle,
            active_slot: self.active.as_deref(),
            now: NOW,
            lang: Lang::German,
            usage: None,
        };
        build_deck_view(&input, mem)
    }

    fn tile(&self, slot: &str) -> deck_core::view::TileView {
        let v = self.view(&mut StatusMemory::default());
        v.blocks.into_iter().flat_map(|b| b.tiles).find(|t| t.slot == slot).expect(slot)
    }
}

#[test]
fn the_home_block_stands_on_top_and_dragged_block_order_follows_repo_names() {
    let mut f = Fixture::new();
    f.store.bindings.insert("C".into(), "api".into());
    f.windows.push(win("C", Host::Vscode, false, &[]));
    f.store.win_order = vec!["api".into(), "agent-deck".into()];
    let v = f.view(&mut StatusMemory::default());
    assert_eq!(v.blocks.iter().map(|b| b.letter.as_str()).collect::<Vec<_>>(), ["B", "C", "A"]);
    assert!(v.blocks[0].home && v.blocks[0].host == Host::Terminal);
    assert!(!v.blocks[1].connected && v.blocks[1].tiles.is_empty(), "getrennt: keine Kacheln");
}

#[test]
fn only_bound_or_connected_windows_are_shown_and_unbound_ones_are_named_by_letter() {
    let mut f = Fixture::new();
    f.windows.push(win("D", Host::Vscode, true, &["D1"]));
    f.windows.push(win("E", Host::Vscode, false, &[]));
    let v = f.view(&mut StatusMemory::default());
    assert_eq!(v.blocks.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(), ["Repos", "agent-deck", "Fenster D"]);
}

#[test]
fn the_dragged_tile_order_wins_and_new_slots_append() {
    let mut f = Fixture::new();
    f.windows[0].terminals = vec!["A1".into(), "A2".into(), "A3".into()];
    f.store.order.insert("A".into(), vec!["A3".into(), "A1".into()]);
    let v = f.view(&mut StatusMemory::default());
    let a = v.blocks.iter().find(|b| b.letter == "A").unwrap();
    assert_eq!(a.tiles.iter().map(|t| t.slot.as_str()).collect::<Vec<_>>(), ["A3", "A1", "A2"]);
}

#[test]
fn terminal_tiles_lie_like_the_windows_on_screen_and_minimized_ones_follow() {
    let mut f = Fixture::new();
    let slots = ["B1", "B2", "B3", "B4", "B5", "B6", "B7"];
    f.windows[1].terminals = slots.map(String::from).to_vec();
    // Gezogen war eine andere Reihenfolge: die Lage auf dem Bildschirm gewinnt.
    f.store.order.insert("B".into(), slots.iter().rev().map(|s| s.to_string()).collect());
    f.windows[1].screen = vec![
        vec!["B3".into(), "B1".into(), "B5".into()],
        vec!["B2".into(), "gone".into(), "B6".into(), "B4".into()],
    ];
    let v = f.view(&mut StatusMemory::default());
    let b = v.blocks.iter().find(|b| b.letter == "B").unwrap();
    let order: Vec<&str> = b.tiles.iter().map(|t| t.slot.as_str()).collect();
    assert_eq!(order, ["B3", "B1", "B5", "B2", "B6", "B4", "B7"], "B7 minimiert: hinten");
    assert_eq!(b.rows, [3, 3]);
    let a = v.blocks.iter().find(|b| b.letter == "A").unwrap();
    assert!(a.rows.is_empty(), "VS Code: kein Bildschirmraster");
}

#[test]
fn a_tile_without_report_is_idle_locally_and_unknown_remote() {
    let mut f = Fixture::new();
    assert_eq!(f.tile("A1").status, Status::Idle);
    f.windows[0].remote = true;
    let t = f.tile("A1");
    assert_eq!((t.status, t.status_label.as_str()), (Status::Unknown, "unbekannt"));
}

#[test]
fn thinking_falls_asleep_without_fresh_report_and_running_shows_as_thinking() {
    let mut f = Fixture::new();
    f.states = map(json!({"A1": {"status": "thinking", "ts": NOW - 901.0}, "A2": {"status": "running", "ts": NOW}}));
    assert_eq!(f.tile("A1").status, Status::Idle);
    let t = f.tile("A2");
    assert_eq!((t.status, t.status_label.as_str()), (Status::Thinking, "denkt"));
}

#[test]
fn card_fields_come_from_live_values_state_and_remembered_effort() {
    let mut f = Fixture::new();
    f.states = map(json!({"A1": {"status": "waiting", "ts": NOW, "effort": "high", "mode": "plan"},
                          "B1": {"status": "done", "ts": NOW, "mode": "auto"}}));
    f.live = map(json!({"A1": {"model": "Opus 5 (1M context)", "effort": "xhigh", "session_name": "nicht für VS Code"},
                        "B1": {"model": "Fable", "session_name": "Schrift-Einstellung"}}));
    f.store.effort.insert("A1".into(), "ultracode".into());
    f.slot_mode.insert("B1".into(), 0);
    let a = f.tile("A1");
    assert_eq!((a.model.as_str(), a.effort.as_str(), a.mode.as_str()), ("Opus 5 (1M)", "ultracode", "plan"));
    assert_eq!((a.status_label.as_str(), a.title.as_deref()), ("Rückfrage", None));
    let b = f.tile("B1");
    assert_eq!((b.mode.as_str(), b.title.as_deref(), b.effort.as_str()), ("manual", Some("Schrift-Einstellung"), ""));
    assert_eq!(f.tile("A2").model, "", "unbekanntes Modell bleibt leer");
}

#[test]
fn an_assigned_ticket_shows_only_with_worktree_otherwise_the_detected_one_dimmed() {
    let mut f = Fixture::new();
    f.states = map(json!({"A1": {"status": "idle", "ts": NOW, "session_id": "s1"}}));
    f.store.tickets.insert("A1".into(), "ORPH-4116".into());
    f.refs.insert("s1".into(), Refs { ticket: "ORPH-1".into(), pr: "62".into() });
    let t = f.tile("A1");
    assert_eq!((t.reference.as_deref(), t.reference_detected), (Some("ORPH-1 #62"), true));
    f.worktrees.insert("A1".into(), "C:/wt".into());
    let t = f.tile("A1");
    assert_eq!((t.reference.as_deref(), t.reference_detected), (Some("ORPH-4116"), false));
    f.store.tickets.clear();
    f.found.insert("A1".into(), "SEHR-LANGES-TICKET-1".into());
    assert_eq!(f.tile("A1").reference.as_deref(), Some("SEHR-LANGES-T…"));
    assert_eq!(f.tile("A2").reference, None);
}

#[test]
fn the_active_tile_is_selected() {
    let mut f = Fixture::new();
    f.active = Some("A2".into());
    assert!(f.tile("A2").selected && !f.tile("A1").selected);
}

#[test]
fn the_handle_glows_in_the_most_urgent_status_and_stays_calm_when_idle() {
    let mut f = Fixture::new();
    assert_eq!(f.view(&mut StatusMemory::default()).urgent, None);
    f.states = map(json!({"A1": {"status": "done", "ts": NOW}, "B1": {"status": "waiting", "ts": NOW}}));
    assert_eq!(f.view(&mut StatusMemory::default()).urgent, Some(Status::Waiting));
    f.states = map(json!({"A1": {"status": "running", "ts": NOW}}));
    assert_eq!(f.view(&mut StatusMemory::default()).urgent, Some(Status::Thinking));
}

#[test]
fn the_status_change_time_moves_only_when_the_status_changes() {
    let mut f = Fixture::new();
    let mut mem = StatusMemory::default();
    f.states = map(json!({"A1": {"status": "thinking", "ts": NOW}}));
    let first = f.view(&mut mem).blocks[1].tiles[0].status_changed_at;
    assert_eq!(first, NOW);
    assert_eq!(mem.observe("A1", Status::Thinking, NOW + 5.0), NOW);
    assert_eq!(mem.observe("A1", Status::Done, NOW + 6.0), NOW + 6.0);
}

#[test]
fn the_view_carries_language_code() {
    let f = Fixture::new();
    assert_eq!(f.view(&mut StatusMemory::default()).lang, "de");
    assert_eq!(short_model("Opus 5 (1M context)"), "Opus 5 (1M)");
    assert_eq!(to_status("running"), Status::Thinking);
}
