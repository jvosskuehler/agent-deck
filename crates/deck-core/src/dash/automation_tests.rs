//! Tests zu `automation.rs`: wann gemergt wird, was nachgetragen wird.

use super::*;
use crate::dash::feed_state::Handled;
use crate::dash::jira_sync::PrState;
use crate::dash::pr_extra::{Check, Comment, PrExtra, Review};

fn approved() -> Review {
    Review { comment: Comment { author: "reviewer1".into(), at: 10.0, ..Comment::default() }, state: "APPROVED".into(), ..Review::default() }
}

fn ready(n: u64) -> PrInfo {
    PrInfo {
        repo: "Org/orpheus-client-electron".into(),
        number: n,
        title: format!("feat(ORPH-{n}): x"),
        url: format!("u{n}"),
        head_ref: "b".into(),
        ci: CiState::Success,
        extra: PrExtra {
            merge_state: "CLEAN".into(),
            head_oid: "h".into(),
            assignees: vec!["me".into()],
            reviews: vec![approved()],
            ..PrExtra::default()
        },
        ..PrInfo::default()
    }
}

fn st() -> FeedState {
    FeedState { automation: true, ..FeedState::fresh(1.0) }
}

fn r(logins: &[&str]) -> Vec<String> {
    logins.iter().map(|l| l.to_string()).collect()
}

fn one(p: &PrInfo) -> &[PrInfo] {
    std::slice::from_ref(p)
}

#[test]
fn a_clean_green_approved_pr_is_merged() {
    let p = ready(1);
    assert_eq!(merge_wait(&p, one(&p), None, &st()), None);
    let acts = plan(one(&p), &st(), |_| None, &r(&["reviewer1"]), "me");
    assert_eq!(acts, [Act::Merge { pr: p.key(), url: "u1".into(), repo: p.repo.clone(), head_ref: "b".into() }]);
}

#[test]
fn every_missing_condition_blocks_the_merge() {
    let s = st();
    let check = |p: PrInfo, cause: Option<CiCause>| merge_wait(&p, one(&p), cause, &s);
    assert_eq!(check(PrInfo { draft: true, ..ready(1) }, None), Some(MergeWait::Draft));
    assert_eq!(check(PrInfo { open_threads: 2, ..ready(1) }, None), Some(MergeWait::OpenThreads(2)));
    assert_eq!(check(PrInfo { ci: CiState::Running, ..ready(1) }, None), Some(MergeWait::CiRunning));
    assert_eq!(check(PrInfo { ci: CiState::Failure, ..ready(1) }, Some(CiCause::Unknown)), Some(MergeWait::CiRed));
    assert_eq!(check(PrInfo { ci: CiState::Failure, ..ready(1) }, Some(CiCause::ReviewGate)), Some(MergeWait::NeedsHuman));
    let mut no_approval = ready(1);
    no_approval.extra.reviews.clear();
    assert_eq!(check(no_approval, None), Some(MergeWait::NoApproval));
    let mut changes = ready(1);
    changes.extra.reviews.push(Review { comment: Comment { author: "reviewer2".into(), at: 20.0, ..Comment::default() }, state: "CHANGES_REQUESTED".into(), ..Review::default() });
    assert_eq!(check(changes, None), Some(MergeWait::ChangesRequested));
    let mut behind = ready(1);
    behind.extra.merge_state = "BEHIND".into();
    assert_eq!(check(behind, None), Some(MergeWait::State("BEHIND".into())));
    let mut running = ready(1);
    running.extra.checks = vec![Check { running: true, ..Check::default() }];
    assert_eq!(check(running, None), Some(MergeWait::CiRunning));
    let mut conflict = ready(1);
    conflict.extra.conflicting = true;
    assert_eq!(check(conflict, None), Some(MergeWait::Conflict));
}

