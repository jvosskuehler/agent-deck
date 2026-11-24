//! Reserve-Agent (`wt_reserve.rs`, `reserve_agent.rs`) gegen Attrappen: kein Test öffnet
//! ein echtes Fenster. Eingeschaltet wird er nur über eine Erweiterung; hier eine, die
//! den Testschlüssel `reserve_on` liest (fehlt er, an).

use deck_win::{Hwnd, Rect};
use serde_json::{Value, json};

use crate::extension::{DeckExtension, Extensions, Settings};
use crate::tests::{Rig, rig};

const T0: f64 = 10_000.0;
const WINDOW: Hwnd = Hwnd(70);
const SHELL: u32 = 71;
const BEFORE: Hwnd = Hwnd(5);

struct WantsReserve;

impl DeckExtension for WantsReserve {
    fn name(&self) -> &'static str {
        "reserve"
    }
    fn reserve_agent(&self, settings: &Settings) -> bool {
        settings.get("reserve_on") != Some(&Value::Bool(false))
    }
}

fn launches(r: &Rig) -> Vec<Vec<String>> {
    r.launcher.0.lock().unwrap().clone()
}

/// Block B, Reserve an, seit zwei Minuten keine Eingabe, Notepad vorn; das Fenster, das
/// Windows Terminal gleich öffnet, gibt es schon (die Attrappe startet nichts).
fn quiet_desk() -> Rig {
    let mut r = rig();
    r.deck.ext = Extensions::new(vec![Box::new(WantsReserve)]);
    r.wt_block("B", "Repos");
    r.desk.set_input_idle(120.0);
    r.desk.set_foreground(BEFORE, "Notepad", "Notepad");
    r.desk.add_wt(WINDOW, "B1", Rect::new(0, 0, 800, 600), false);
    r
}

/// Reserve angelegt, Shell lebt, Ergebnis abgeholt.
fn ready_reserve() -> Rig {
    let mut r = quiet_desk();
    r.deck.tick(T0);
    r.wt_agent("B1", SHELL);
    r.deck.tick(T0 + 3.0);
    assert_eq!(r.deck.wt.reserve_slot().as_deref(), Some("B1"));
    r
}

#[test]
fn without_an_extension_asking_for_it_there_is_never_a_reserve() {
    let mut r = quiet_desk();
    r.deck.ext = Extensions::default();
    r.deck.tick(T0);
    r.deck.tick(T0 + 3.0);
    assert!(launches(&r).is_empty());
    assert_eq!(r.deck.wt.reserve_slot(), None);
}

#[test]
fn after_a_quiet_minute_a_reserve_opens_hides_and_hands_the_keyboard_back() {
    let mut r = quiet_desk();
    r.deck.tick(T0);
    let l = launches(&r);
    assert_eq!(l.len(), 1);
    assert!(l[0].contains(&"deck-B1".to_string()), "{:?}", l[0]);
    assert_eq!(r.desk.visibility(), vec![(WINDOW, false)], "gleich versteckt");
    assert_eq!(r.desk.focused(), vec![BEFORE], "Fokus zurück an Notepad");
}

#[test]
fn no_reserve_opens_after_a_short_pause_or_when_switched_off() {
    let mut r = quiet_desk();
    r.desk.set_input_idle(20.0);
    r.deck.tick(T0);
    assert!(launches(&r).is_empty(), "20 s Pause ist Nachdenken, keine Ruhe");
    r.desk.set_input_idle(120.0);
    r.deck.store.settings.insert("reserve_on".into(), Value::Bool(false));
    r.deck.tick(T0 + 3.0);
    assert!(launches(&r).is_empty());
}

