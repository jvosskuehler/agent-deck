//! Tests zu `feed_state.rs`: Datei lesen, Konflikte datieren, aufräumen.

use super::*;
use crate::dash::pr_extra::PrExtra;
use crate::dash::prs::PrInfo;

fn pr(n: u64, conflicting: bool) -> PrInfo {
    PrInfo { repo: "Org/r".into(), number: n, extra: PrExtra { conflicting, ..PrExtra::default() }, ..PrInfo::default() }
}

fn snap(prs: Vec<PrInfo>) -> PrSnapshot {
    PrSnapshot { open: prs, fetched_at: 10.0, ..PrSnapshot::default() }
}

#[test]
fn a_broken_or_missing_file_starts_fresh_with_automation_on() {
    let st = FeedState::from_json(Some("{kaputt"), 500.0);
    assert_eq!((st.since, st.automation), (500.0, true));
    assert_eq!(FeedState::from_json(None, 7.0).since, 7.0);
}

#[test]
fn a_run_that_was_running_when_the_deck_stopped_counts_as_failed() {
    let mut st = FeedState::fresh(1.0);
    st.runs.push(Run {
        id: "a".into(),
        pr: "Org/r#1".into(),
        label: "r#1".into(),
        url: String::new(),
        title: String::new(),
        task: TaskKind::CiRed,
        started: 2.0,
        ended: 0.0,
        status: RunStatus::Running,
        report: String::new(),
        dir: "d".into(),
        comment_id: String::new(),
        thread_id: String::new(),
        draft_path: String::new(),
    });
    st.automation = false;
    let back = FeedState::from_json(Some(&st.to_json()), 50.0);
    assert_eq!((back.runs[0].status, back.runs[0].ended, back.automation), (RunStatus::Failed, 50.0, false));
}

#[test]
fn conflicts_get_the_time_they_were_first_seen_and_closed_prs_are_forgotten() {
    let mut st = FeedState::fresh(1.0);
    st.hold.insert("Org/r#9".into());
    st.seen_any = true;
    assert!(st.observe(&snap(vec![pr(1, true), pr(2, false)]), 100.0));
    assert_eq!(st.conflict_since.get("Org/r#1"), Some(&100.0));
    assert!(st.hold.is_empty(), "PR #9 ist nicht mehr offen");
    st.observe(&snap(vec![pr(1, true)]), 200.0);
    assert_eq!(st.conflict_since.get("Org/r#1"), Some(&100.0), "bleibt beim ersten Sehen");
    st.observe(&snap(vec![pr(1, false)]), 300.0);
    assert!(st.conflict_since.is_empty());
}

#[test]
fn after_a_failed_fetch_nothing_is_forgotten() {
    let mut st = FeedState::fresh(1.0);
    st.hold.insert("Org/r#9".into());
    let failed = PrSnapshot { error: Some("gh: Exit 1".into()), fetched_at: 10.0, ..PrSnapshot::default() };
    st.observe(&failed, 100.0);
    assert!(st.hold.contains("Org/r#9"));
}

#[test]
fn old_activity_and_jira_checks_go_after_two_days() {
    let mut st = FeedState::fresh(1.0);
    let a = |at: f64| Activity { at, pr: String::new(), label: String::new(), url: String::new(), title: String::new(), did: Did::Merged, run: String::new() };
    st.record(a(10.0));
    st.record(a(KEEP_S + 5.0));
    st.jira_checked.insert("Org/r#1".into(), 10.0);
    st.observe(&PrSnapshot::default(), KEEP_S + 20.0);
    assert_eq!(st.activity.len(), 1);
    assert!(st.jira_checked.is_empty());
}

#[test]
fn a_conflict_that_already_exists_at_the_first_fetch_counts_as_old() {
    let mut st = FeedState::fresh(50.0);
    st.observe(&snap(vec![pr(1, true)]), 100.0);
    assert_eq!(st.conflict_since.get("Org/r#1"), Some(&50.0), "nicht neuer als since: startet keinen Agenten");
    st.observe(&snap(vec![pr(1, true), pr(2, true)]), 200.0);
    assert_eq!(st.conflict_since.get("Org/r#2"), Some(&200.0), "ab dem zweiten Abruf ist ein Konflikt neu");
}

#[test]
fn log_verdicts_from_an_older_rule_are_dropped() {
    let old = r#"{"since":5.0,"verdicts":{"7":true}}"#;
    let st = FeedState::from_json(Some(old), 10.0);
    assert!(st.verdicts.is_empty());
    assert_eq!(st.verdict_rule, crate::dash::ci_cause::VERDICT_RULE);
    let mut cur = FeedState::fresh(5.0);
    cur.verdicts.insert(7, true);
    assert_eq!(FeedState::from_json(Some(&cur.to_json()), 10.0).verdicts.len(), 1, "gleicher Regelstand bleibt");
}
