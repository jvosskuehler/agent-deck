//! Tests zu `feed.rs` und `feed_events.rs`: wer mich braucht, wann das Kommentar-Fenster
//! erscheint, was in der Zeitleiste steht.

use super::*;
use crate::dash::bg_tasks::{Causes, TaskKind};
use crate::dash::ci_cause::CiCause;
use crate::dash::feed_state::{Activity, Did, Draft};
use crate::dash::feed_view::FeedKind;
use crate::dash::pr_extra::{Check, PrExtra, Review, Thread};
use crate::dash::prs::{CiState, MergedPr};

const NOW: f64 = 100_000.0;

fn c(id: &str, author: &str, bot: bool, at: f64) -> Comment {
    Comment { id: id.into(), author: author.into(), bot, at, text: format!("text of {id}"), url: format!("https://github.com/o/r/pull/1#{id}") }
}

fn pr(n: u64, extra: PrExtra) -> PrInfo {
    PrInfo { repo: "Org/orpheus-client-electron".into(), number: n, title: format!("feat(ORPH-{n}): thing {n}"), url: format!("https://github.com/o/r/pull/{n}"), extra, ..PrInfo::default() }
}

fn snap(open: Vec<PrInfo>) -> PrSnapshot {
    PrSnapshot { open, viewer: "me".into(), fetched_at: NOW, ..PrSnapshot::default() }
}

fn build_with(s: &PrSnapshot, st: &FeedState, causes: &Causes) -> (FeedView, Option<CommentPopup>) {
    build(&FeedInput { snap: s, st, causes, waiting: &[], now: NOW, lang: Lang::German })
}

#[test]
fn only_human_comments_newer_than_my_last_activity_need_me() {
    let x = PrExtra {
        last_commit_at: NOW - 500.0,
        comments: vec![c("old", "reviewer1", false, NOW - 900.0), c("bot", "tool", true, NOW - 100.0), c("new", "reviewer1", false, NOW - 60.0)],
        threads: vec![
            Thread { id: "t1".into(), resolved: false, head: Some(c("h", "claude", true, NOW - 800.0)), tail: vec![c("reply", "reviewer2", false, NOW - 30.0)] },
            Thread { id: "t2".into(), resolved: true, head: Some(c("r", "reviewer2", false, NOW - 20.0)), tail: vec![] },
        ],
        reviews: vec![Review { comment: Comment { text: String::new(), ..c("ok", "reviewer1", false, NOW - 10.0) }, state: "APPROVED".into(), ..Review::default() }],
        ..PrExtra::default()
    };
    let p = pr(1, x);
    let list = pending(&p, "me");
    assert_eq!(list.iter().map(|p| p.comment.id.as_str()).collect::<Vec<_>>(), ["new", "reply"]);
    assert_eq!((list[1].verb, list[1].thread.as_str()), (Verb::InThread, "t1"));
    let (view, popup) = build_with(&snap(vec![p]), &FeedState::fresh(1.0), &Causes::new());
    let card = &view.needs[0];
    assert_eq!((card.login.as_str(), card.initials.as_str(), card.more, card.label.as_str()), ("reviewer2", "RE", 1, "client#1"));
    assert!(card.can_delegate);
    assert_eq!(popup.unwrap().card.key, card.key);
}

#[test]
fn a_draft_needs_nobody_and_pops_up_nothing() {
    let x = PrExtra { comments: vec![c("q", "reviewer1", false, NOW - 60.0)], ..PrExtra::default() };
    let p = PrInfo { draft: true, ..pr(1, x) };
    let (view, popup) = build_with(&snap(vec![p]), &FeedState::fresh(1.0), &Causes::new());
    assert!(view.needs.is_empty() && popup.is_none());
}

#[test]
fn my_reply_clears_the_card() {
    let x = PrExtra { comments: vec![c("q", "reviewer1", false, NOW - 60.0), c("a", "me", false, NOW - 30.0)], ..PrExtra::default() };
    let (view, popup) = build_with(&snap(vec![pr(1, x)]), &FeedState::fresh(1.0), &Causes::new());
    assert!(view.needs.is_empty() && popup.is_none());
}

#[test]
fn later_hides_until_a_newer_comment_and_the_popup_shows_each_comment_once() {
    let x = PrExtra { comments: vec![c("q", "reviewer1", false, NOW - 60.0)], ..PrExtra::default() };
    let p = pr(1, x.clone());
    let mut st = FeedState::fresh(1.0);
    st.snoozed.insert(p.key(), NOW - 60.0);
    let (view, popup) = build_with(&snap(vec![p.clone()]), &st, &Causes::new());
    assert!(view.needs.is_empty() && popup.is_none());
    let mut y = x;
    y.comments.push(c("q2", "reviewer1", false, NOW - 5.0));
    let (view, _) = build_with(&snap(vec![pr(1, y.clone())]), &st, &Causes::new());
    assert_eq!(view.needs.len(), 1, "neuer Kommentar holt die Karte zurück");
    let mut seen = FeedState::fresh(1.0);
    seen.notified.insert(p.key(), NOW - 5.0);
    let (view, popup) = build_with(&snap(vec![pr(1, y)]), &seen, &Causes::new());
    assert_eq!((view.needs.len(), popup.is_none()), (1, true), "gemeldet: Karte bleibt, Fenster nicht");
}

#[test]
fn comments_from_before_the_first_start_never_pop_up() {
    let x = PrExtra { comments: vec![c("q", "reviewer1", false, NOW - 60.0)], ..PrExtra::default() };
    let (view, popup) = build_with(&snap(vec![pr(1, x)]), &FeedState::fresh(NOW - 10.0), &Causes::new());
    assert_eq!((view.needs.len(), popup.is_none()), (1, true));
}