#[test]
fn no_reserve_opens_while_an_agent_or_terminal_is_in_front() {
    let mut r = quiet_desk();
    r.desk.set_foreground(Hwnd(9), "B7", deck_core::wt::WINDOW_CLASS);
    r.deck.tick(T0);
    r.desk.set_foreground(Hwnd(8), "Eingabeaufforderung", "ConsoleWindowClass");
    r.deck.tick(T0 + 3.0);
    assert!(launches(&r).is_empty());
    r.desk.set_foreground(BEFORE, "Notepad", "Notepad");
    r.deck.tick(T0 + 6.0);
    assert_eq!(launches(&r).len(), 1, "erst mit einem anderen Fenster vorn");
}

#[test]
fn input_while_the_reserve_was_in_front_spoils_it_and_it_is_never_handed_over() {
    let mut r = quiet_desk();
    r.desk.type_while_hiding();
    r.deck.tick(T0);
    r.deck.tick(T0 + 3.0);
    assert_eq!(r.deck.wt.reserve_slot(), None, "verworfen");
    // Ihre Shell kommt erst jetzt: sie wird beendet, nicht als Agent gezeigt.
    r.wt_agent("B1", SHELL);
    r.deck.tick(T0 + 6.0);
    assert_eq!(*r.procs.killed.lock().unwrap(), vec![SHELL]);
    r.deck.create_agent("B", T0 + 7.0);
    assert_eq!(launches(&r).len(), 2, "der neue Agent startet kalt");
    assert_eq!(r.desk.visibility(), vec![(WINDOW, false)], "die verdorbene wird nie gezeigt");
}

#[test]
fn the_waiting_reserve_is_no_tile_and_no_live_agent() {
    let mut r = ready_reserve();
    assert!(r.deck.wt.terminals("B").is_empty());
    let view = r.deck.tick(T0 + 6.0);
    let tiles: Vec<String> = view.blocks.iter().flat_map(|b| b.tiles.iter().map(|t| t.slot.clone())).collect();
    assert!(!tiles.contains(&"B1".to_string()), "{tiles:?}");
}

#[test]
fn a_new_agent_takes_over_the_reserve_without_starting_a_terminal() {
    let mut r = ready_reserve();
    r.desk.set_input_idle(0.0);
    r.deck.create_agent("B", T0 + 4.0);
    assert_eq!(launches(&r).len(), 1, "kein zweites wt.exe");
    assert_eq!(r.desk.visibility().last(), Some(&(WINDOW, true)), "gezeigt");
    assert_eq!(r.desk.focused().last(), Some(&WINDOW), "und vorn");
    assert_eq!(r.deck.wt.reserve_slot(), None);
    assert_eq!(r.deck.wt.terminals("B"), vec!["B1".to_string()], "jetzt ein gewöhnlicher Agent");
}

#[test]
fn after_a_take_over_the_next_reserve_waits_for_the_next_quiet_minute() {
    let mut r = ready_reserve();
    r.desk.set_input_idle(0.0);
    r.deck.create_agent("B", T0 + 4.0);
    r.deck.tick(T0 + 7.0);
    assert_eq!(launches(&r).len(), 1);
    r.desk.set_input_idle(60.0);
    r.desk.add_wt(Hwnd(80), "B2", Rect::new(0, 0, 800, 600), false);
    r.deck.tick(T0 + 10.0);
    let l = launches(&r);
    assert_eq!(l.len(), 2);
    assert!(l[1].contains(&"deck-B2".to_string()), "neuer Name, B1 lebt: {:?}", l[1]);
}

#[test]
fn a_reserve_that_is_not_ready_yet_is_skipped_and_its_name_stays_taken() {
    let mut r = quiet_desk();
    r.deck.tick(T0);
    r.deck.create_agent("B", T0 + 0.5);
    let l = launches(&r);
    assert_eq!(l.len(), 2, "kalt gestartet");
    assert!(l[1].contains(&"deck-B2".to_string()), "{:?}", l[1]);
}

