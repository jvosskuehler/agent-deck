//! Verhalten des Decks gegen Attrappen: Takt, Klicks, Windows-Terminal-Host,
//! Einstellungen. Kein Test bewegt ein echtes Fenster, schließt etwas oder sendet
//! Tasten; `forbid_effects()` sperrt zusätzlich jeden echten Eingriff.

use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use deck_core::protocol::Command;
use deck_core::slot_state::{read_all, write_state};
use deck_win::{Hwnd, Rect};
use serde_json::{Value, json};

use crate::app_paths::Paths;
use crate::deck::Deck;
use crate::hosts::Hosts;
use crate::services::Services;
use crate::testing::{FakeConsole, FakeDesktop, FakeLauncher, FakeProcesses, FakeVsCode};
use crate::worker::Worker;
use crate::wt_host::WtHost;
use crate::wt_setup::{WtEffects, WtSetup};

static CLOCK: AtomicU64 = AtomicU64::new(1_000_000);

/// Jede Abfrage eine Sekunde später: der Lebend-Cache des WT-Hosts greift so nie.
fn clock() -> f64 {
    CLOCK.fetch_add(1, Ordering::SeqCst) as f64
}

pub(crate) struct Rig {
    _tmp: tempfile::TempDir,
    pub(crate) deck: Deck,
    pub(crate) vs: Arc<FakeVsCode>,
    pub(crate) desk: Arc<FakeDesktop>,
    procs: Arc<FakeProcesses>,
    pub(crate) console: Arc<FakeConsole>,
    launcher: Arc<FakeLauncher>,
}

pub(crate) fn rig() -> Rig {
    deck_win::forbid_effects();
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths::under(tmp.path());
    fs::create_dir_all(&paths.state_dir).unwrap();
    let (vs, desk) = (Arc::new(FakeVsCode::default()), Arc::new(FakeDesktop::default()));
    let (procs, console, launcher) = (Arc::new(FakeProcesses::default()), Arc::new(FakeConsole::default()), Arc::new(FakeLauncher::default()));
    let setup = WtSetup {
        hosts_file: paths.app_dir.join("wt_hosts.json"),
        state_dir: paths.state_dir.clone(),
        script: paths.wt_agent_script(),
        wt_exe: "wt.exe".into(),
        shell: "pwsh.exe".into(),
        profile: Some("Agent Deck".into()),
    };
    let fx = WtEffects { desktop: desk.clone(), procs: procs.clone(), console: console.clone(), launcher: launcher.clone() };
    let wt = WtHost::new(setup, fx, Worker::inline(), clock);
    let hosts = Hosts { vscode: vs.clone(), wt };
    let services = Services::inert();
    let deck = Deck::new(paths, hosts, desk.clone(), services);
    Rig { _tmp: tmp, deck, vs, desk, procs, console, launcher }
}

impl Rig {
    pub(crate) fn state(&self, slot: &str, v: Value) {
        let path = deck_core::paths::state_path(&self.deck.paths.state_dir, slot);
        fs::write(path, v.to_string()).unwrap();
    }
    fn status(&self, slot: &str) -> Option<String> {
        read_all(&self.deck.paths.state_dir).get(slot).and_then(|s| s.get("status")).and_then(|v| v.as_str()).map(String::from)
    }
    pub(crate) fn wt_agent(&self, slot: &str, pid: u32) {
        let rec = json!({"pid": pid, "created": 133_000_000_000_000_000i64, "slot": slot});
        fs::write(self.deck.paths.state_dir.join(format!("{slot}.wtpid")), rec.to_string()).unwrap();
        self.procs.alive.lock().unwrap().insert(pid, 133_000_000_000_000_000);
    }
}

