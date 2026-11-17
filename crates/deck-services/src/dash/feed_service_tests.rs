//! Tests zum Feed-Dienst gegen Attrappen: kein echtes gh, acli, git oder claude.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use deck_core::dash::bg_tasks::TaskKind;
use deck_core::dash::feed_state::{Did, RunStatus};
use deck_core::dash::pr_extra::{Comment, PrExtra, Review, Thread};
use deck_core::dash::prs::{CiState, MergedPr, PrInfo};

use super::*;
use crate::dash::agents::AgentProc;
use crate::dash::feed_actions::FeedReply;

#[derive(Default)]
struct World {
    calls: Mutex<Vec<String>>,
    jira_status: Mutex<Option<Result<String, String>>>,
    done: Arc<AtomicBool>,
    snap: Mutex<PrSnapshot>,
    now: AtomicU64,
}

impl World {
    fn call(&self, c: String) {
        lock(&self.calls).push(c);
    }
    fn calls(&self) -> Vec<String> {
        lock(&self.calls).clone()
    }
    fn set_now(&self, t: f64) {
        self.now.store(t.to_bits(), Ordering::SeqCst);
    }
}

struct Fake(Arc<World>);

impl GhApi for Fake {
    fn graphql(&self, query: &str) -> Result<String, String> {
        if query.contains("d0: repository") {
            self.0.call("deps".into());
            return Ok(r#"{"data":{"d0":{"pullRequest":{"state":"OPEN"}}}}"#.into());
        }
        self.0.call(format!("search {}", query.contains("in:title ORPH-5")));
        Ok(r#"{"data":{"s":{"nodes":[{"title":"feat(ORPH-5): a","state":"MERGED"}]}}}"#.into())
    }
    fn job_log(&self, _repo: &str, job: u64) -> Result<String, String> {
        self.0.call(format!("log {job}"));
        Ok("##[error]Code Security must be enabled for this repository".into())
    }
}

impl GhWrite for Fake {
    fn add_reviewer(&self, url: &str, login: &str) -> Result<(), String> {
        self.0.call(format!("reviewer {url} {login}"));
        Ok(())
    }
    fn add_assignee_me(&self, url: &str) -> Result<(), String> {
        self.0.call(format!("assignee {url}"));
        Ok(())
    }
    fn merge(&self, url: &str) -> Result<(), String> {
        self.0.call(format!("merge {url}"));
        Ok(())
    }
    fn delete_branch(&self, repo: &str, branch: &str) -> Result<(), String> {
        self.0.call(format!("delete {repo} {branch}"));
        Ok(())
    }
    fn reply_thread(&self, thread: &str, body: &str) -> Result<(), String> {
        self.0.call(format!("reply {thread} {body}"));
        Ok(())
    }
    fn resolve_thread(&self, thread: &str) -> Result<(), String> {
        self.0.call(format!("resolve {thread}"));
        Ok(())
    }
    fn comment(&self, url: &str, body: &str) -> Result<(), String> {
        self.0.call(format!("comment {url} {body}"));
        Ok(())
    }
}

impl JiraApi for Fake {
    fn status(&self, key: &str) -> Result<String, String> {
        self.0.call(format!("jira status {key}"));
        lock(&self.0.jira_status).clone().unwrap_or_else(|| Ok(String::new()))
    }
    fn transition(&self, key: &str, status: &str) -> Result<(), String> {
        self.0.call(format!("jira {key} {status}"));
        Ok(())
    }
}

struct FakeProc(Arc<AtomicBool>, Arc<World>);

impl AgentProc for FakeProc {
    fn poll(&mut self) -> Option<i32> {
        self.0.load(Ordering::SeqCst).then_some(0)
    }
    fn kill(&mut self) {
        self.1.call("kill".into());
    }
}

impl AgentRunner for Fake {
    fn start(&self, dir: &Path, prompt: &str, out: &Path, _extra: &Path) -> Result<Box<dyn AgentProc>, String> {
        self.0.call(format!("claude in {}", dir.file_name().unwrap().to_string_lossy()));
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        std::fs::write(out, "{\"type\":\"result\",\"is_error\":false,\"result\":\"Fixed in abc.\"}\n").unwrap();
        if let Some(rest) = prompt.split("into the file `").nth(1) {
            std::fs::write(rest.split('`').next().unwrap(), "Done in abc.").unwrap();
        }
        Ok(Box::new(FakeProc(self.0.done.clone(), self.0.clone())))
    }
}

impl Git for Fake {
    fn run(&self, args: &[&str], _cwd: &Path) -> (i32, String) {
        match args {
            ["worktree", "list", ..] => (0, "worktree C:/R/client\nbranch refs/heads/main\n\nworktree C:/R/wt/7-client\nbranch refs/heads/feat/7\n".into()),
            _ => (0, String::new()),
        }
    }
}

const NOW: f64 = 1_000_000.0;

fn ready(n: u64) -> PrInfo {
    let approve = Review { comment: Comment { author: "reviewer1".into(), at: NOW - 500.0, ..Comment::default() }, state: "APPROVED".into(), ..Review::default() };
    PrInfo {
        repo: "Org/orpheus-client-electron".into(),
        number: n,
        title: format!("feat(ORPH-{n}): x"),
        url: format!("https://github.com/o/r/pull/{n}"),
        head_ref: format!("feat/{n}"),
        ci: CiState::Success,
        extra: PrExtra { merge_state: "CLEAN".into(), head_oid: "h".into(), assignees: vec!["me".into()], reviews: vec![approve], ..PrExtra::default() },
        ..PrInfo::default()
    }
}

fn setup(dir: &Path, since: f64) -> (Arc<World>, FeedService) {
    let w = Arc::new(World::default());
    *lock(&w.jira_status) = Some(Ok("waiting for review".into()));
    w.set_now(NOW);
    let st = FeedState::fresh(since);
    std::fs::write(dir.join(STATE_FILE), st.to_json()).unwrap();
    let (ws, wn) = (w.clone(), w.clone());
    let deps = FeedDeps {
        gh: Arc::new(Fake(w.clone())),
        write: Arc::new(Fake(w.clone())),
        jira: Arc::new(Fake(w.clone())),
        runner: Arc::new(Fake(w.clone())),
        git: Arc::new(Fake(w.clone())),
        prs: Arc::new(move || lock(&ws.snap).clone()),
        now: Arc::new(move || f64::from_bits(wn.now.load(Ordering::SeqCst))),
    };
    let svc = FeedService::new(deps, dir, &dir.join("feed"), LogFile::new(dir.join(LOG_FILE), 100_000, Rotate::Truncate));
    std::fs::create_dir_all(dir.join("orpheus-client-electron").join(".git")).unwrap();
    svc.set_config(FeedConfig { reviewer: "reviewer1".into(), org: "Org".into(), prefix: "ORPH".into(), repos_root: PathBuf::from(dir) });
    (w, svc)
}

fn publish(w: &World, open: Vec<PrInfo>, merged: Vec<MergedPr>, at: f64) {
    *lock(&w.snap) = PrSnapshot { open, merged, viewer: "me".into(), fetched_at: at, ..PrSnapshot::default() };
}

#[test]
fn a_ready_pr_is_merged_its_branch_deleted_and_its_worktree_handed_to_cleanup() {
    let d = tempfile::tempdir().unwrap();
    let (w, svc) = setup(d.path(), 1.0);
    let mut p = ready(1);
    p.waiting_on = vec!["reviewer1".into()];
    publish(&w, vec![p], vec![], NOW);
    svc.round_now();
    assert_eq!(w.calls(), ["merge https://github.com/o/r/pull/1", "delete Org/orpheus-client-electron feat/1"]);
    let wanted = vec![("Org/orpheus-client-electron".to_string(), "feat/1".to_string())];
    assert_eq!(svc.clean_wanted(NOW), wanted);
    assert!(svc.clean_wanted(NOW + CLEAN_WAIT_S + 1.0).is_empty(), "nach einer Stunde aufgegeben");
    let saved = FeedState::from_json(std::fs::read_to_string(d.path().join(STATE_FILE)).ok().as_deref(), NOW);
    assert!(saved.activity.iter().any(|a| a.did == Did::Merged));
    svc.round_now();
    assert_eq!(w.calls().len(), 2, "ohne neuen Abruf keine zweite Runde Mechanik");
}

#[test]
fn the_emergency_stop_and_the_hold_list_keep_everything_still() {
    let d = tempfile::tempdir().unwrap();
    let (w, svc) = setup(d.path(), 1.0);
    let p = ready(1);
    svc.action(&format!("hold:{}", p.key())).unwrap();
    publish(&w, vec![p.clone()], vec![], NOW);
    svc.round_now();
    assert!(!w.calls().iter().any(|c| c.starts_with("merge")));
    svc.action("automation:off").unwrap();
    svc.action(&format!("release:{}", p.key())).unwrap();
    publish(&w, vec![p], vec![], NOW + 1.0);
    svc.round_now();
    assert!(w.calls().iter().all(|c| !c.starts_with("merge") && !c.starts_with("reviewer")));
    assert!(!svc.state().automation);
}

#[test]
fn a_red_ci_explained_by_code_security_is_merged_after_one_log_fetch() {
    let d = tempfile::tempdir().unwrap();
    let (w, svc) = setup(d.path(), 1.0);
    let mut p = PrInfo { ci: CiState::Failure, ..ready(2) };
    p.extra.merge_state = "UNSTABLE".into();
    p.extra.checks = vec![deck_core::dash::pr_extra::Check { name: "codeql / Analyze".into(), workflow: "CodeQL".into(), job_id: 42, failed: true, done_at: NOW - 60.0, ..Default::default() }];
    publish(&w, vec![p.clone()], vec![], NOW);
    svc.round_now();
    publish(&w, vec![p], vec![], NOW + 1.0);
    svc.round_now();
    let calls = w.calls();
    assert_eq!(calls.iter().filter(|c| *c == "log 42").count(), 1, "{calls:?}");
    assert!(calls.contains(&"merge https://github.com/o/r/pull/2".to_string()));
}

#[test]
fn a_fresh_merge_moves_the_ticket_to_done_and_a_jira_failure_becomes_a_feed_entry() {
    let d = tempfile::tempdir().unwrap();
    let (w, svc) = setup(d.path(), 1.0);
    let m = |n: u64, t: &str| MergedPr { repo: "Org/r".into(), number: n, title: t.into(), merged_at: NOW - 100.0, ..MergedPr::default() };
    publish(&w, vec![], vec![m(5, "feat(ORPH-5): a")], NOW);
    svc.round_now();
    assert!(w.calls().contains(&"jira ORPH-5 Done".to_string()), "{:?}", w.calls());
    *lock(&w.jira_status) = Some(Err("acli: not logged in".into()));
    publish(&w, vec![], vec![m(5, "feat(ORPH-5): a"), m(6, "fix(ORPH-6): b")], NOW + 1.0);
    svc.round_now();
    let st = svc.state();
    assert!(st.activity.iter().any(|a| a.did == Did::JiraMoved("ORPH-5".into(), "Done".into())));
    assert!(st.activity.iter().any(|a| matches!(&a.did, Did::JiraFailed(t, why) if t == "ORPH-6" && why.contains("not logged in"))));
    assert_eq!(w.calls().iter().filter(|c| c.as_str() == "jira ORPH-5 Done").count(), 1, "jedes Ticket einmal");
}

fn with_bot_thread(n: u64) -> PrInfo {
    let mut p = PrInfo { ci: CiState::Running, ..ready(n) };
    p.extra.threads = vec![Thread { id: "t".into(), resolved: false, head: Some(Comment { author: "claude".into(), bot: true, at: NOW - 30.0, ..Comment::default() }), tail: vec![] }];
    p.open_threads = 1;
    p
}

#[test]
fn a_new_bot_finding_starts_a_headless_agent_in_the_existing_worktree_and_its_report_lands_in_the_feed() {
    let d = tempfile::tempdir().unwrap();
    let (w, svc) = setup(d.path(), 1.0);
    publish(&w, vec![with_bot_thread(7)], vec![], NOW);
    svc.round_now();
    assert!(w.calls().contains(&"claude in 7-client".to_string()), "{:?}", w.calls());
    assert_eq!(svc.state().running().count(), 1);
    w.done.store(true, Ordering::SeqCst);
    w.set_now(NOW + 300.0);
    svc.round_now();
    let st = svc.state();
    assert_eq!((st.runs[0].status, st.runs[0].report.as_str()), (RunStatus::Done, "Fixed in abc."));
    assert!(st.activity.iter().any(|a| a.did == Did::AgentDone(TaskKind::BotFindings)));
    publish(&w, vec![with_bot_thread(7)], vec![], NOW + 400.0);
    svc.round_now();
    assert_eq!(w.calls().iter().filter(|c| c.starts_with("claude")).count(), 1, "derselbe Fund nur einmal");
}

#[test]
fn no_agent_starts_where_a_deck_session_works_and_the_feed_says_so() {
    let d = tempfile::tempdir().unwrap();
    let (w, svc) = setup(d.path(), 1.0);
    svc.set_in_use(vec!["C:\\R\\wt\\7-client".into()]);
    publish(&w, vec![with_bot_thread(7)], vec![], NOW);
    svc.round_now();
    assert!(!w.calls().iter().any(|c| c.starts_with("claude")));
    let (view, _) = svc.view(NOW, Lang::German);
    assert_eq!(view.agents[0].state, "wartet, Session aktiv");
}

#[test]
fn an_agent_over_the_time_limit_is_killed() {
    let d = tempfile::tempdir().unwrap();
    let (w, svc) = setup(d.path(), 1.0);
    publish(&w, vec![with_bot_thread(7)], vec![], NOW);
    svc.round_now();
    w.set_now(NOW + deck_core::dash::bg_tasks::TIME_LIMIT_S + 1.0);
    svc.round_now();
    assert!(w.calls().contains(&"kill".to_string()));
    assert_eq!(svc.state().runs[0].status, RunStatus::TimedOut);
}

#[test]
fn delegate_drafts_a_reply_that_is_only_sent_on_click() {
    let d = tempfile::tempdir().unwrap();
    let (w, svc) = setup(d.path(), 1.0);
    let mut p = PrInfo { ci: CiState::Running, ..ready(7) };
    let human = Comment { id: "c9".into(), author: "reviewer1".into(), at: NOW - 20.0, url: "https://github.com/o/r/pull/7#c9".into(), ..Comment::default() };
    p.extra.threads = vec![Thread { id: "th".into(), resolved: false, head: Some(human), tail: vec![] }];
    publish(&w, vec![p.clone()], vec![], NOW);
    svc.round_now();
    assert!(!w.calls().iter().any(|c| c.starts_with("claude")), "Menschen nie automatisch");
    let reply = svc.action(&format!("delegate:{}", p.key())).unwrap();
    assert!(matches!(reply, FeedReply::Say(_)));
    w.done.store(true, Ordering::SeqCst);
    svc.round_now();
    let st = svc.state();
    assert_eq!(st.drafts.get(&p.key()).map(|d| (d.text.as_str(), d.thread_id.as_str())), Some(("Done in abc.", "th")));
    assert!(!w.calls().iter().any(|c| c.starts_with("reply")), "nichts gesendet vor dem Klick");
    assert!(matches!(svc.action(&format!("draftEdit:{}", p.key())).unwrap(), FeedReply::Open(_)));
    svc.action(&format!("draftSend:{}", p.key())).unwrap();
    let calls = w.calls();
    assert!(calls.contains(&"reply th Done in abc.".to_string()) && calls.contains(&"resolve th".to_string()));
    assert!(svc.state().drafts.is_empty());
}

#[test]
fn later_hides_the_card_and_closes_the_popup() {
    let d = tempfile::tempdir().unwrap();
    let (w, svc) = setup(d.path(), 1.0);
    let mut p = ready(8);
    p.extra.comments = vec![Comment { id: "c".into(), author: "reviewer1".into(), at: NOW - 10.0, ..Comment::default() }];
    publish(&w, vec![p.clone()], vec![], NOW);
    let (view, popup) = svc.view(NOW, Lang::German);
    assert_eq!((view.needs.len(), popup.is_some()), (1, true));
    svc.action(&format!("popupClose:{}", p.key())).unwrap();
    let (view, popup) = svc.view(NOW, Lang::German);
    assert_eq!((view.needs.len(), popup.is_some()), (1, false));
    svc.action(&format!("later:{}", p.key())).unwrap();
    assert!(svc.view(NOW, Lang::German).0.needs.is_empty());
    assert!(svc.action("agentAbort:nope").is_err());
    assert!(FeedService::handles("draftSend:x") && !FeedService::handles("approve:x"));
}

#[test]
fn a_failed_start_is_reported_once_and_not_retried_every_round() {
    let d = tempfile::tempdir().unwrap();
    let (w, svc) = setup(d.path(), 1.0);
    let mut p = with_bot_thread(7);
    p.repo = "Org/no-checkout-here".into();
    publish(&w, vec![p], vec![], NOW);
    svc.round_now();
    svc.round_now();
    assert!(!w.calls().iter().any(|c| c.starts_with("claude")));
    let failed = svc.state().activity.iter().filter(|a| matches!(a.did, Did::AgentFailed(TaskKind::BotFindings, _))).count();
    assert_eq!(failed, 1);
}

#[test]
fn a_merge_after_dependency_that_is_still_open_holds_the_merge() {
    let d = tempfile::tempdir().unwrap();
    let (w, svc) = setup(d.path(), 1.0);
    let mut p = ready(1);
    p.extra.body = "Merge after: orpheus-management-api#12".into();
    publish(&w, vec![p], vec![], NOW);
    svc.round_now();
    let calls = w.calls();
    assert!(calls.contains(&"deps".to_string()) && !calls.iter().any(|c| c.starts_with("merge")), "{calls:?}");
    let (view, _) = svc.view(NOW, Lang::German);
    assert_eq!(view.auto[0].state, "wartet auf api#12");
}

#[test]
fn a_draft_gets_no_agent_no_reviewer_no_assignee_and_no_merge() {
    let d = tempfile::tempdir().unwrap();
    let (w, svc) = setup(d.path(), 1.0);
    let mut p = PrInfo { draft: true, ..with_bot_thread(7) };
    p.extra.assignees.clear();
    p.extra.conflicting = true;
    p.extra.comments = vec![Comment { id: "c".into(), author: "reviewer1".into(), at: NOW - 10.0, ..Comment::default() }];
    publish(&w, vec![p.clone()], vec![], NOW);
    svc.round_now();
    publish(&w, vec![p.clone()], vec![], NOW + 1.0);
    svc.round_now();
    assert!(w.calls().is_empty(), "{:?}", w.calls());
    assert!(svc.action(&format!("delegate:{}", p.key())).unwrap_err().contains("Draft"));
}
