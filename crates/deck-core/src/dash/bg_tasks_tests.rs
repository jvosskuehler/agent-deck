//! Tests zu `bg_tasks.rs`: wann ein Hintergrund-Agent fällig ist und wann er wartet.

use super::*;
use crate::dash::feed_state::{Run, RunStatus};
use crate::dash::pr_extra::{Check, PrExtra, Thread};

fn c(author: &str, bot: bool, at: f64) -> Comment {
    Comment { id: format!("{author}{at}"), author: author.into(), bot, at, ..Comment::default() }
}

fn pr(n: u64) -> PrInfo {
    PrInfo {
        repo: "Org/orpheus-client-electron".into(),
        number: n,
        title: format!("feat(ORPH-{n}): x"),
        head_ref: "b".into(),
        url: format!("https://github.com/o/r/pull/{n}"),
        ..PrInfo::default()
    }
}

fn with_thread(n: u64, bot: bool, at: f64, resolved: bool) -> PrInfo {
    let t = Thread { id: "t".into(), resolved, head: Some(c("x", bot, at)), tail: vec![] };
    PrInfo { extra: PrExtra { threads: vec![t], ..PrExtra::default() }, ..pr(n) }
}

fn run(pr: &str) -> Run {
    Run {
        id: pr.into(),
        pr: pr.into(),
        label: String::new(),
        url: String::new(),
        title: String::new(),
        task: TaskKind::CiRed,
        started: 0.0,
        ended: 0.0,
        status: RunStatus::Running,
        report: String::new(),
        dir: String::new(),
        comment_id: String::new(),
        thread_id: String::new(),
        draft_path: String::new(),
    }
}

#[test]
fn a_new_unresolved_bot_thread_is_due_but_old_resolved_or_human_ones_are_not() {
    let st = FeedState::fresh(100.0);
    assert_eq!(due_for(&with_thread(1, true, 150.0, false), &st, None).unwrap().task, TaskKind::BotFindings);
    assert!(due_for(&with_thread(1, true, 50.0, false), &st, None).is_none(), "vor dem Start");
    assert!(due_for(&with_thread(1, true, 150.0, true), &st, None).is_none(), "aufgelöst");
    assert!(due_for(&with_thread(1, false, 150.0, false), &st, None).is_none(), "Menschen nie automatisch");
    let mut done = st.clone();
    done.handled_mut("Org/orpheus-client-electron#1").bot = 150.0;
    assert!(due_for(&with_thread(1, true, 150.0, false), &done, None).is_none(), "schon bearbeitet");
}

#[test]
fn a_draft_never_gets_an_agent() {
    let st = FeedState::fresh(100.0);
    let mut p = with_thread(1, true, 150.0, false);
    p.draft = true;
    p.extra.conflicting = true;
    assert!(due_for(&p, &st, Some(CiCause::Unknown)).is_none());
}

#[test]
fn red_ci_of_unknown_cause_is_due_once_per_commit() {
    let mut p = pr(2);
    p.extra.head_oid = "abc".into();
    p.extra.checks = vec![Check { failed: true, done_at: 200.0, ..Check::default() }];
    let mut st = FeedState::fresh(100.0);
    assert_eq!(due_for(&p, &st, Some(CiCause::Unknown)).unwrap().oid, "abc");
    assert!(due_for(&p, &st, Some(CiCause::CodeSecurity)).is_none(), "erklärt");
    st.handled_mut(&p.key()).ci_oid = "abc".into();
    assert!(due_for(&p, &st, Some(CiCause::Unknown)).is_none());
}

#[test]
fn a_conflict_comes_before_red_ci_and_bot_findings() {
    let mut p = with_thread(3, true, 150.0, false);
    p.extra.conflicting = true;
    let mut st = FeedState::fresh(100.0);
    st.conflict_since.insert(p.key(), 160.0);
    assert_eq!(due_for(&p, &st, Some(CiCause::Unknown)).unwrap().task, TaskKind::Conflict);
}

#[test]
fn at_most_two_agents_one_per_pr_and_none_where_a_session_works() {
    let mut st = FeedState::fresh(0.0);
    st.runs.push(run("A#1"));
    let d = |pr: &str| Due { pr: pr.into(), task: TaskKind::BotFindings, mark: 1.0, oid: String::new() };
    let (start, wait) = schedule(vec![d("A#1"), d("B#2"), d("C#3"), d("D#4")], &st, |pr| pr == "B#2");
    assert_eq!(start.iter().map(|x| x.pr.as_str()).collect::<Vec<_>>(), ["C#3"]);
    assert_eq!(wait.iter().map(|(x, h)| (x.pr.as_str(), *h)).collect::<Vec<_>>(), [("B#2", Hold::SessionActive), ("D#4", Hold::Full)]);
}

#[test]
fn the_worktree_follows_the_folder_convention() {
    assert_eq!(worktree_folder(&pr(4116)), "4116-client");
    let mut p = pr(7);
    p.title = "chore: no ticket".into();
    p.repo = "Org/orpheus-management-api".into();
    assert_eq!(worktree_folder(&p), "pr7-api");
    assert_eq!(ticket_of(&pr(5)), "ORPH-5");
}

#[test]
fn prompts_never_merge_and_human_replies_only_draft() {
    let p = pr(9);
    let auto = prompt(TaskKind::BotFindings, &p, None, "");
    assert!(auto.contains("Never merge") && auto.contains("resolve it") && auto.contains("ORPH-9"));
    assert!(auto.contains("CLAUDE.md"), "Konventionen kommen aus der CLAUDE.md, nicht aus dem Deck");
    let target = Comment { url: "https://github.com/o/r/pull/9#c".into(), ..Comment::default() };
    let human = prompt(TaskKind::HumanReply, &p, Some(&target), "C:/d/x.md");
    assert!(human.contains("Do NOT post anything") && human.contains("C:/d/x.md") && human.contains("#c"));
    assert!(!human.contains("resolve it"));
}

#[test]
fn the_report_is_the_last_result_line_of_the_stream() {
    let out = "{\"type\":\"system\"}
{\"type\":\"assistant\"}
{\"type\":\"result\",\"is_error\":false,\"result\":\" Fixed in abc. \"}
";
    assert_eq!(report_from_stream(out), Some((false, "Fixed in abc.".to_string())));
    assert_eq!(report_from_stream("{\"type\":\"result\",\"is_error\":true}"), Some((true, String::new())));
    assert_eq!(report_from_stream("garbage
{\"type\":\"assistant\"}"), None);
}
