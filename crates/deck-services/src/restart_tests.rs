//! Der Ablauf „VS Code neu starten“ gegen Attrappen (aus `tests/test_ops_vscode_restart.py`):
//! kein VS Code, kein Broker, keine echte Uhr, keine Datei. Beide Eskalationsstufen
//! (WM_CLOSE an alle Fenster, taskkill) sind gefälscht: am 2026-11-06 hat genau diese
//! Lücke die Python-Suite das VS Code beenden lassen, in dem sie lief.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use deck_core::restart_plan::Plan;

use super::fake::*;
use super::*;

// ── Herunterfahren ──────────────────────────────────────────────────────────

#[test]
fn first_save_then_terminals_then_windows() {
    let env = with(|_| {});
    run(&env, 400);
    let kinds: Vec<&str> = env
        .events()
        .iter()
        .filter_map(|e| match e {
            Ev::Save(_) => Some("save"),
            Ev::Kill(_) => Some("kill"),
            Ev::Shut(_) => Some("shut"),
            _ => None,
        })
        .collect();
    assert_eq!(kinds, ["save", "save", "kill", "kill", "kill", "shut", "shut"]);
}

#[test]
fn exactly_the_chats_of_the_plan_are_closed_before_their_windows() {
    let env = with(|_| {});
    run(&env, 400);
    let ev = env.events();
    let kills: Vec<String> = ev.iter().filter_map(|e| if let Ev::Kill(s) = e { Some(s.clone()) } else { None }).collect();
    assert_eq!(kills, ["A1", "A2", "B1"]);
    let last_kill = ev.iter().rposition(|e| matches!(e, Ev::Kill(_))).unwrap();
    let first_shut = ev.iter().position(|e| matches!(e, Ev::Shut(_))).unwrap();
    assert!(last_kill < first_shut);
}

#[test]
fn without_vs_code_nothing_is_closed() {
    let env = with(|e| e.exe = None);
    let r = run(&env, 400);
    assert_eq!(r.phase(), Phase::Failed);
    assert_eq!(env.count(|e| matches!(e, Ev::Shut(_) | Ev::Kill(_))), 0);
}

#[test]
fn without_a_connected_window_there_is_nothing_to_do() {
    let env = with(|e| e.plan = Plan::default());
    let r = run(&env, 400);
    assert_eq!(r.phase(), Phase::Failed);
    assert_eq!(env.count(|e| matches!(e, Ev::Shut(_))), 0);
}

// ── Nachsehen statt warten ──────────────────────────────────────────────────

#[test]
fn only_when_no_process_runs_any_more_is_anything_started() {
    let env = with(|e| e.alive = Mutex::new(vec![Some(3), Some(3), Some(2), Some(0)]));
    run(&env, 400);
    let ev = env.events();
    let first_launch = ev.iter().position(|e| matches!(e, Ev::Launch(_))).unwrap();
    assert_eq!(ev[..first_launch].iter().filter(|e| matches!(e, Ev::Shut(_))).count(), 2);
    assert_eq!(env.launched(), [REPO_A, REPO_B]);
}

#[test]
fn running_processes_abort_the_run_instead_of_starting_blindly() {
    let env = with(|e| e.alive = Mutex::new(vec![Some(4)]));
    let r = run(&env, 400);
    assert_eq!(r.phase(), Phase::Failed);
    assert!(env.launched().is_empty());
    assert!(!env.cleared.load(Ordering::SeqCst) && env.saved.lock().unwrap().is_some());
    // Beide Stufen wurden versucht, an der Attrappe, nicht am echten VS Code.
    let ev = env.events();
    assert!(ev.contains(&Ev::WmClose) && ev.contains(&Ev::Taskkill("Code.exe".into())));
}

#[test]
fn an_unreadable_process_list_is_no_all_clear() {
    let env = with(|e| e.alive = Mutex::new(vec![None]));
    let r = run(&env, 400);
    assert_eq!(r.phase(), Phase::Failed);
    assert!(env.launched().is_empty());
}

// ── Wieder hochfahren ───────────────────────────────────────────────────────

#[test]
fn within_a_window_the_next_chat_comes_only_after_the_previous() {
    let env = with(|_| {});
    run(&env, 400);
    let seq: Vec<(&str, String)> = env
        .events()
        .iter()
        .filter_map(|e| match e {
            Ev::Create(w, s, _, _) if w == "A" => Some(("create", s.clone())),
            Ev::Appear(w, s) if w == "A" => Some(("appear", s.clone())),
            _ => None,
        })
        .collect();
    let want = [("create", "A1"), ("appear", "A1"), ("create", "A2"), ("appear", "A2")].map(|(k, s)| (k, s.to_string()));
    assert_eq!(seq, want);
}

