//! Tests zu `stale_review.rs`: wann Claudes überholtes „Änderungen gewünscht“ verworfen wird.

use super::*;
use crate::dash::automation::{plan, Act};
use crate::dash::pr_extra::{Comment, PrExtra, Thread};
use crate::dash::prs::CiState;

const HEAD: &str = "abcdef0123456789";

fn review(author: &str, bot: bool, state: &str, oid: &str, at: f64, id: u64, class: &str) -> Review {
    Review { comment: Comment { author: author.into(), bot, at, ..Comment::default() }, state: state.into(), oid: oid.into(), db_id: id, classification: class.into() }
}

/// Die Lage vom 19.11.2026: Claude wünschte Änderungen am alten Stand, der Fund ist
/// behoben, der Thread aufgelöst, Claude kommentiert den neuen Stand nur (high), ein
/// Mensch hat freigegeben, CI grün, GitHub sagt BLOCKED.
fn blocked_by_stale_bot_review() -> PrInfo {
    PrInfo {
        repo: "Org/some-service".into(),
        number: 74,
        url: "u74".into(),
        head_ref: "feat/x".into(),
        ci: CiState::Success,
        review: ReviewState::ChangesRequested,
        extra: PrExtra {
            merge_state: "BLOCKED".into(),
            head_oid: HEAD.into(),
            assignees: vec!["me".into()],
            threads: vec![Thread { id: "t".into(), resolved: true, ..Thread::default() }],
            reviews: vec![
                review("claude", true, "CHANGES_REQUESTED", "old", 10.0, 11, "high"),
                review("claude", true, "COMMENTED", HEAD, 30.0, 12, "high"),
                review("reviewer1", false, "APPROVED", HEAD, 40.0, 13, ""),
            ],
            ..PrExtra::default()
        },
        ..PrInfo::default()
    }
}

fn st() -> FeedState {
    FeedState { automation: true, ..FeedState::fresh(1.0) }
}

fn check(p: &PrInfo, s: &FeedState) -> Option<Dismissal> {
    dismissal(p, std::slice::from_ref(p), None, s)
}

#[test]
fn a_stale_claude_change_request_that_alone_blocks_the_merge_is_dismissed() {
    let p = blocked_by_stale_bot_review();
    let d = check(&p, &st()).expect("verwerfen");
    assert_eq!(d.ids, [11]);
    assert_eq!((d.author.as_str(), d.head.as_str()), ("claude", "abcdef0"));
    assert_eq!(d.message, "All findings addressed in abcdef0; the latest review on the head requests no changes.");
    let acts = plan(std::slice::from_ref(&p), &st(), |_| None, &[], "me");
    assert_eq!(acts, [Act::DismissReview { pr: p.key(), repo: p.repo.clone(), number: 74, dismissal: d }], "verwerfen, noch nicht mergen");
}

#[test]
fn a_human_change_request_is_never_dismissed() {
    let mut p = blocked_by_stale_bot_review();
    p.extra.reviews[0] = review("reviewer2", false, "CHANGES_REQUESTED", "old", 10.0, 11, "");
    assert_eq!(check(&p, &st()), None);
    // Auch nicht neben einem überholten von Claude: der Mensch blockiert weiter.
    p.extra.reviews.push(review("claude", true, "CHANGES_REQUESTED", "old", 5.0, 9, "high"));
    assert_eq!(check(&p, &st()), None);
}

#[test]
fn an_open_thread_keeps_the_review() {
    let mut p = blocked_by_stale_bot_review();
    p.extra.threads[0].resolved = false;
    assert_eq!(check(&p, &st()), None);
    let mut p = blocked_by_stale_bot_review();
    p.open_threads = 1;
    assert_eq!(check(&p, &st()), None);
}

#[test]
fn not_when_claudes_latest_review_on_the_head_still_requests_changes() {
    let mut p = blocked_by_stale_bot_review();
    p.extra.reviews[1].state = "CHANGES_REQUESTED".into();
    assert_eq!(check(&p, &st()), None);
}

#[test]
fn not_when_claude_has_not_reviewed_the_head_yet() {
    let mut p = blocked_by_stale_bot_review();
    p.extra.reviews[1].oid = "old".into();
    assert_eq!(check(&p, &st()), None);
}

#[test]
fn not_when_the_change_request_is_on_the_head_commit() {
    let mut p = blocked_by_stale_bot_review();
    p.extra.reviews[0].oid = HEAD.into();
    assert_eq!(check(&p, &st()), None);
}

#[test]
fn not_on_a_draft_or_a_held_pr() {
    let p = PrInfo { draft: true, ..blocked_by_stale_bot_review() };
    assert_eq!(check(&p, &st()), None);
    assert!(plan(std::slice::from_ref(&p), &st(), |_| None, &[], "me").is_empty());
    let p = blocked_by_stale_bot_review();
    let mut held = st();
    held.hold.insert(p.key());
    assert_eq!(check(&p, &held), None);
    let mut off = st();
    off.automation = false;
    assert_eq!(check(&p, &off), None);
}

#[test]
fn the_merge_rule_still_applies_after_the_dismissal() {
    // high ohne Freigabe eines Menschen: Claudes Kommentar allein reicht nie.
    let mut p = blocked_by_stale_bot_review();
    p.extra.reviews.pop();
    assert_eq!(check(&p, &st()), None);
    let mut p = blocked_by_stale_bot_review();
    p.ci = CiState::Running;
    assert_eq!(check(&p, &st()), None, "CI läuft");
    let mut p = blocked_by_stale_bot_review();
    p.extra.merge_state = "BEHIND".into();
    assert_eq!(check(&p, &st()), None, "nur BLOCKED");
    let mut p = blocked_by_stale_bot_review();
    p.review = ReviewState::Approved;
    assert_eq!(check(&p, &st()), None, "GitHub sieht kein „Änderungen gewünscht“");
}

#[test]
fn a_failed_dismissal_waits_for_a_new_commit() {
    let p = blocked_by_stale_bot_review();
    let mut s = st();
    s.handled_mut(&p.key()).dismiss_failed_oid = HEAD.into();
    assert_eq!(check(&p, &s), None);
    s.handled_mut(&p.key()).dismiss_failed_oid = "older".into();
    assert!(check(&p, &s).is_some());
}
