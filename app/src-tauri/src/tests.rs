//! Verhalten des Decks gegen Attrappen: Takt, Klicks, Windows-Terminal-Host,
//! Einstellungen. Kein Test bewegt ein echtes Fenster, schließt etwas oder sendet
//! Tasten; `forbid_effects()` sperrt zusätzlich jeden echten Eingriff.

use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use deck_core::slot_state::{read_all, write_state};
use deck_win::console_input::Step;
use deck_win::{Hwnd, Rect};
use serde_json::{Value, json};

use crate::app_paths::Paths;
use crate::deck::Deck;
use crate::services::Services;
use crate::testing::{FakeConsole, FakeDesktop, FakeLauncher, FakeProcesses};
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
    let desk = Arc::new(FakeDesktop::default());
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
    let deck = Deck::new(paths, wt, desk.clone(), Services::inert());
    Rig { _tmp: tmp, deck, desk, procs, console, launcher }
}

impl Rig {
    pub(crate) fn state(&self, slot: &str, v: Value) {
        let path = deck_core::paths::state_path(&self.deck.paths.state_dir, slot);
        fs::write(path, v.to_string()).unwrap();
    }
    fn status(&self, slot: &str) -> Option<String> {
        read_all(&self.deck.paths.state_dir).get(slot).and_then(|s| s.get("status")).and_then(|v| v.as_str()).map(String::from)
    }
    /// Ein Windows-Terminal-Block `win` über dem Repo `repo`, gebunden wie beim Start.
    pub(crate) fn wt_block(&mut self, win: &str, repo: &str) {
        self.deck.wt.open_repo(win, &format!("C:/{repo}"));
        self.deck.store.bindings.insert(win.into(), repo.into());
    }
    pub(crate) fn wt_agent(&self, slot: &str, pid: u32) {
        let rec = json!({"pid": pid, "created": 116_444_736_000_000_000i64, "slot": slot});
        fs::write(self.deck.paths.state_dir.join(format!("{slot}.wtpid")), rec.to_string()).unwrap();
        self.procs.alive.lock().unwrap().insert(pid, 116_444_736_000_000_000);
    }
    /// Was in die Konsolen getippt wurde, je Sendung `(pid, Text ohne Pausen)`.
    pub(crate) fn typed(&self) -> Vec<(u32, String)> {
        let text = |steps: &[Step]| steps.iter().filter_map(|s| if let Step::Text(t) = s { Some(t.as_str()) } else { None }).collect();
        self.console.0.lock().unwrap().iter().map(|(pid, steps)| (*pid, text(steps))).collect()
    }
    /// Die claude-Argumente des zuletzt gestarteten Agenten (aus der `-Spec` des Starts).
    fn last_claude_args(&self) -> Vec<String> {
        let argv = self.launcher.0.lock().unwrap().last().cloned().expect("kein Start");
        let spec = deck_core::wt::decode_spec(argv.last().unwrap()).unwrap();
        serde_json::from_value(spec["args"].clone()).unwrap()
    }
}

#[test]
fn a_binding_without_a_terminal_block_is_dropped_together_with_its_traces() {
    let mut r = rig();
    r.wt_block("B", "x");
    r.deck.store.bindings.insert("A".into(), "alt-aus-vs-code".into());
    r.deck.store.tickets.insert("A1".into(), "X-1".into());
    r.deck.tick(100.0);
    assert!(!r.deck.store.bindings.contains_key("A"));
    assert!(!r.deck.store.tickets.contains_key("A1"), "Ticket geht mit");
    assert!(r.deck.store.bindings.contains_key("B"), "ein Terminal-Block bleibt");
}

#[test]
fn clicking_an_unread_tile_marks_it_read_and_brings_its_window_forward() {
    let mut r = rig();
    r.wt_block("B", "repo");
    r.wt_agent("B1", 41);
    r.desk.add_wt(Hwnd(9), "B1", Rect::new(0, 0, 800, 600), false);
    write_state(&r.deck.paths.state_dir, "B1", "done", 100.0).unwrap();
    r.deck.focus_slot("B1");
    assert_eq!(r.status("B1").as_deref(), Some("idle"));
    assert_eq!(r.desk.focused(), vec![Hwnd(9)]);
    assert_eq!(r.deck.active_slot.as_deref(), Some("B1"));
}

#[test]
fn an_agent_window_brought_forward_by_hand_is_read_and_becomes_the_selection() {
    let mut r = rig();
    r.wt_block("B", "repo");
    r.wt_agent("B1", 41);
    r.wt_agent("B2", 42);
    write_state(&r.deck.paths.state_dir, "B2", "done", 100.0).unwrap();
    r.deck.tick(100.0);
    r.desk.set_foreground(Hwnd(32), "B2", deck_core::wt::WINDOW_CLASS);
    let view = r.deck.tick(100.4);
    assert_eq!(r.status("B2").as_deref(), Some("idle"));
    let tile = view.blocks[0].tiles.iter().find(|t| t.slot == "B2").unwrap();
    assert!(tile.selected);
}

