//! Abgestürzte Agenten fortsetzen (`crash_resume.rs`) gegen Attrappen: kein Test öffnet
//! ein echtes Fenster.

use deck_core::crash_resume::{LAUNCH_WAIT_S, RESUME_DELAY_S, RETRY_DELAY_S};
use serde_json::json;

use crate::tests::{Rig, rig};

const SID: &str = "f9cc922b-c282-46f1-afa7-97d71d17b0b9";
const T0: f64 = 5_000.0;

/// Block B mit einem laufenden Agenten B4, den das Deck schon einmal gesehen hat.
fn running_agent(status: &str) -> Rig {
    let mut r = rig();
    r.wt_block("B", "Repos");
    r.wt_agent("B4", 40);
    r.state("B4", json!({"slot": "B4", "status": status, "ts": T0 - 10.0, "session_id": SID, "effort": "high", "cwd": "Z:/gibt-es-nicht"}));
    r.deck.tick(T0);
    r
}

fn launches(r: &Rig) -> usize {
    r.launcher.0.lock().unwrap().len()
}

fn kill_shell(r: &Rig) {
    r.procs.alive.lock().unwrap().clear();
}

#[test]
fn a_working_agent_that_dies_while_nobody_is_at_the_machine_comes_back_with_a_nudge() {
    let mut r = running_agent("running");
    r.desk.set_input_idle(3600.0);
    kill_shell(&r);
    r.deck.tick(T0 + 1.0);
    assert_eq!(launches(&r), 0, "erst nach der Wartezeit");
    r.deck.tick(T0 + 1.0 + RESUME_DELAY_S);
    assert_eq!(launches(&r), 1);
    let argv = r.launcher.0.lock().unwrap()[0].clone();
    assert!(argv.contains(&"deck-B4".to_string()), "unter dem alten Slot: {argv:?}");
    assert!(argv.iter().any(|a| a == "C:/Repos"), "Ordner des Blocks, weil das cwd fehlt: {argv:?}");
    let args = r.last_claude_args();
    assert_eq!(&args[..2], ["--resume", SID]);
    assert!(args.windows(2).any(|w| w == ["--effort", "high"]), "{args:?}");
    assert!(args.last().unwrap().contains("Agent Deck hat die Session fortgesetzt"), "{args:?}");
    // Die neue Shell meldet sich: erledigt, kein weiterer Start.
    r.wt_agent("B4", 41);
    r.deck.tick(T0 + 5.0 + RESUME_DELAY_S);
    assert!(r.deck.resumes.is_empty());
    r.deck.tick(T0 + RESUME_DELAY_S + LAUNCH_WAIT_S + RETRY_DELAY_S + 10.0);
    assert_eq!(launches(&r), 1);
}

#[test]
fn a_finished_agent_comes_back_without_a_prompt() {
    let mut r = running_agent("done");
    r.desk.set_input_idle(3600.0);
    kill_shell(&r);
    r.deck.tick(T0 + 1.0);
    r.deck.tick(T0 + 1.0 + RESUME_DELAY_S);
    let args = r.last_claude_args();
    assert_eq!(args.last().map(String::as_str), Some("high"), "letztes Argument ist ein Flag: {args:?}");
}

#[test]
fn a_window_closed_while_the_user_is_typing_stays_closed() {
    let mut r = running_agent("running");
    r.desk.set_input_idle(2.0);
    kill_shell(&r);
    r.deck.tick(T0 + 1.0);
    r.deck.tick(T0 + 1.0 + RESUME_DELAY_S);
    assert_eq!(launches(&r), 0);
}

#[test]
fn closing_on_the_tile_never_resumes() {
    let mut r = running_agent("running");
    r.desk.set_input_idle(3600.0);
    r.deck.close_agent("B4");
    kill_shell(&r);
    r.deck.tick(T0 + 1.0);
    r.deck.tick(T0 + 1.0 + RESUME_DELAY_S);
    assert_eq!(launches(&r), 0);
}

#[test]
fn shells_already_dead_when_the_deck_starts_are_left_alone() {
    let mut r = rig();
    r.wt_block("B", "Repos");
    r.wt_agent("B4", 40);
    r.state("B4", json!({"slot": "B4", "status": "running", "ts": T0, "session_id": SID}));
    r.desk.set_input_idle(3600.0);
    kill_shell(&r);
    r.deck.tick(T0);
    r.deck.tick(T0 + RESUME_DELAY_S);
    assert_eq!(launches(&r), 0);
}

#[test]
fn a_launch_that_never_shows_up_is_tried_twice_then_given_up() {
    let mut r = running_agent("running");
    r.desk.set_input_idle(3600.0);
    kill_shell(&r);
    r.deck.tick(T0 + 1.0);
    let t = T0 + 1.0 + RESUME_DELAY_S;
    r.deck.tick(t);
    r.deck.tick(t + LAUNCH_WAIT_S);
    r.deck.tick(t + LAUNCH_WAIT_S + RETRY_DELAY_S);
    assert_eq!(launches(&r), 2);
    r.deck.tick(t + 2.0 * LAUNCH_WAIT_S + RETRY_DELAY_S);
    assert!(r.deck.resumes.is_empty());
    r.deck.tick(t + 10.0 * LAUNCH_WAIT_S);
    assert_eq!(launches(&r), 2);
}