#[test]
fn code_security_red_counts_as_green_even_when_github_says_unstable() {
    let mut p = PrInfo { ci: CiState::Failure, ..ready(1) };
    p.extra.merge_state = "UNSTABLE".into();
    assert_eq!(merge_wait(&p, one(&p), Some(CiCause::CodeSecurity), &st()), None);
    p.extra.merge_state = "UNSTABLE".into();
    assert_eq!(merge_wait(&PrInfo { ci: CiState::Success, ..p.clone() }, one(&p), None, &st()), Some(MergeWait::State("UNSTABLE".into())));
}

#[test]
fn unstable_from_an_outdated_red_run_merges_when_every_latest_check_is_green() {
    let mut p = PrInfo { ci: CiState::Failure, ..ready(1) };
    p.extra.merge_state = "UNSTABLE".into();
    p.extra.checks = vec![Check { name: "lint".into(), done_at: 5.0, ..Check::default() }];
    assert_eq!(merge_wait(&p, one(&p), None, &st()), None);
}

#[test]
fn hold_list_and_emergency_stop_prevent_every_merge() {
    let p = ready(1);
    let mut s = st();
    s.hold.insert(p.key());
    assert_eq!(merge_wait(&p, one(&p), None, &s), Some(MergeWait::Held));
    let mut off = st();
    off.automation = false;
    assert!(plan(one(&p), &off, |_| None, &r(&["reviewer1"]), "me").is_empty());
    assert_eq!(merge_wait(&p, one(&p), None, &off), Some(MergeWait::Off));
}

#[test]
fn a_present_merge_order_status_must_be_success() {
    let status = |failed: bool, running: bool| Check { name: "merge-order".into(), failed, running, ..Check::default() };
    let with = |c: Check| PrInfo { extra: PrExtra { checks: vec![c], ..ready(1).extra }, ..ready(1) };
    let s = st();
    let p = with(status(false, true));
    assert_eq!(merge_wait(&p, one(&p), None, &s), Some(MergeWait::MergeOrder("pending".into())));
    let p = PrInfo { ci: CiState::Failure, ..with(status(true, false)) };
    assert_eq!(merge_wait(&p, one(&p), None, &s), Some(MergeWait::MergeOrder("failure".into())));
    let p = with(status(false, false));
    assert_eq!(merge_wait(&p, one(&p), None, &s), None);
    assert_eq!(MergeWait::DependsOn("api#7".into()).text(Lang::German), "wartet auf api#7");
}

fn review(author: &str, bot: bool, state: &str, oid: &str, class: &str) -> Review {
    Review { comment: Comment { author: author.into(), bot, at: 10.0, ..Comment::default() }, state: state.into(), oid: oid.into(), classification: class.into(), ..Review::default() }
}

#[test]
fn a_claude_approval_of_the_current_head_suffices_unless_claude_says_high() {
    let s = st();
    let mut p = ready(1);
    p.extra.reviews = vec![review("claude", true, "APPROVED", "h", "low")];
    assert_eq!(merge_wait(&p, one(&p), None, &s), None, "Claude-Freigabe reicht");
    p.extra.reviews = vec![review("claude", true, "APPROVED", "h", "high")];
    assert_eq!(merge_wait(&p, one(&p), None, &s), Some(MergeWait::HighNeedsHuman));
    p.extra.reviews.push(review("reviewer1", false, "APPROVED", "old", ""));
    assert_eq!(merge_wait(&p, one(&p), None, &s), None, "high plus ein Mensch");
    p.extra.reviews = vec![review("claude", true, "APPROVED", "old", "low")];
    assert_eq!(merge_wait(&p, one(&p), None, &s), Some(MergeWait::NoApproval), "Freigabe eines alten Stands");
    p.extra.reviews = vec![review("copilot-pull-request-reviewer", true, "APPROVED", "h", "")];
    assert_eq!(merge_wait(&p, one(&p), None, &s), Some(MergeWait::NoApproval), "andere Bots zählen nicht");
}

#[test]
fn drafts_are_never_touched_by_the_mechanics() {
    let mut p = PrInfo { draft: true, ..ready(30) };
    p.extra.assignees.clear();
    assert!(plan(one(&p), &st(), |_| None, &r(&["reviewer1"]), "me").is_empty());
}