#[test]
fn a_new_vscode_window_binds_itself_to_the_letter_its_terminals_carry() {
    let mut r = rig();
    r.vs.open("agent-deck", None, &["C1", "C2"]);
    let view = r.deck.tick(100.0);
    assert_eq!(r.deck.store.bindings.get("C").map(String::as_str), Some("agent-deck"));
    let block = view.blocks.iter().find(|b| b.letter == "C").expect("Block C");
    assert_eq!(block.tiles.iter().map(|t| t.slot.as_str()).collect::<Vec<_>>(), ["C1", "C2"]);
    assert!(block.connected);
}

#[test]
fn a_closed_vscode_window_goes_only_after_the_grace_and_while_no_title_shows_it() {
    let mut r = rig();
    r.deck.store.bindings.insert("A".into(), "repo".into());
    r.deck.store.tickets.insert("A1".into(), "X-1".into());
    r.desk.add_title(Hwnd(7), "main.rs - repo - Visual Studio Code");
    r.deck.tick(100.0);
    r.deck.tick(110.0);
    assert!(r.deck.store.bindings.contains_key("A"), "Fenster offen (Reload): bleibt");
    r.desk.clear_titles();
    r.deck.tick(120.0);
    r.deck.tick(122.0);
    assert!(r.deck.store.bindings.contains_key("A"), "noch in der Frist");
    r.deck.tick(123.5);
    assert!(!r.deck.store.bindings.contains_key("A"));
    assert!(!r.deck.store.tickets.contains_key("A1"), "Ticket geht mit");
}

#[test]
fn clicking_an_unread_tile_marks_it_read_and_brings_window_and_pane_forward() {
    let mut r = rig();
    r.vs.open("repo", Some("B"), &["B1"]);
    r.deck.store.bindings.insert("B".into(), "repo".into());
    r.desk.add_title(Hwnd(9), "x - repo - Visual Studio Code");
    write_state(&r.deck.paths.state_dir, "B1", "done", 100.0).unwrap();
    r.deck.focus_slot("B1");
    assert_eq!(r.status("B1").as_deref(), Some("idle"));
    assert_eq!(r.desk.focused(), vec![Hwnd(9)]);
    assert_eq!(r.vs.sent(), vec![("B".to_string(), Command::FocusPane { slot: "B1".into() })]);
    assert_eq!(r.deck.active_slot.as_deref(), Some("B1"));
}

#[test]
fn a_pane_seen_in_vscode_is_read_and_becomes_the_selection() {
    let mut r = rig();
    r.vs.open("repo", Some("B"), &["B1", "B2"]);
    r.deck.store.bindings.insert("B".into(), "repo".into());
    write_state(&r.deck.paths.state_dir, "B2", "done", 100.0).unwrap();
    r.deck.tick(100.0);
    r.vs.seen.lock().unwrap().insert("B2".into());
    let view = r.deck.tick(100.4);
    assert_eq!(r.status("B2").as_deref(), Some("idle"));
    let tile = view.blocks[0].tiles.iter().find(|t| t.slot == "B2").unwrap();
    assert!(tile.selected);
}

