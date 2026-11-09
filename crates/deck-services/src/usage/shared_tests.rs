//! Tests zu `shared.rs` (aus `claude-usage-shared/test_usage_poller.py`, „Backoff landet
//! im Cache“, und die Regel „lebt der Cache, wird nicht abgerufen“). Cache und Protokoll
//! liegen in einem Wegwerf-Ordner, kein Netz, keine echte Sperre.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::super::fetch::{HttpReply, Transport};
use super::super::policy::{BACKOFF_MAX, INTERVAL_MIN};
use super::*;

pub(crate) struct FakeLock {
    pub busy: AtomicBool,
}

impl PollLock for FakeLock {
    fn try_lock(&self, _: u32) -> Option<Box<dyn std::any::Any>> {
        (!self.busy.load(Ordering::SeqCst)).then(|| Box::new(()) as Box<dyn std::any::Any>)
    }
}

pub(crate) struct FixedTokens(pub Vec<&'static str>);

impl TokenSource for FixedTokens {
    fn tokens(&self, _: bool) -> Result<Vec<String>, String> {
        if self.0.is_empty() { Err("CLI: weg; Desktop: weg".into()) } else { Ok(self.0.iter().map(|s| s.to_string()).collect()) }
    }
}

/// Antwortet mit einer festen Folge (letzte wiederholt sich) und zählt die Abrufe.
pub(crate) struct ScriptHttp {
    pub replies: Mutex<Vec<Result<HttpReply, Transport>>>,
    pub calls: AtomicUsize,
}

impl ScriptHttp {
    pub fn new(replies: Vec<Result<HttpReply, Transport>>) -> ScriptHttp {
        ScriptHttp { replies: Mutex::new(replies), calls: AtomicUsize::new(0) }
    }
}

impl UsageHttp for ScriptHttp {
    fn get(&self, _: &str) -> Result<HttpReply, Transport> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut r = self.replies.lock().unwrap();
        if r.len() > 1 { r.remove(0) } else { r[0].clone() }
    }
}

pub(crate) fn reply(status: u16, retry_after: Option<&str>, body: &str) -> Result<HttpReply, Transport> {
    Ok(HttpReply { status, retry_after: retry_after.map(String::from), body: body.into() })
}

const NOW: f64 = 1_800_000_000.0;

struct Rig {
    _dir: tempfile::TempDir,
    poller: SharedPoller,
    http: Arc<ScriptHttp>,
    lock: Arc<FakeLock>,
}

fn rig(replies: Vec<Result<HttpReply, Transport>>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let http = Arc::new(ScriptHttp::new(replies));
    let lock = Arc::new(FakeLock { busy: AtomicBool::new(false) });
    let poller = SharedPoller::new(
        dir.path().to_path_buf(),
        lock.clone(),
        Arc::new(FixedTokens(vec!["tok-a", "tok-b"])),
        http.clone(),
        Arc::new(|| NOW),
        Arc::new(|| 0.0),
    );
    Rig { _dir: dir, poller, http, lock }
}

fn cache(r: &Rig) -> Value {
    r.poller.read_cache().expect("Cache geschrieben")
}

#[test]
fn a_long_server_deadline_is_respected() {
    let r = rig(vec![reply(429, Some(&BACKOFF_MAX.to_string()), "")]);
    r.poller.get_usage(false);
    let c = cache(&r);
    assert_eq!(c["error"], "Rate-Limit");
    assert!(c["next_ts"].as_f64().unwrap() - NOW >= BACKOFF_MAX);
}

#[test]
fn a_short_server_deadline_does_not_undercut_the_interval() {
    // „Retry-After: 5“ heißt NICHT, in 5 s wieder anzuklopfen: die Sperre wäre vorbei,
    // unser Budget nicht.
    let r = rig(vec![reply(429, Some("5"), "")]);
    r.poller.get_usage(false);
    assert!(cache(&r)["next_ts"].as_f64().unwrap() - NOW > INTERVAL_MIN);
}

#[test]
fn without_header_the_curve_is_guessed() {
    let r = rig(vec![reply(429, None, "")]);
    r.poller.get_usage(false);
    let c = cache(&r);
    assert!(c["next_ts"].as_f64().unwrap() - NOW >= 90.0);
    assert_eq!(c["n429"], 1);
}