#[test]
fn a_failed_merge_waits_for_a_new_commit() {
    let p = ready(1);
    let mut s = st();
    s.handled_mut(&p.key()).merge_failed_oid = "h".into();
    assert_eq!(merge_wait(&p, one(&p), None, &s), Some(MergeWait::Failed));
}

#[test]
fn dependencies_from_the_description_and_backend_before_frontend() {
    let mut p = ready(1);
    p.extra.body = "Merge after: orpheus-management-api#7
after #2".into();
    let mut s = st();
    assert_eq!(merge_wait(&p, one(&p), None, &s), Some(MergeWait::DependsOn("api#7".into())), "unbekannt heißt warten");
    s.deps.insert("Org/orpheus-management-api#7".into(), PrState::Merged);
    s.deps.insert("Org/orpheus-client-electron#2".into(), PrState::Open);
    assert_eq!(merge_wait(&p, one(&p), None, &s), Some(MergeWait::DependsOn("client#2".into())));
    s.deps.insert("Org/orpheus-client-electron#2".into(), PrState::Merged);
    assert_eq!(merge_wait(&p, one(&p), None, &s), None, "alles gemergt");
    let ui = PrInfo { repo: "Org/orpheus-management-ui".into(), title: "feat(ORPH-5): ui".into(), ..ready(3) };
    let api = PrInfo { repo: "Org/orpheus-management-api".into(), title: "feat(ORPH-5): api".into(), ..ready(4) };
    assert_eq!(merge_wait(&ui, &[ui.clone(), api.clone()], None, &st()), Some(MergeWait::DependsOn("api#4".into())));
    assert_eq!(merge_wait(&api, &[ui.clone(), api.clone()], None, &st()), None, "das Backend wartet nicht");
}

#[test]
fn reviewer_and_assignee_are_added_once_and_never_for_myself() {
    let mut p = ready(20);
    p.extra.assignees.clear();
    p.extra.reviews.clear();
    let s = st();
    let acts = plan(one(&p), &s, |_| None, &r(&["reviewer1"]), "me");
    assert!(acts.contains(&Act::AddReviewer { pr: p.key(), url: "u20".into(), login: "reviewer1".into() }));
    assert!(acts.contains(&Act::AddAssignee { pr: p.key(), url: "u20".into() }));
    let mut done = s.clone();
    *done.handled_mut(&p.key()) = Handled { reviewer: true, assignee: true, ..Handled::default() };
    assert!(plan(one(&p), &done, |_| None, &r(&["reviewer1"]), "me").is_empty());
    let reviewer_added = |acts: Vec<Act>| acts.iter().any(|a| matches!(a, Act::AddReviewer { .. }));
    assert!(!reviewer_added(plan(one(&p), &s, |_| None, &r(&["me"]), "me")));
    assert!(!reviewer_added(plan(one(&p), &s, |_| None, &[], "me")), "ohne Einstellung keiner");
    p.waiting_on = vec!["Reviewer1".into()];
    assert!(!reviewer_added(plan(one(&p), &s, |_| None, &r(&["reviewer1"]), "me")), "schon angefragt");
}

#[test]
fn several_reviewers_come_in_one_call_without_me_and_without_those_already_asked() {
    let mut p = ready(21);
    p.extra.reviews.clear();
    p.waiting_on = vec!["Anna".into()];
    let acts = plan(one(&p), &st(), |_| None, &r(&["anna", "leo", "me", "ben"]), "me");
    assert!(acts.contains(&Act::AddReviewer { pr: p.key(), url: "u21".into(), login: "leo,ben".into() }));
}

#[test]
fn the_reviewer_setting_is_a_comma_list_cleaned_up() {
    assert_eq!(reviewer_list(" @leo, anna;Leo  ben ,, "), r(&["leo", "anna", "ben"]));
    assert!(reviewer_list("  ").is_empty());
}