#[test]
fn a_draft_replaces_the_delegate_button() {
    let x = PrExtra { comments: vec![c("q", "reviewer1", false, NOW - 60.0)], ..PrExtra::default() };
    let p = pr(1, x);
    let mut st = FeedState::fresh(1.0);
    st.drafts.insert(p.key(), Draft { text: " Done in abc123. ".into(), thread_id: "t".into(), ..Draft::default() });
    let (view, _) = build_with(&snap(vec![p]), &st, &Causes::new());
    let card = &view.needs[0];
    assert!(!card.can_delegate);
    assert_eq!(card.draft.as_ref().map(|d| (d.text.as_str(), d.thread)), Some(("Done in abc123.", true)));
}

#[test]
fn the_timeline_shows_merges_ci_reviews_conflicts_and_deck_actions_newest_first() {
    let red = PrExtra { checks: vec![Check { failed: true, done_at: NOW - 300.0, ..Check::default() }], ..PrExtra::default() };
    let sec = PrExtra { checks: vec![Check { failed: true, done_at: NOW - 200.0, ..Check::default() }], ..PrExtra::default() };
    let rev = PrExtra {
        checks: vec![Check { done_at: NOW - 100.0, ..Check::default() }],
        reviews: vec![Review { comment: c("rv", "reviewer1", false, NOW - 50.0), state: "APPROVED".into(), ..Review::default() }],
        conflicting: true,
        ..PrExtra::default()
    };
    let mut s = snap(vec![PrInfo { ci: CiState::Failure, ..pr(1, red) }, PrInfo { ci: CiState::Failure, ..pr(2, sec) }, PrInfo { ci: CiState::Success, ..pr(3, rev) }]);
    s.merged = vec![
        MergedPr { repo: "Org/orpheus-management-api".into(), number: 7, title: "fix(ORPH-7): x".into(), merged_at: NOW - 400.0, ..MergedPr::default() },
        MergedPr { repo: "Org/orpheus-management-api".into(), number: 8, merged_at: NOW - 30.0 * 3600.0, ..MergedPr::default() },
        MergedPr { repo: "Org/orpheus-management-api".into(), number: 9, merged_at: NOW - 20.0, ..MergedPr::default() },
    ];
    let mut st = FeedState::fresh(1.0);
    st.conflict_since.insert("Org/orpheus-client-electron#3".into(), NOW - 40.0);
    let deck = |did: Did, at: f64, n: u64| Activity { at, pr: format!("Org/orpheus-management-api#{n}"), label: format!("api#{n}"), url: String::new(), title: String::new(), did, run: String::new() };
    st.record(deck(Did::Merged, NOW - 20.0, 9));
    st.record(deck(Did::AgentStarted(TaskKind::CiRed), NOW - 10.0, 1));
    st.record(deck(Did::JiraMoved("ORPH-9".into(), "Done".into()), NOW - 5.0, 9));
    let causes = Causes::from([("Org/orpheus-client-electron#1".to_string(), Some(CiCause::Unknown)), ("Org/orpheus-client-electron#2".to_string(), Some(CiCause::CodeSecurity))]);
    let (view, _) = build_with(&s, &st, &causes);
    let got: Vec<(FeedKind, &str, &str)> = view.events.iter().map(|e| (e.kind, e.label.as_str(), e.text.as_str())).collect();
    assert_eq!(
        got,
        [
            (FeedKind::Minor, "api#9", "ORPH-9 auf Done"),
            (FeedKind::Agent, "api#1", "Claude gestartet: rote CI untersuchen"),
            (FeedKind::Merged, "api#9", "gemergt von der Automatik"),
            (FeedKind::Problem, "client#3", "Konflikt entstanden"),
            (FeedKind::Review, "client#3", "Approve von reviewer1"),
            (FeedKind::Minor, "client#3", "CI grün geworden"),
            (FeedKind::Minor, "client#2", "CI rot, erklärt"),
            (FeedKind::Problem, "client#1", "CI rot geworden"),
            (FeedKind::Merged, "api#7", "gemergt"),
        ]
    );
    assert_eq!(view.events[6].detail, "Code Security ist aus");
}

#[test]
fn auto_rows_say_what_the_automation_waits_for() {
    let p = PrInfo { ci: CiState::Running, ..pr(1, PrExtra::default()) };
    let mut st = FeedState::fresh(1.0);
    st.hold.insert(p.key());
    let (view, _) = build_with(&snap(vec![p]), &st, &Causes::new());
    assert_eq!((view.auto[0].state.as_str(), view.auto[0].held), ("gehalten, wird nicht gemergt", true));
}

#[test]
fn small_text_helpers() {
    assert_eq!(initials("reviewer-one"), "RO");
    assert_eq!(initials("lx"), "LX");
    assert_eq!(excerpt("a\n b   c", 10), "a b c");
    assert_eq!(excerpt("abcdefghijk", 4), "abcd…");
    assert_eq!(since_text(250.0, Lang::German), "seit 4 Min.");
    assert_eq!(since_text(7300.0, Lang::English), "for 2 h");
}

#[test]
fn feed_texts_contain_no_dashes() {
    let x = PrExtra { comments: vec![c("q", "reviewer1", false, NOW - 60.0)], ..PrExtra::default() };
    for lang in [Lang::German, Lang::English] {
        let s = snap(vec![pr(1, x.clone())]);
        let st = FeedState::fresh(1.0);
        let text = format!("{:?}", build(&FeedInput { snap: &s, st: &st, causes: &Causes::new(), waiting: &[], now: NOW, lang }));
        assert!(!text.contains('—') && !text.contains('–'), "{text}");
    }
}