#[test]
fn changed_settings_retire_the_reserve_and_its_shell() {
    let mut r = ready_reserve();
    r.deck.store.settings.insert("model".into(), json!("fable"));
    r.desk.set_input_idle(0.0);
    r.deck.tick(T0 + 6.0);
    assert_eq!(r.deck.wt.reserve_slot(), None);
    assert_eq!(*r.procs.killed.lock().unwrap(), vec![SHELL]);
}

#[test]
fn a_dead_reserve_is_no_crash_and_is_simply_replaced() {
    let mut r = ready_reserve();
    r.procs.alive.lock().unwrap().clear();
    r.deck.tick(T0 + 6.0);
    assert_eq!(r.deck.wt.reserve_slot().as_deref(), Some("B1"), "erst nach der Startfrist als tot");
    r.deck.tick(T0 + 40.0);
    assert!(r.deck.resumes.is_empty(), "kein Fortsetzen");
    assert_eq!(r.deck.wt.reserve_slot(), None);
    r.deck.tick(T0 + 43.0);
    assert_eq!(launches(&r).len(), 2, "neue Reserve bei Ruhe");
}

#[test]
fn a_shell_that_shows_up_after_its_reserve_was_retired_is_ended_not_shown() {
    let mut r = quiet_desk();
    r.deck.tick(T0);
    r.deck.store.settings.insert("model".into(), json!("fable"));
    r.desk.set_input_idle(0.0);
    r.deck.tick(T0 + 3.0);
    assert_eq!(r.deck.wt.reserve_slot(), None, "abgeräumt, bevor die Shell lebte");
    r.wt_agent("B1", SHELL);
    r.deck.tick(T0 + 6.0);
    assert_eq!(*r.procs.killed.lock().unwrap(), vec![SHELL], "verspätete Shell beendet");
    assert!(r.deck.wt.terminals("B").is_empty(), "keine Geister-Kachel");
}

#[test]
fn the_name_of_a_retired_reserve_without_shell_is_not_reused() {
    let mut r = quiet_desk();
    r.deck.tick(T0);
    r.deck.store.settings.insert("model".into(), json!("fable"));
    r.deck.tick(T0 + 3.0);
    r.desk.add_wt(Hwnd(81), "B2", Rect::new(0, 0, 800, 600), false);
    r.deck.tick(T0 + 6.0);
    let l = launches(&r);
    assert_eq!(l.len(), 2);
    assert!(l[1].contains(&"deck-B2".to_string()), "B1 kann noch kommen: {:?}", l[1]);
}

#[test]
fn a_reserve_whose_window_never_shows_up_pauses_further_attempts() {
    let mut r = quiet_desk();
    r.desk.remove_wt(WINDOW);
    r.deck.tick(T0);
    r.deck.tick(T0 + 3.0);
    assert_eq!(r.deck.wt.reserve_slot(), None, "abgeräumt");
    r.deck.tick(T0 + 60.0);
    assert_eq!(launches(&r).len(), 1, "erst nach der Pause wieder");
}

#[test]
fn a_pause_retires_the_reserve_and_keeps_new_ones_away_for_a_while() {
    let mut r = ready_reserve();
    assert!(r.deck.wt.pause_reserve("Wartung", T0 + 4.0, 600.0));
    assert_eq!(*r.procs.killed.lock().unwrap(), vec![SHELL]);
    r.deck.tick(T0 + 10.0);
    assert_eq!(launches(&r).len(), 1, "in der Pause keine neue");
    r.desk.add_wt(Hwnd(82), "B2", Rect::new(0, 0, 800, 600), false);
    r.deck.tick(T0 + 700.0);
    assert_eq!(launches(&r).len(), 2, "danach wieder");
}