#[test]
fn the_learned_interval_is_shared_in_the_cache() {
    // Beide Tokens 429 (Antworten gelten je Abruf, nicht je Token), danach Erfolg.
    let ok = r#"{"five_hour": {"utilization": 1.0}}"#;
    let r = rig(vec![reply(429, Some("30"), ""), reply(429, Some("30"), ""), reply(200, None, ok)]);
    r.poller.get_usage(false);
    let stretched = cache(&r)["interval"].as_f64().unwrap();
    assert!(stretched > INTERVAL_START);
    for _ in 0..3 {
        r.poller.get_usage(true); // force umgeht die Frist, wie der manuelle Refresh
    }
    let c = cache(&r);
    assert!(c["interval"].as_f64().unwrap() < stretched, "Erfolg muss den Takt verkürzen");
    assert!(c["error"].is_null());
    assert_eq!(c["data_ts"].as_f64(), Some(NOW));
}

#[test]
fn a_network_error_does_not_clear_a_running_backoff() {
    let r = rig(vec![Err(Transport::Network)]);
    std::fs::write(
        r.poller.cache_path(),
        r#"{"data": {"x": 1}, "data_ts": 1.0, "error": "Rate-Limit", "next_ts": 0, "n429": 3}"#,
    )
    .unwrap();
    let v = r.poller.get_usage(false);
    let c = cache(&r);
    assert_eq!(c["n429"], 3, "n429 wurde gelöscht");
    assert!(c["next_ts"].as_f64().unwrap() - NOW > 100.0, "zu kurze Wartezeit nach Netzfehler");
    assert_eq!(c["data"], json!({"x": 1}), "letzter Wert muss stehenbleiben");
    assert_eq!((v.error.as_deref(), v.data_ts), (Some("URLError"), Some(1.0)));
}

#[test]
fn a_living_cache_is_read_not_polled() {
    // Die Regel aus CLAUDE.md: hat der gemeinsame Poller einen frischen Cache, pollt
    // das Deck NICHT selbst nach. Der Endpoint ist eng limitiert.
    let r = rig(vec![reply(200, None, "{}")]);
    std::fs::write(r.poller.cache_path(), format!(r#"{{"data": {{"a": 1}}, "data_ts": 5.0, "error": null, "next_ts": {}}}"#, NOW + 60.0))
        .unwrap();
    let v = r.poller.get_usage(false);
    assert_eq!(r.http.calls.load(Ordering::SeqCst), 0);
    assert_eq!((v.data, v.data_ts), (Some(json!({"a": 1})), Some(5.0)));
}

#[test]
fn while_another_process_polls_the_last_value_is_returned() {
    let r = rig(vec![reply(200, None, "{}")]);
    std::fs::write(r.poller.cache_path(), r#"{"data": {"a": 2}, "data_ts": 7.0, "error": "Rate-Limit", "next_ts": 0}"#).unwrap();
    r.lock.busy.store(true, Ordering::SeqCst);
    let v = r.poller.get_usage(false);
    assert_eq!(r.http.calls.load(Ordering::SeqCst), 0);
    assert_eq!(v.error.as_deref(), Some("Rate-Limit"));
}

#[test]
fn a_due_cache_leads_to_exactly_one_poll_and_a_success_resets_the_counter() {
    let r = rig(vec![reply(200, None, r#"{"limits": []}"#)]);
    std::fs::write(r.poller.cache_path(), r#"{"data": null, "next_ts": 0, "n429": 4, "interval": 400}"#).unwrap();
    r.poller.get_usage(false);
    let c = cache(&r);
    assert_eq!(r.http.calls.load(Ordering::SeqCst), 1);
    assert_eq!((c["n429"].as_u64(), c["interval"].as_f64()), (Some(0), Some(360.0)));
    assert_eq!(c["next_ts"].as_f64(), Some(NOW + 360.0));
}

#[test]
fn a_rejected_token_empties_the_number_and_says_so() {
    let r = rig(vec![reply(401, None, "")]);
    let v = r.poller.get_usage(false);
    assert!(v.data.is_none());
    assert!(v.error.unwrap().contains("ungueltig"));
}

#[test]
fn the_log_names_whose_budget_was_empty() {
    let r = rig(vec![reply(429, None, ""), reply(200, None, "{}")]);
    r.poller.get_usage(false);
    let log = std::fs::read_to_string(r.poller.cache_path().with_file_name(LOG_FILE)).unwrap();
    assert!(log.contains("    CLI-Token -> HTTP 429, naechste Quelle"), "{log}");
    assert!(log.contains("OK (Abruf, Desktop-Token) -> Cache aktualisiert, Takt 270s"), "{log}");
}
