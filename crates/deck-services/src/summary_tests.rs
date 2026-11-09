//! Tests zu `summary.rs`: Begrenzung, Cache, Reihenfolge der Meldungen. `claude` wird
//! nie gestartet, die Attrappe antwortet.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

use deck_core::claude::summarize::enc_cwd;

use super::*;

const CWD: &str = r"C:\Repos\alpha";

/// Antwortet mit festem Text (oder `None`), zählt Aufrufe und die höchste Gleichzeitigkeit.
struct FakeClaude {
    answer: Option<&'static str>,
    pause: Duration,
    calls: AtomicUsize,
    running: AtomicUsize,
    peak: AtomicUsize,
}

impl FakeClaude {
    fn new(answer: Option<&'static str>, pause: Duration) -> Arc<FakeClaude> {
        Arc::new(FakeClaude { answer, pause, calls: AtomicUsize::new(0), running: AtomicUsize::new(0), peak: AtomicUsize::new(0) })
    }
}

impl ClaudeRunner for FakeClaude {
    fn run(&self, prompt: &str, model: &str, _: Duration, _: &Path) -> Option<String> {
        assert!(prompt.contains("--- Chat ---") && model == "haiku");
        self.calls.fetch_add(1, Ordering::SeqCst);
        let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(now, Ordering::SeqCst);
        std::thread::sleep(self.pause);
        self.running.fetch_sub(1, Ordering::SeqCst);
        self.answer.map(String::from)
    }
}

struct Rig {
    _dir: tempfile::TempDir,
    cfg: SummaryConfig,
}

fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let mut cfg = SummaryConfig::standard(&state);
    cfg.projects = dir.path().join("projects");
    cfg.project_key = Some("ABC".into());
    Rig { _dir: dir, cfg }
}

fn transcript(rig: &Rig, sid: &str, bytes: usize) {
    let d = rig.cfg.projects.join(enc_cwd(CWD));
    std::fs::create_dir_all(&d).unwrap();
    let mut text = String::new();
    text += &serde_json::json!({"type": "user", "message": {"content": "Bitte ABC-123 umsetzen: Login reparieren"}}).to_string();
    text += "\n";
    text += &serde_json::json!({"type": "assistant", "message": {"content": [{"type": "text", "text": "Ich schaue mir ABC-123 an."}]}}).to_string();
    text += "\n";
    while text.len() < bytes {
        text += &serde_json::json!({"type": "user", "message": {"content": "weiter mit ABC-123"}}).to_string();
        text += "\n";
    }
    std::fs::write(d.join(format!("{sid}.jsonl")), text).unwrap();
}

#[test]
fn a_summary_is_made_once_and_then_comes_from_the_cache() {
    let r = rig();
    transcript(&r, "s1", 0);
    let fake = FakeClaude::new(Some("Zusammenfassung: Login reparieren."), Duration::ZERO);
    let svc = SummaryService::new(r.cfg.clone(), fake.clone());
    assert_eq!(svc.generate("s1", Some(CWD), Lang::German).as_deref(), Some("Login reparieren."));
    assert_eq!(svc.generate("s1", Some(CWD), Lang::German).as_deref(), Some("Login reparieren."));
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1, "ohne Zuwachs kein zweiter Aufruf");
    assert_eq!(svc.cached_summary("s1").as_deref(), Some("Login reparieren."));
}

#[test]
fn a_language_switch_asks_again_at_once() {
    let r = rig();
    transcript(&r, "s1", 0);
    let fake = FakeClaude::new(Some("Fixing the login."), Duration::ZERO);
    let svc = SummaryService::new(r.cfg.clone(), fake.clone());
    svc.generate("s1", Some(CWD), Lang::German);
    svc.generate("s1", Some(CWD), Lang::English);
    assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
}

#[test]
fn a_failed_run_keeps_the_old_value() {
    let r = rig();
    transcript(&r, "s1", 0);
    SummaryService::new(r.cfg.clone(), FakeClaude::new(Some("Alter Satz."), Duration::ZERO)).generate("s1", Some(CWD), Lang::German);
    let svc = SummaryService::new(r.cfg.clone(), FakeClaude::new(None, Duration::ZERO));
    // Englisch erzwingt einen neuen Lauf; der scheitert, der deutsche Satz bleibt.
    assert_eq!(svc.generate("s1", Some(CWD), Lang::English).as_deref(), Some("Alter Satz."));
}

