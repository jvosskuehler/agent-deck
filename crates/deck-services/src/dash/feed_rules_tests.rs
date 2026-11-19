//! Tests zum Feed-Dienst: Grenzen der Automatik (CI-Versuche, Log je Wechsel, schon
//! gelöschter Branch, überholtes Bot-Review). Attrappen und Helfer aus
//! `feed_service_tests.rs`.

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

/// Claude wünschte Änderungen am alten Stand und kommentiert den Kopf nur; ein Mensch hat
/// den Kopf freigegeben, alles grün, GitHub sagt trotzdem BLOCKED.
fn blocked_by_stale_bot_review(n: u64) -> PrInfo {
    let review = |author: &str, bot: bool, state: &str, oid: &str, at: f64, id: u64| Review {
        comment: Comment { author: author.into(), bot, at, ..Comment::default() },
        state: state.into(),
        oid: oid.into(),
        db_id: id,
        classification: if bot { "high".into() } else { String::new() },
    };
    let mut p = ready(n);
    p.repo = "Org/demo-service".into();
    p.review = deck_core::dash::prs::ReviewState::ChangesRequested;
    p.extra.merge_state = "BLOCKED".into();
    p.extra.head_oid = "1234567890".into();
    p.extra.threads = vec![Thread { id: "t".into(), resolved: true, ..Thread::default() }];
    p.extra.reviews = vec![
        review("claude", true, "CHANGES_REQUESTED", "0ld", NOW - 900.0, 51),
        review("claude", true, "COMMENTED", "1234567890", NOW - 600.0, 52),
        review("reviewer1", false, "APPROVED", "1234567890", NOW - 300.0, 53),
    ];
    p
}

#[test]
fn a_stale_bot_change_request_is_dismissed_once_logged_once_and_the_next_fetch_merges() {
    let d = tempfile::tempdir().unwrap();
    let (w, svc) = setup(d.path(), 1.0);
    let p = blocked_by_stale_bot_review(74);
    publish(&w, vec![p.clone()], vec![], NOW);
    svc.round_now();
    svc.round_now();
    let dismiss = "dismiss Org/demo-service#74 51 All findings addressed in 1234567; the latest review on the head requests no changes.";
    assert_eq!(w.calls(), [dismiss], "verwerfen, noch nicht mergen");
    let st = svc.state();
    assert!(st.activity.iter().any(|a| a.did == Did::ReviewDismissed("claude".into(), "1234567".into())));
    assert_eq!(log_text(d.path()).matches("überholtes Review von claude verworfen").count(), 1);
    // Nächster Abruf: GitHub zeigt das Review als verworfen und gibt den PR frei.
    let mut after = p;
    after.review = deck_core::dash::prs::ReviewState::Approved;
    after.extra.merge_state = "CLEAN".into();
    after.extra.reviews[0].state = "DISMISSED".into();
    publish(&w, vec![after], vec![], NOW + 180.0);
    svc.round_now();
    let calls = w.calls();
    assert_eq!(calls.iter().filter(|c| c.starts_with("dismiss")).count(), 1, "{calls:?}");
    assert!(calls.contains(&"merge https://github.com/o/r/pull/74".to_string()), "{calls:?}");
}

#[test]
fn a_human_change_request_or_an_open_thread_is_left_alone() {
    let d = tempfile::tempdir().unwrap();
    let (w, svc) = setup(d.path(), 1.0);
    let mut human = blocked_by_stale_bot_review(1);
    human.extra.reviews[0].comment.author = "reviewer2".into();
    human.extra.reviews[0].comment.bot = false;
    let mut thread = blocked_by_stale_bot_review(2);
    thread.extra.threads[0].resolved = false;
    thread.open_threads = 1;
    publish(&w, vec![human, thread], vec![], NOW);
    svc.round_now();
    assert!(!w.calls().iter().any(|c| c.starts_with("dismiss") || c.starts_with("merge")), "{:?}", w.calls());
}