/// Agenten in WSL: die Reserve startet wie jeder Agent über `deck-wsl.exe` in der Distro,
/// und ihr Rezept nennt die Distro (eine Reserve aus der Windows-Zeit passt dann nicht).
#[test]
fn with_agents_in_wsl_the_reserve_starts_in_the_distro() {
    use std::sync::Arc;

    use crate::testing::{FakeConsole, FakeDesktop, FakeLauncher, FakeProcesses};
    use crate::worker::Worker;
    use crate::wt_host::WtHost;
    use crate::wt_setup::{WslLaunch, WtEffects, WtSetup};

    deck_win::forbid_effects();
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path().join("state");
    std::fs::create_dir_all(&state).unwrap();
    let wsl = deck_core::wsl::Wsl { distro: "Ubuntu".into(), home: "/home/dev".into(), mount_root: "/mnt/".into() };
    let setup = WtSetup {
        hosts_file: tmp.path().join("wt_hosts.json"),
        state_dir: state,
        script: tmp.path().join("wt_agent.ps1"),
        wt_exe: "wt.exe".into(),
        shell: "pwsh.exe".into(),
        profile: None,
        wsl: Some(WslLaunch { wsl, launcher: r"C:\deck\deck-wsl.exe".into(), script: "/mnt/c/deck/wt_agent.sh".into(), window_dir: r"C:\Users\dev".into() }),
    };
    let desk = Arc::new(FakeDesktop::default());
    let launcher = Arc::new(FakeLauncher::default());
    let fx = WtEffects { desktop: desk.clone(), procs: Arc::new(FakeProcesses::default()), console: Arc::new(FakeConsole::default()), launcher: launcher.clone() };
    let mut wt = WtHost::new(setup, fx, Worker::inline(), || T0);
    wt.open_repo("B", r"\wsl.localhost\Ubuntu\home\dev\repos");
    desk.set_input_idle(120.0);
    desk.add_wt(WINDOW, "B1", Rect::new(0, 0, 800, 600), false);
    let want = wt.recipe("B", None, None).expect("Rezept");
    assert_eq!(want.wsl.as_deref(), Some("Ubuntu"));
    wt.keep_reserve(Some(want), T0);
    let l = launcher.0.lock().unwrap().clone();
    assert_eq!(l.len(), 1);
    assert!(l[0].iter().any(|a| a.ends_with("deck-wsl.exe")), "{:?}", l[0]);
    assert!(!l[0].iter().any(|a| a == "pwsh.exe"), "keine PowerShell: {:?}", l[0]);
}

/// Endet das Deck mitten im Anlegen, liegt eine unfertige Reserve in `wt_reserve.json`,
/// deren Fenster womöglich nie versteckt wurde. Das nächste Deck beendet sie, statt sie
/// als unsichtbaren, aber sichtbar offenen Agenten ohne Kachel stehen zu lassen.
#[test]
fn an_unfinished_reserve_from_a_deck_that_died_while_spawning_is_ended_on_the_next_start() {
    let mut r = quiet_desk();
    let file = r.deck.paths.app_dir.join(deck_core::reserve::FILE);
    let left = json!({"slot": "B1", "recipe": {"win": "B", "folder": "C:/Repos", "argv": [], "scheme": null}, "since": T0 - 30.0, "ready": false});
    std::fs::write(&file, left.to_string()).unwrap();
    let setup = r.deck.wt.setup_for_tests();
    r.deck.wt = crate::wt_host::WtHost::new(setup, r.deck.wt.effects_for_tests(), crate::worker::Worker::inline(), || T0);
    r.wt_agent("B1", SHELL);
    r.desk.set_input_idle(0.0);
    r.deck.tick(T0);
    assert_eq!(*r.procs.killed.lock().unwrap(), vec![SHELL], "die Shell der unfertigen Reserve ist beendet");
    assert_eq!(r.deck.wt.reserve_slot(), None);
    assert!(!file.exists());
    // Kein Fehlschlag: bei Ruhe kommt gleich eine neue, ohne Pause.
    r.desk.set_input_idle(120.0);
    r.desk.add_wt(Hwnd(83), "B2", Rect::new(0, 0, 800, 600), false);
    r.deck.tick(T0 + 3.0);
    assert_eq!(launches(&r).len(), 1, "neue Reserve sofort");
}