#[test]
fn without_a_transcript_nothing_is_started() {
    let r = rig();
    let fake = FakeClaude::new(Some("x"), Duration::ZERO);
    assert_eq!(SummaryService::new(r.cfg.clone(), fake.clone()).generate("fehlt", Some(CWD), Lang::German), None);
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn refs_come_first_and_the_summary_closes_the_job() {
    let r = rig();
    transcript(&r, "s1", 0);
    let svc = SummaryService::new(r.cfg.clone(), FakeClaude::new(Some("Login reparieren"), Duration::ZERO));
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    assert!(svc.request("s1", Some(CWD), Lang::German, true, Arc::new(move |sid, info| {
        tx.lock().unwrap().send((sid.to_string(), info)).unwrap();
    })));
    let first = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let second = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(&first, (s, ChatInfo::Refs(refs)) if s == "s1" && refs.ticket == "ABC-123"), "{first:?}");
    assert_eq!(second, ("s1".to_string(), ChatInfo::Summary(Some("Login reparieren".into()))));
}

#[test]
fn the_project_key_from_the_settings_is_used_for_bare_numbers() {
    let r = rig();
    let d = r.cfg.projects.join(enc_cwd(CWD));
    std::fs::create_dir_all(&d).unwrap();
    let line = serde_json::json!({"type": "user", "message": {"content": "Ticket 4116 bitte"}}).to_string();
    std::fs::write(d.join("s9.jsonl"), line).unwrap();
    let svc = SummaryService::new(r.cfg.clone(), FakeClaude::new(None, Duration::ZERO));
    svc.set_project_key(Some(" ORPH "));
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    assert!(svc.request("s9", Some(CWD), Lang::German, false, Arc::new(move |_, info| {
        if let ChatInfo::Refs(refs) = info {
            tx.lock().unwrap().send(refs.ticket).unwrap();
        }
    })));
    assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), "ORPH-4116");
    assert_eq!(svc.cached_refs("s9").ticket, "ORPH-4116");
    svc.set_project_key(Some("ABC"));
    assert_eq!(svc.cached_refs("s9").ticket, "", "für ein anderes Projekt gilt der Cache nicht");
}

#[test]
fn per_session_only_one_job_runs() {
    let r = rig();
    transcript(&r, "s1", 0);
    let svc = SummaryService::new(r.cfg.clone(), FakeClaude::new(Some("x"), Duration::from_millis(300)));
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let cb: InfoCallback = Arc::new(move |_, info| {
        if matches!(info, ChatInfo::Summary(_)) {
            tx.lock().unwrap().send(()).unwrap();
        }
    });
    assert!(svc.request("s1", Some(CWD), Lang::German, true, cb.clone()));
    assert!(!svc.request("s1", Some(CWD), Lang::German, true, cb.clone()), "zweiter Auftrag derselben Session");
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    std::thread::sleep(Duration::from_millis(20));
    assert!(svc.request("s1", Some(CWD), Lang::German, false, cb), "nach dem Ende wieder frei");
}

#[test]
fn at_most_two_claude_runs_at_once() {
    let r = rig();
    let fake = FakeClaude::new(Some("x"), Duration::from_millis(150));
    let svc = SummaryService::new(r.cfg.clone(), fake.clone());
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let cb: InfoCallback = Arc::new(move |_, info| {
        if matches!(info, ChatInfo::Summary(_)) {
            tx.lock().unwrap().send(()).unwrap();
        }
    });
    for i in 0..5 {
        let sid = format!("s{i}");
        transcript(&r, &sid, 0);
        assert!(svc.request(&sid, Some(CWD), Lang::German, true, cb.clone()));
    }
    for _ in 0..5 {
        rx.recv_timeout(Duration::from_secs(10)).unwrap();
    }
    assert_eq!(fake.calls.load(Ordering::SeqCst), 5);
    assert_eq!(fake.peak.load(Ordering::SeqCst), MAX_CONCURRENT);
}

#[test]
fn the_real_runner_starts_nothing_in_tests() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(ClaudeCli.run("x", "haiku", Duration::from_secs(1), d.path()), None);
}