#[test]
fn a_new_agent_gets_the_chosen_effort_as_a_start_flag() {
    // Das globale effortLevel zählt für aktuelle Modelle nicht mehr; nur der Schalter wirkt.
    let mut r = rig();
    r.wt_block("C", "repo");
    fs::create_dir_all(r.deck.paths.claude_settings.parent().unwrap()).unwrap();
    fs::write(&r.deck.paths.claude_settings, r#"{"effortLevel": "high"}"#).unwrap();
    r.deck.create_agent("C", 100.0);
    assert_eq!(r.last_claude_args(), ["--model", "opus[1m]", "--effort", "high"]);

    // Ultracode ist xhigh plus Schalter in settings.json; auf der Kommandozeile xhigh.
    fs::write(&r.deck.paths.claude_settings, r#"{"effortLevel": "xhigh", "ultracode": true}"#).unwrap();
    r.deck.create_agent("C", 101.0);
    assert_eq!(r.last_claude_args(), ["--model", "opus[1m]", "--effort", "xhigh"]);
}

#[test]
fn a_new_agent_is_focused_once_it_appears_and_driven_into_auto_mode() {
    let mut r = rig();
    r.wt_block("C", "repo");
    r.wt_agent("C1", 51);
    r.deck.tick(100.0);
    r.deck.create_agent("C", 100.0);
    assert_eq!(r.last_claude_args(), ["--model", "opus[1m]"]);
    r.wt_agent("C2", 52);
    r.desk.add_wt(Hwnd(52), "C2", Rect::new(0, 0, 800, 600), false);
    r.deck.tick(101.0);
    assert_eq!(r.deck.active_slot.as_deref(), Some("C2"));
    assert!(r.deck.pending_auto.contains_key("C2"));
    // Erster Hook, dann die Warmlaufzeit, dann Shift+Tab vom Startmodus bis „auto“.
    r.state("C2", json!({"slot": "C2", "status": "idle", "ts": 101.5}));
    r.deck.tick(101.6);
    r.deck.tick(103.2);
    assert_eq!(r.typed().pop(), Some((52, "\x1b[Z".repeat(3))));
    r.state("C2", json!({"slot": "C2", "status": "idle", "ts": 104.0, "mode": "auto"}));
    r.deck.tick(104.1);
    assert!(!r.deck.pending_auto.contains_key("C2"), "im Ziel angekommen");
}

#[test]
fn closing_an_agent_forgets_its_traces_so_a_recycled_slot_name_starts_clean() {
    let mut r = rig();
    r.wt_block("A", "repo");
    r.wt_agent("A1", 61);
    r.deck.store.tickets.insert("A1".into(), "X-1".into());
    r.deck.slot_mode.insert("A1".into(), 2);
    write_state(&r.deck.paths.state_dir, "A1", "waiting", 100.0).unwrap();
    fs::write(r.deck.paths.state_dir.join("A1.worktree"), "C:/r.wt/x-1").unwrap();
    r.deck.active_slot = Some("A1".into());
    r.deck.close_agent("A1");
    assert_eq!(*r.procs.killed.lock().unwrap(), vec![61]);
    assert_eq!(r.status("A1"), None);
    assert!(!r.deck.store.tickets.contains_key("A1"));
    assert!(!r.deck.slot_mode.contains_key("A1"));
    assert!(!r.deck.paths.state_dir.join("A1.worktree").exists());
    assert_eq!(r.deck.active_slot, None);
}

#[test]
fn the_new_agent_key_opens_in_the_block_of_the_last_used_agent() {
    let mut r = rig();
    r.wt_block("A", "repo-a");
    r.wt_block("B", "repo-b");
    r.deck.active_slot = Some("B2".into());
    r.deck.create_agent_here(100.0);
    assert_eq!(r.deck.await_new.as_ref().map(|a| a.win.as_str()), Some("B"));

    // Kein zuletzt benutzter Agent: der erste offene Block.
    r.deck.active_slot = None;
    r.deck.await_new = None;
    r.deck.create_agent_here(101.0);
    assert_eq!(r.deck.await_new.as_ref().map(|a| a.win.as_str()), Some("A"));
}

#[test]
fn the_jump_key_goes_to_the_longest_waiting_and_then_round_robin() {
    let mut r = rig();
    r.wt_block("A", "repo");
    for (slot, pid) in [("A1", 71), ("A2", 72), ("A3", 73)] {
        r.wt_agent(slot, pid);
    }
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
fn assigning_a_ticket_sends_one_submitted_line_and_shows_it_at_once() {
    let mut r = rig();
    r.wt_block("A", "repo");
    r.wt_agent("A1", 81);
    r.deck.assign_ticket("A1", "2701");
    let steps = r.console.0.lock().unwrap().last().cloned().unwrap().1;
    let Step::Text(text) = &steps[0] else { panic!("erst der Text") };
    assert!(text.starts_with(deck_core::wt::PASTE_START) && text.contains("PROJ-2701"));
    assert!(!text.contains('\n'));
    assert_eq!(steps.last(), Some(&Step::Text("\r".into())), "abgeschickt per eigenem Enter");
    let view = r.deck.tick(100.0);
    let ticket = |v: &deck_core::view::DeckView| v.blocks[0].tiles[0].ticket.as_ref().map(|t| (t.text.clone(), t.detected));
    assert_eq!(ticket(&view), Some(("PROJ-2701".into(), false)), "zugewiesen gilt sofort, nicht erst mit worktree");
    fs::write(r.deck.paths.state_dir.join("A1.worktree"), "C:/repo.wt/2701").unwrap();
    let view = r.deck.tick(100.4);
    assert_eq!(ticket(&view), Some(("PROJ-2701".into(), false)));
    let tip = crate::tooltip::content(&r.deck, "A1").ticket.unwrap();
    assert_eq!((tip.text.as_str(), tip.origin.as_str()), ("PROJ-2701", "zugewiesen"), "Tooltip wie Kachel");
}

#[test]
fn the_mode_menu_counts_shift_tab_from_the_remembered_mode() {
    let mut r = rig();
    r.wt_block("A", "repo");
    r.wt_agent("A1", 91);
    r.deck.slot_mode.insert("A1".into(), 1);
    r.deck.menu_set_mode("A1", "auto");
    assert_eq!(r.typed().pop(), Some((91, "\x1b[Z".repeat(2))));
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
    r.deck.wt.open_repo("A", &folder.to_string_lossy());
    r.deck.store.bindings.insert("A".into(), "repo".into());
    r.deck.create_agent("A", 100.0);
    let argv = r.launcher.0.lock().unwrap().last().cloned().unwrap();
    // Geht gleich an seinem Platz auf: leerer Block = ganze Arbeitsfläche der Hauptanzeige.
    assert_eq!(&argv[..7], ["wt.exe", "-w", "deck-A1", "--focus", "--pos=1,1", "--size=188,46", "new-tab"]);
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
    assert_eq!(r.typed().pop(), Some((4242, "/model fable\r".into())));
    r.deck.close_agent("A1");
    assert_eq!(*r.procs.killed.lock().unwrap(), vec![4242]);
}

#[test]
fn the_front_key_fetches_all_agents_and_a_second_press_goes_back() {
    let mut r = rig();
    r.wt_block("D", "x");
    r.wt_agent("D1", 71);
    r.wt_agent("D2", 72);
    r.desk.add_wt(Hwnd(31), "D1", Rect::new(0, 0, 800, 600), true);
    r.desk.add_wt(Hwnd(32), "D2", Rect::new(0, 0, 800, 600), false);
    write_state(&r.deck.paths.state_dir, "D2", "waiting", 90.0).unwrap();
    r.deck.tick(100.0);
    r.desk.add_title(Hwnd(500), "Editor");
    r.desk.set_foreground(Hwnd(500), "Editor", "Notepad");
    let before = r.desk.focused().len();
    r.deck.bring_all_front(Some((10, 10)));
    assert_eq!(r.desk.raised().len(), 2, "beide liegen obenauf");
    assert_eq!(r.desk.focused().len(), before, "die Tastatur bleibt im Editor");
    assert_eq!(r.desk.restored(), [Hwnd(31)]);

    // Zweiter Druck, noch im Editor, die Agenten darüber: verstecken.
    r.deck.bring_all_front(Some((10, 10)));
    assert_eq!(r.desk.focused().last(), Some(&Hwnd(500)), "zurück ins Ausgangsfenster");
    assert_eq!(r.desk.minimized(), [Hwnd(31), Hwnd(32)], "und die Agenten sind versteckt");

    // Dritter Druck: wieder holen.
    r.deck.bring_all_front(Some((10, 10)));
    assert_eq!(r.desk.restored().len(), 3, "beide wiederhergestellt");
    assert_eq!(r.desk.raised().len(), 4);

    // Vierter Druck aus einem Agenten heraus (man hat hineingeklickt): auch verstecken.
    r.desk.set_foreground(Hwnd(32), "D2", deck_core::wt::WINDOW_CLASS);
    r.deck.bring_all_front(Some((10, 10)));
    assert_eq!(r.desk.minimized().len(), 4);
}

#[test]
fn with_the_mouse_on_another_monitor_the_key_moves_the_agents_over_in_one_press() {
    let mut r = rig();
    r.desk.add_monitor(Rect::new(1920, 0, 4320, 1200), Rect::new(1920, 0, 4320, 1160), 1.0);
    r.wt_block("D", "x");
    r.wt_agent("D1", 71);
    r.wt_agent("D2", 72);
    r.desk.add_wt(Hwnd(31), "D1", Rect::new(0, 0, 960, 1040), false);
    r.desk.add_wt(Hwnd(32), "D2", Rect::new(960, 0, 1920, 1040), false);
    r.deck.tick(100.0);
    r.desk.add_title(Hwnd(500), "Editor");
    r.desk.set_foreground(Hwnd(500), "Editor", "Notepad");
    r.deck.bring_all_front(Some((10, 10)));
    assert_eq!(r.desk.raised().len(), 2, "erster Druck holt sie nach vorn, links");

    // Die Maus steht rechts: nicht verstecken, sondern herüberziehen.
    r.deck.bring_all_front(Some((2500, 500)));
    assert!(r.desk.minimized().is_empty(), "nichts versteckt");
    let placed = r.desk.placed();
    let moved: Vec<(Hwnd, Rect)> = placed[placed.len().saturating_sub(2)..].to_vec();
    assert_eq!(moved.len(), 2);
    assert!(moved.iter().all(|(_, rc)| rc.left >= 1920), "beide auf dem rechten Monitor: {moved:?}");

    // Liegen sie dort, versteckt der nächste Druck sie wie gewohnt, zurück in den Editor.
    for (h, rc) in moved {
        r.desk.move_wt(h, rc);
    }
    r.deck.bring_all_front(Some((2500, 500)));
    assert_eq!(r.desk.minimized(), [Hwnd(31), Hwnd(32)]);
    assert_eq!(r.desk.focused().last(), Some(&Hwnd(500)), "der Rückweg von vor dem Umzug gilt noch");
}

#[test]
fn whoever_went_back_to_another_window_fetches_instead_of_hiding() {
    let mut r = rig();
    r.wt_block("D", "x");
    r.wt_agent("D1", 71);
    r.desk.add_wt(Hwnd(31), "D1", Rect::new(0, 0, 800, 600), false);
    r.deck.tick(100.0);
    r.desk.add_title(Hwnd(500), "Editor");
    r.desk.set_foreground(Hwnd(500), "Editor", "Notepad");
    r.deck.bring_all_front(Some((10, 10)));
    // Klick in den Editor: er liegt wieder über dem Agenten.
    r.desk.set_foreground(Hwnd(500), "Editor", "Notepad");
    r.deck.bring_all_front(Some((10, 10)));
    assert!(r.desk.minimized().is_empty());
    assert_eq!(r.desk.raised(), [Hwnd(31), Hwnd(31)]);
}

#[test]
fn a_new_agent_does_not_inherit_session_and_prompt_of_its_predecessor_in_the_slot() {
    let mut r = rig();
    r.wt_block("D", "x");
    // D1 wurde am Deck vorbei geschlossen: seine Dateien liegen noch da.
    r.state("D1", json!({"slot": "D1", "status": "done", "ts": 50.0, "session_id": "alt", "prompt": "alte Frage"}));
    let live = r.deck.paths.state_dir.join("D1.live.json");
    fs::write(&live, json!({"session_name": "alter Titel", "ts": 55.0}).to_string()).unwrap();
    // Ein neuer Agent im selben Slot, Shell gestartet bei Unix 1000.
    let created = 116_444_736_000_000_000i64 + 1000 * 10_000_000;
    fs::write(r.deck.paths.state_dir.join("D1.wtpid"), json!({"pid": 81, "created": created, "slot": "D1"}).to_string()).unwrap();
    r.procs.alive.lock().unwrap().insert(81, created as u64);
    r.deck.tick(1001.0);
    assert!(r.deck.state_of("D1").is_none(), "nichts vom Vorgänger");
    assert!(!r.deck.paths.state_dir.join("D1.json").exists() && !live.exists());
    // Der erste Hook der neuen Session bleibt.
    r.state("D1", json!({"slot": "D1", "status": "idle", "ts": 1004.0, "session_id": "neu"}));
    r.deck.tick(1005.0);
    assert_eq!(r.deck.state_of("D1").and_then(|s| s.get("session_id")).and_then(|v| v.as_str()), Some("neu"));
}

#[test]
fn a_dead_shell_frees_its_slot_and_its_pid_file() {
    let mut r = rig();
    r.deck.wt.open_repo("D", "C:/x");
    r.wt_agent("D1", 77);
    assert_eq!(r.deck.wt.terminals("D"), vec!["D1"]);
    r.procs.alive.lock().unwrap().clear();
    assert!(r.deck.wt.terminals("D").is_empty());
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