#[test]
fn a_new_agent_gets_the_chosen_effort_as_a_start_flag() {
    // Das globale effortLevel zählt für aktuelle Modelle nicht mehr; nur der Schalter wirkt.
    let mut r = rig();
    r.vs.open("repo", Some("C"), &["C1"]);
    r.deck.store.bindings.insert("C".into(), "repo".into());
    r.deck.tick(100.0);
    fs::create_dir_all(r.deck.paths.claude_settings.parent().unwrap()).unwrap();
    fs::write(&r.deck.paths.claude_settings, r#"{"effortLevel": "high"}"#).unwrap();
    r.deck.create_agent("C", 100.0);
    let Command::CreateAgent { args, .. } = r.vs.sent().pop().unwrap().1 else { panic!("kein CreateAgent") };
    assert_eq!(args.as_deref(), Some(r#"--model "opus[1m]" --effort high"#));

    // Ultracode ist xhigh plus Schalter in settings.json; auf der Kommandozeile xhigh.
    fs::write(&r.deck.paths.claude_settings, r#"{"effortLevel": "xhigh", "ultracode": true}"#).unwrap();
    r.deck.create_agent("C", 101.0);
    let Command::CreateAgent { args, .. } = r.vs.sent().pop().unwrap().1 else { panic!("kein CreateAgent") };
    assert_eq!(args.as_deref(), Some(r#"--model "opus[1m]" --effort xhigh"#));
}

#[test]
fn a_new_agent_is_focused_once_it_appears_and_driven_into_auto_mode() {
    let mut r = rig();
    r.vs.open("repo", Some("C"), &["C1"]);
    r.deck.store.bindings.insert("C".into(), "repo".into());
    r.deck.tick(100.0);
    r.deck.create_agent("C", 100.0);
    let create = r.vs.sent().pop().unwrap().1;
    let args = Some(r#"--model "opus[1m]""#.to_string());
    assert_eq!(create, Command::CreateAgent { model: Some("opus[1m]".into()), args, cwd: None, slot: None });
    r.vs.windows.lock().unwrap()[0].slots.push("C2".into());
    r.deck.tick(101.0);
    assert_eq!(r.deck.active_slot.as_deref(), Some("C2"));
    assert!(r.deck.pending_auto.contains_key("C2"));
    // Erster Hook, dann die Warmlaufzeit, dann Shift+Tab vom Startmodus bis „auto“.
    r.state("C2", json!({"slot": "C2", "status": "idle", "ts": 101.5}));
    r.deck.tick(101.6);
    r.deck.tick(103.2);
    let key = r.vs.sent().pop().unwrap().1;
    assert_eq!(key, Command::Key { slot: "C2".into(), key: "shift-tab".into(), repeat: 3 });
    r.state("C2", json!({"slot": "C2", "status": "idle", "ts": 104.0, "mode": "auto"}));
    r.deck.tick(104.1);
    assert!(!r.deck.pending_auto.contains_key("C2"), "im Ziel angekommen");
}

#[test]
fn closing_an_agent_forgets_its_traces_so_a_recycled_slot_name_starts_clean() {
    let mut r = rig();
    r.vs.open("repo", Some("A"), &["A1"]);
    r.deck.store.bindings.insert("A".into(), "repo".into());
    r.deck.store.tickets.insert("A1".into(), "X-1".into());
    r.deck.slot_mode.insert("A1".into(), 2);
    write_state(&r.deck.paths.state_dir, "A1", "waiting", 100.0).unwrap();
    fs::write(r.deck.paths.state_dir.join("A1.worktree"), "C:/r.wt/x-1").unwrap();
    r.deck.active_slot = Some("A1".into());
    r.deck.close_agent("A1");
    assert_eq!(r.vs.sent().last().unwrap().1, Command::CloseAgent { slot: "A1".into() });
    assert_eq!(r.status("A1"), None);
    assert!(!r.deck.store.tickets.contains_key("A1"));
    assert!(!r.deck.slot_mode.contains_key("A1"));
    assert!(!r.deck.paths.state_dir.join("A1.worktree").exists());
    assert_eq!(r.deck.active_slot, None);
}

#[test]
fn the_jump_key_goes_to_the_longest_waiting_and_then_round_robin() {
    let mut r = rig();
    r.vs.open("repo", Some("A"), &["A1", "A2", "A3"]);
    r.deck.store.bindings.insert("A".into(), "repo".into());
    write_state(&r.deck.paths.state_dir, "A1", "done", 90.0).unwrap();
    write_state(&r.deck.paths.state_dir, "A2", "waiting", 95.0).unwrap();
    write_state(&r.deck.paths.state_dir, "A3", "waiting", 80.0).unwrap();
    r.deck.tick(100.0);
    r.deck.jump_to_waiting();
    assert_eq!(r.deck.active_slot.as_deref(), Some("A3"));
    r.deck.jump_to_waiting();
    assert_eq!(r.deck.active_slot.as_deref(), Some("A2"));
}

#[test]
fn assigning_a_ticket_sends_one_submitted_line_and_shows_it_once_the_worktree_exists() {
    let mut r = rig();
    r.vs.open("repo", Some("A"), &["A1"]);
    r.deck.store.bindings.insert("A".into(), "repo".into());
    r.deck.assign_ticket("A1", "2701");
    let (_, cmd) = r.vs.sent().pop().unwrap();
    let Command::Send { text, submit, execute, .. } = cmd else { panic!("kein send") };
    assert!(submit && !execute);
    assert!(!text.contains('\n'));
    assert!(text.contains("PROJ-2701"));
    let view = r.deck.tick(100.0);
    assert_eq!(view.blocks[0].tiles[0].reference, None, "ohne worktree noch nicht gebunden");
    fs::write(r.deck.paths.state_dir.join("A1.worktree"), "C:/repo.wt/2701").unwrap();
    let view = r.deck.tick(100.4);
    assert_eq!(view.blocks[0].tiles[0].reference.as_deref(), Some("2701"));
}

#[test]
fn the_mode_menu_counts_shift_tab_from_the_remembered_mode() {
    let mut r = rig();
    r.vs.open("repo", Some("A"), &["A1"]);
    r.deck.store.bindings.insert("A".into(), "repo".into());
    r.deck.slot_mode.insert("A1".into(), 1);
    r.deck.menu_set_mode("A1", "auto");
    assert_eq!(r.vs.sent().pop().unwrap().1, Command::Key { slot: "A1".into(), key: "shift-tab".into(), repeat: 2 });
    assert_eq!(r.deck.slot_mode.get("A1"), Some(&3));
}

#[test]
fn the_home_block_takes_the_first_free_letter_and_its_folder_name() {
    let mut r = rig();
    let home = r._tmp.path().join("Repos");
    fs::create_dir_all(&home).unwrap();
    r.deck.store.settings.insert("terminal_home".into(), home.to_string_lossy().into_owned().into());
    r.deck.store.bindings.insert("A".into(), "other".into());
    r.deck.ensure_home_terminal();
    assert_eq!(r.deck.home_win.as_deref(), Some("B"));
    assert_eq!(r.deck.store.bindings.get("B").map(String::as_str), Some("Repos"));
    let view = r.deck.tick(100.0);
    assert!(view.blocks[0].home && view.blocks[0].letter == "B", "Heimat-Block steht oben");
    r.deck.ensure_home_terminal();
    assert_eq!(r.deck.home_win.as_deref(), Some("B"), "beim nächsten Start derselbe Buchstabe");
}

#[test]
fn terminal_agents_open_in_their_own_window_and_take_keys_through_the_console() {
    let mut r = rig();
    let folder = r._tmp.path().join("repo");
    fs::create_dir_all(&folder).unwrap();
    r.deck.hosts.wt.open_repo("A", &folder.to_string_lossy());
    r.deck.store.bindings.insert("A".into(), "repo".into());
    r.deck.create_agent("A", 100.0);
    let argv = r.launcher.0.lock().unwrap().last().cloned().unwrap();
    assert_eq!(&argv[..4], ["wt.exe", "-w", "deck-A1", "new-tab"]);
    assert!(argv.contains(&"--suppressApplicationTitle".to_string()));

    r.wt_agent("A1", 4242);
    r.desk.add_wt(Hwnd(31), "A1", Rect::new(0, 0, 800, 600), false);
    let view = r.deck.tick(101.0);
    let block = view.blocks.iter().find(|b| b.letter == "A").unwrap();
    assert_eq!(block.tiles.len(), 1);
    assert_eq!(r.deck.active_slot.as_deref(), Some("A1"), "der neue Agent wird fokussiert");
    assert_eq!(r.desk.focused(), vec![Hwnd(31)]);
    assert!(!r.desk.placed().is_empty(), "neu gekachelt");

    r.deck.set_slot_model("A1", "fable");
    let sent = r.console.0.lock().unwrap().last().cloned().unwrap();
    assert_eq!(sent.0, 4242);
    r.deck.close_agent("A1");
    assert_eq!(*r.procs.killed.lock().unwrap(), vec![4242]);
}

#[test]
fn the_front_key_fetches_all_agents_and_a_second_press_goes_back() {
    let mut r = rig();
    r.deck.hosts.wt.open_repo("D", "C:/x");
    r.deck.store.bindings.insert("D".into(), "x".into());
    r.wt_agent("D1", 71);
    r.wt_agent("D2", 72);
    r.desk.add_wt(Hwnd(31), "D1", Rect::new(0, 0, 800, 600), true);
    r.desk.add_wt(Hwnd(32), "D2", Rect::new(0, 0, 800, 600), false);
    write_state(&r.deck.paths.state_dir, "D2", "waiting", 90.0).unwrap();
    r.deck.tick(100.0);
    r.desk.add_title(Hwnd(500), "Editor");
    r.desk.set_foreground(Hwnd(500), "Editor", "Chrome_WidgetWin_1");
    let before = r.desk.focused().len();
    r.deck.bring_all_front(Some((10, 10)));
    assert_eq!(r.desk.focused()[before..], [Hwnd(31), Hwnd(32)], "der Wartende zuletzt, also oben");
    assert_eq!(r.desk.restored(), [Hwnd(31)]);
    r.desk.set_foreground(Hwnd(32), "D2", deck_core::wt::WINDOW_CLASS);
    r.deck.bring_all_front(Some((10, 10)));
    assert_eq!(r.desk.focused().last(), Some(&Hwnd(500)), "zurück ins Ausgangsfenster");
}

#[test]
fn a_dead_shell_frees_its_slot_and_its_pid_file() {
    let mut r = rig();
    r.deck.hosts.wt.open_repo("D", "C:/x");
    r.wt_agent("D1", 77);
    assert_eq!(r.deck.hosts.terminals("D"), vec!["D1"]);
    r.procs.alive.lock().unwrap().clear();
    assert!(r.deck.hosts.terminals("D").is_empty());
    assert!(!r.deck.paths.state_dir.join("D1.wtpid").exists());
}

#[test]
fn settings_go_to_the_right_file_and_take_effect_at_once() {
    let mut r = rig();
    use crate::settings::Effect;
    assert_eq!(r.deck.set_setting("mode", &json!("plan")), Ok(Effect::None));
    assert_eq!(r.deck.set_setting("effort", &json!("Ultracode")), Ok(Effect::None));
    assert_eq!(r.deck.set_setting("model", &json!("fable")), Ok(Effect::None));
    assert_eq!(r.deck.set_setting("language", &json!("english")), Ok(Effect::Language));
    assert!(r.deck.set_setting("jiraPrefix", &json!("ORPH")).is_err(), "nicht mehr im Fenster");
    let edge = r.deck.set_setting("dockEdge", &json!("bottom")).unwrap();
    assert_eq!(edge, Effect::Edge(deck_core::dock::Edge::Bottom));
    assert!(r.deck.set_setting("nonsense", &json!(1)).is_err());
    let claude: Value = serde_json::from_str(&fs::read_to_string(&r.deck.paths.claude_settings).unwrap()).unwrap();
    assert_eq!(claude["permissions"]["defaultMode"], "plan");
    assert_eq!(claude["effortLevel"], "xhigh");
    assert_eq!(claude["ultracode"], true);
    let v = r.deck.settings_view();
    assert_eq!((v.model.as_str(), v.effort.as_str(), v.language.as_str()), ("fable", "Ultracode", "english"));
    assert_eq!(v.dock_edge.as_str(), "bottom");
    assert!(r.deck.lang.is_english());
}
