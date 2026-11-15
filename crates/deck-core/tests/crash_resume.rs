//! Agenten, die ohne das Deck verschwinden: Urteil, Protokollzeile, Ablauf des Fortsetzens.

use deck_core::crash_resume::*;
use deck_core::i18n::Lang;
use deck_core::slot_state::SlotState;
use serde_json::json;

const NOW: f64 = 1_000_000.0;
const SID: &str = "f9cc922b-c282-46f1-afa7-97d71d17b0b9";

fn ended(slot: &str) -> Ended {
    Ended { slot: slot.into(), by_deck: false, at_start: false }
}

fn state(status: &str) -> SlotState {
    json!({"slot": "B43", "status": status, "session_id": SID, "cwd": "C:\\Repos", "effort": "high", "mode": "bypass"})
        .as_object()
        .unwrap()
        .clone()
}

fn judge_now(e: &Ended, st: Option<&SlotState>, idle_s: f64) -> Verdict {
    judge(e, st, idle_s, true, &mut ResumeMemo::default(), NOW)
}

#[test]
fn a_shell_that_dies_while_nobody_is_at_the_machine_is_resumed() {
    let Verdict::Resume(p) = judge_now(&ended("B43"), Some(&state("running")), 3600.0) else { panic!() };
    assert_eq!(p.slot, "B43");
    assert_eq!(p.session_id, SID);
    assert_eq!(p.cwd.as_deref(), Some("C:\\Repos"));
    assert_eq!(p.effort.as_deref(), Some("high"));
    assert_eq!(p.due, NOW + RESUME_DELAY_S);
}

#[test]
fn only_an_agent_that_was_working_gets_a_nudge() {
    for (status, nudge) in [("thinking", true), ("running", true), ("waiting", true), ("done", false), ("idle", false)] {
        let Verdict::Resume(p) = judge_now(&ended("B1"), Some(&state(status)), 600.0) else { panic!("{status}") };
        assert_eq!(p.nudge, nudge, "{status}");
    }
}

#[test]
fn recent_input_means_the_user_closed_it_and_it_stays_closed() {
    assert_eq!(judge_now(&ended("B1"), Some(&state("running")), 2.0), Verdict::UserActive(2.0));
    assert!(matches!(judge_now(&ended("B1"), Some(&state("running")), USER_ACTIVE_S), Verdict::Resume(_)));
}

#[test]
fn closing_by_the_deck_and_deaths_found_at_start_are_never_resumed() {
    let by_deck = Ended { by_deck: true, ..ended("B1") };
    assert_eq!(judge_now(&by_deck, Some(&state("running")), 3600.0), Verdict::ByDeck);
    let at_start = Ended { at_start: true, ..ended("B1") };
    assert_eq!(judge_now(&at_start, Some(&state("running")), 3600.0), Verdict::AtStart);
}

#[test]
fn without_a_valid_session_or_block_there_is_nothing_to_resume() {
    assert_eq!(judge_now(&ended("B1"), None, 3600.0), Verdict::NoSession);
    let mut bad = state("running");
    bad.insert("session_id".into(), json!("x; rm -rf"));
    assert_eq!(judge_now(&ended("B1"), Some(&bad), 3600.0), Verdict::NoSession);
    assert_eq!(judge(&ended("B1"), Some(&state("running")), 3600.0, false, &mut ResumeMemo::default(), NOW), Verdict::NoBlock);
}

#[test]
fn a_session_that_keeps_dying_is_given_up_after_two_resumes() {
    let mut memo = ResumeMemo::default();
    memo.record(SID, NOW - 100.0);
    memo.record(SID, NOW - 50.0);
    assert_eq!(judge(&ended("B1"), Some(&state("running")), 3600.0, true, &mut memo, NOW), Verdict::TooOften);
    // Nach dem Fenster zählt es wieder.
    let later = NOW + MEMO_WINDOW_S;
    assert!(matches!(judge(&ended("B1"), Some(&state("running")), 3600.0, true, &mut memo, later), Verdict::Resume(_)));
}

#[test]
fn simultaneous_deaths_share_one_log_line_with_count_and_reasons() {
    let st = state("running");
    let v = judge_now(&ended("B43"), Some(&st), 3600.0);
    let batch = vec![
        (ended("B42"), Some("done".into()), Verdict::NoSession),
        (ended("B43"), status_text(Some(&st)), v),
        (Ended { by_deck: true, ..ended("B44") }, None, Verdict::ByDeck),
    ];
    let line = log_line(&batch, 3600.0).unwrap();
    assert!(line.starts_with("2 Agenten gleichzeitig ohne das Deck beendet (letzte Eingabe vor 3600 s)"), "{line}");
    assert!(line.contains("B43 (running): wird in 20 s fortgesetzt mit Anstoß (Session f9cc922b)"), "{line}");
    assert!(line.contains("B42 (done): ohne Session-ID"), "{line}");
    assert!(!line.contains("B44"), "{line}");
}

#[test]
fn closing_by_the_deck_alone_is_not_logged() {
    assert_eq!(log_line(&[(Ended { by_deck: true, ..ended("B1") }, None, Verdict::ByDeck)], 0.0), None);
}

#[test]
fn deaths_found_at_start_say_so() {
    let at = Ended { at_start: true, ..ended("B1") };
    let line = log_line(&[(at, None, Verdict::AtStart)], 0.0).unwrap();
    assert!(line.starts_with("Agent beim Start ohne Fenster vorgefunden: B1 (ohne Status)"), "{line}");
}

#[test]
fn the_nudge_contains_no_dashes_or_quotes() {
    for lang in [Lang::German, Lang::English] {
        let p = continue_prompt(lang);
        assert!(!p.contains(['—', '–', '"', '\'', '`', '$']), "{p}");
    }
}

fn plan() -> Plan {
    Plan { slot: "B43".into(), session_id: SID.into(), cwd: None, effort: None, mode: None, nudge: true, due: NOW }
}

#[test]
fn a_pending_resume_waits_for_its_time_then_launches_and_confirms() {
    let mut p = Pending::new(Plan { due: NOW + 5.0, ..plan() });
    assert_eq!(p.step(NOW, |_| false), Step::Wait);
    assert_eq!(p.step(NOW + 5.0, |_| false), Step::Launch);
    p.launched_as(Some("B43".into()), NOW + 5.0);
    assert_eq!(p.step(NOW + 6.0, |_| false), Step::Wait);
    assert_eq!(p.step(NOW + 7.0, |s| s == "B43"), Step::Confirmed("B43".into()));
}

#[test]
fn a_launch_that_never_shows_up_is_retried_once_then_given_up() {
    let mut p = Pending::new(plan());
    assert_eq!(p.step(NOW, |_| false), Step::Launch);
    p.launched_as(None, NOW);
    let t = NOW + LAUNCH_WAIT_S;
    assert_eq!(p.step(t, |_| false), Step::Wait);
    assert_eq!(p.step(t + RETRY_DELAY_S - 1.0, |_| false), Step::Wait);
    assert_eq!(p.step(t + RETRY_DELAY_S, |_| false), Step::Launch);
    p.launched_as(Some("B44".into()), t + RETRY_DELAY_S);
    assert_eq!(p.holds(), vec!["B43".to_string(), "B44".to_string()]);
    assert_eq!(p.step(t + RETRY_DELAY_S + LAUNCH_WAIT_S, |_| false), Step::GiveUp);
}