#[test]
fn all_chats_come_back_with_their_flags_and_their_folder() {
    let env = with(|_| {});
    run(&env, 400);
    let creates: Vec<Ev> = env.events().into_iter().filter(|e| matches!(e, Ev::Create(..))).collect();
    let c = |w: &str, s: &str, a: &str, cwd: &str| Ev::Create(w.into(), s.into(), a.into(), cwd.into());
    assert!(creates.contains(&c("A", "A1", "--resume \"x\"", REPO_A)));
    assert!(creates.contains(&c("A", "A2", "", REPO_A)));
    assert!(creates.contains(&c("B", "B1", "--resume \"y\"", REPO_B)));
}

#[test]
fn a_window_that_never_reports_back_ends_as_failure_with_the_plan_kept() {
    let env = with(|e| e.connect_after = 10_000.0);
    let r = run(&env, 2000);
    assert_eq!(r.phase(), Phase::Failed);
    assert_eq!(env.count(|e| matches!(e, Ev::Create(..))), 0);
    assert!(!env.cleared.load(Ordering::SeqCst));
}

#[test]
fn a_successful_run_clears_the_plan() {
    let env = with(|_| {});
    let r = run(&env, 400);
    assert_eq!(r.phase(), Phase::Done);
    assert!(env.cleared.load(Ordering::SeqCst));
    assert_eq!(r.made(), 3);
    assert_eq!(env.events().last(), Some(&Ev::Say(Some(true))));
}

#[test]
fn a_lost_connection_while_restoring_skips_the_rest_of_that_window() {
    let env = with(|e| e.create_ok = false);
    let r = run(&env, 400);
    assert!(matches!(r.phase(), Phase::Done | Phase::Failed));
    assert_eq!(env.count(|e| matches!(e, Ev::Create(..))), 2, "je Fenster genau ein Versuch");
    assert_eq!(r.made(), 0);
}

#[test]
fn a_chat_that_never_appears_does_not_hold_up_its_window() {
    let env = with(|e| e.appear = false);
    let r = run(&env, 2000);
    let a: Vec<String> = env.events().into_iter().filter_map(|e| if let Ev::Create(w, s, _, _) = e { (w == "A").then_some(s) } else { None }).collect();
    assert_eq!(a, ["A1", "A2"]);
    assert_eq!((r.phase(), r.made()), (Phase::Done, 0));
}

// ── Fortsetzen nach einem Abbruch ───────────────────────────────────────────

#[test]
fn a_leftover_plan_is_reopened_instead_of_planned_anew() {
    let env = with(|e| {
        e.saved = Mutex::new(Some(plan()));
        e.plan = Plan::default();
        // Erst zu (needs_relaunch fragt VOR dem Start), dann verbunden.
        e.t_start = Mutex::new(1001.0);
    });
    run(&env, 400);
    assert_eq!(env.count(|e| matches!(e, Ev::Shut(_) | Ev::Kill(_))), 0, "es ist schon alles zu");
    assert_eq!(env.launched(), [REPO_A, REPO_B]);
}

#[test]
fn whoever_is_busy_says_so() {
    let env = with(|_| {});
    let mut r = restarter(&env);
    let p = env.clone();
    r.start(&move || p.plan.clone(), None);
    assert!(r.busy());
    *env.t.lock().unwrap() += 1.0;
    r.step();
    assert!(r.busy());
}

#[test]
fn a_second_press_during_the_run_is_not_silent() {
    let env = with(|_| {});
    let mut r = restarter(&env);
    let p = env.clone();
    r.start(&move || p.plan.clone(), None);
    let said = Arc::new(Mutex::new(Vec::<String>::new()));
    let s = said.clone();
    let p = env.clone();
    assert!(!r.start(&move || p.plan.clone(), Some(Box::new(move |t, _| s.lock().unwrap().push(t.to_string())))));
    let said = said.lock().unwrap();
    assert!(said.first().is_some_and(|t| t.contains("Neustart")), "{said:?}");
}

#[test]
fn an_error_in_the_sequence_ends_the_run_instead_of_tearing_the_poll() {
    let env = with(|_| {});
    let mut r = restarter(&env);
    let p = env.clone();
    r.start(&move || p.plan.clone(), None);
    env.explode.store(true, Ordering::SeqCst);
    *env.t.lock().unwrap() += 5.0;
    r.step(); // darf NICHT werfen
    assert_eq!(r.phase(), Phase::Failed);
}

#[test]
fn messages_for_the_surface_have_no_dashes() {
    for lang in [Lang::German, Lang::English] {
        let texts = Arc::new(Mutex::new(Vec::<String>::new()));
        for alive in [vec![Some(0)], vec![Some(4)], vec![None]] {
            let env = with(|e| e.alive = Mutex::new(alive));
            let mut r = Restarter::new(Box::new(F(env.clone())), Box::new(F(env.clone())), Box::new(F(env.clone())), lang);
            let t = texts.clone();
            let p = env.clone();
            r.start(&move || p.plan.clone(), Some(Box::new(move |s, _| t.lock().unwrap().push(s.to_string()))));
            for _ in 0..400 {
                r.step();
                *env.t.lock().unwrap() += 0.5;
            }
        }
        for t in texts.lock().unwrap().iter() {
            assert!(!t.contains('–') && !t.contains('—'), "{t}");
        }
    }
}
