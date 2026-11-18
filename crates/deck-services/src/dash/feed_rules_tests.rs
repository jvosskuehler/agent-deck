//! Tests zum Feed-Dienst: Grenzen der Automatik (CI-Versuche, Log je Wechsel, schon
//! gelöschter Branch). Attrappen und Helfer aus `feed_service_tests.rs`.

use super::*;

fn log_text(dir: &Path) -> String {
    std::fs::read_to_string(dir.join(LOG_FILE)).unwrap_or_default()
}

#[test]
fn a_branch_github_already_deleted_counts_as_deleted_not_as_failure() {
    let d = tempfile::tempdir().unwrap();
    let (w, svc) = setup(d.path(), 1.0);
    w.branch_gone.store(true, Ordering::SeqCst);
    publish(&w, vec![ready(1)], vec![], NOW);
    svc.round_now();
    assert!(svc.state().activity.iter().any(|a| a.did == Did::BranchDeleted));
    let log = log_text(d.path());
    assert!(log.contains("Remote-Branch schon von GitHub gelöscht") && !log.contains("gescheitert"), "{log}");
}

#[test]
fn a_session_in_a_worktree_found_only_by_branch_is_logged_once_not_every_round() {
    let d = tempfile::tempdir().unwrap();
    let (w, svc) = setup(d.path(), 1.0);
    // Ticket im Titel passt nicht zum Ordner: die Session in wt/7-client zeigt sich erst,
    // wenn der worktree über den Branch gefunden ist (sonst alle 15 s dieselbe Zeile).
    let mut p = with_bot_thread(7);
    p.title = "feat(ORPH-9): x".into();
    svc.set_in_use(vec![r"C:\R\wt\7-client".into()]);
    for k in 0..5 {
        publish(&w, vec![p.clone()], vec![], NOW + f64::from(k));
        svc.round_now();
    }
    assert!(!w.calls().iter().any(|c| c.starts_with("claude")));
    assert_eq!(log_text(d.path()).matches("Agent nicht gestartet").count(), 1);
    let (view, _) = svc.view(NOW, Lang::German);
    assert_eq!(view.agents[0].state, "wartet, Session aktiv");
}

fn red(n: u64, head: &str) -> PrInfo {
    let mut p = PrInfo { ci: CiState::Failure, ..ready(n) };
    p.extra.head_oid = head.into();
    p.extra.merge_state = "UNSTABLE".into();
    p.extra.checks = vec![deck_core::dash::pr_extra::Check { name: "build".into(), workflow: "CI".into(), job_id: 1, failed: true, done_at: NOW - 60.0, ..Default::default() }];
    p
}

#[test]
fn red_ci_gets_two_agents_then_waits_for_a_human_until_it_was_green_once() {
    let d = tempfile::tempdir().unwrap();
    let (w, svc) = setup(d.path(), 1.0);
    w.done.store(true, Ordering::SeqCst);
    let agents = |w: &World| w.calls().iter().filter(|c| c.starts_with("claude")).count();
    // Jeder Agent pusht, die CI ist am neuen Commit wieder rot.
    for (k, head) in ["a", "b", "c", "c"].iter().enumerate() {
        publish(&w, vec![red(8, head)], vec![], NOW + k as f64);
        svc.round_now();
        svc.round_now();
    }
    assert_eq!(agents(&w), 2);
    let gave_up = svc.state().activity.iter().filter(|a| a.did == Did::CiGaveUp).count();
    assert_eq!(gave_up, 1, "einmal je Commit melden");
    // Einmal grün, dann wieder rot: ein neuer Versuch.
    let mut green = ready(8);
    green.extra.head_oid = "d".into();
    green.extra.checks = vec![deck_core::dash::pr_extra::Check { name: "build".into(), done_at: NOW, ..Default::default() }];
    green.waiting_on = vec!["reviewer1".into()];
    green.extra.reviews.clear();
    publish(&w, vec![green], vec![], NOW + 10.0);
    svc.round_now();
    publish(&w, vec![red(8, "e")], vec![], NOW + 11.0);
    svc.round_now();
    assert_eq!(agents(&w), 3);
}
