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

/// Feste Tokens eines umstellbaren Kontos (`None` = unbekannt, wie vor dem Konto-Feld).
pub(crate) struct FixedTokens(pub Vec<&'static str>, pub Mutex<Option<String>>);

impl FixedTokens {
    pub fn new(tokens: Vec<&'static str>) -> FixedTokens {
        FixedTokens(tokens, Mutex::new(None))
    }

    pub fn of(account: &str, tokens: Vec<&'static str>) -> FixedTokens {
        FixedTokens(tokens, Mutex::new(Some(account.into())))
    }
}

impl TokenSource for FixedTokens {
    fn tokens(&self, _: bool) -> Result<Vec<String>, String> {
        if self.0.is_empty() { Err("CLI: weg; Desktop: weg".into()) } else { Ok(self.0.iter().map(|s| s.to_string()).collect()) }
    }

    fn account(&self) -> Option<String> {
        self.1.lock().unwrap().clone()
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
    fn get(&self, url: &str, _: &str) -> Result<HttpReply, Transport> {
        assert_eq!(url, super::super::fetch::USAGE_URL, "nur der Usage-Endpoint");
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
    rig_with(FixedTokens::new(vec!["tok-a", "tok-b"]), replies)
}

fn rig_with(tokens: FixedTokens, replies: Vec<Result<HttpReply, Transport>>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let http = Arc::new(ScriptHttp::new(replies));
    let lock = Arc::new(FakeLock { busy: AtomicBool::new(false) });
    let poller = SharedPoller::new(dir.path().to_path_buf(), lock.clone(), Arc::new(tokens), http.clone(), Arc::new(|| NOW), Arc::new(|| 0.0));
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

// ── Zwei Konten (2026-11-08): der Cache trägt sein Konto ─────────────────────────────

/// Cache eines Kontos, der noch eine Minute lebt.
fn living_cache_of(r: &Rig, account: &str) {
    let c = json!({"data": {"a": 40}, "data_ts": NOW - 10.0, "error": null, "next_ts": NOW + 60.0, "n429": 2, "interval": 400, "account": account});
    std::fs::write(r.poller.cache_path(), c.to_string()).unwrap();
}

#[test]
fn every_write_carries_the_account_in_the_format_of_the_python_poller() {
    // Ohne `account` verwirft der Tray (Python) den Cache bei jedem Blick als fremd und
    // fragt selbst nach. Darum JEDER Schreibweg, und als letztes Feld wie in Python.
    let ok = reply(200, None, "{}");
    let cases = [ok, reply(401, None, ""), reply(429, None, ""), reply(503, None, ""), Err(Transport::Network)];
    for answer in cases {
        let r = rig_with(FixedTokens::of("a@x.de", vec!["t"]), vec![answer.clone()]);
        let v = r.poller.get_usage(false);
        let c = cache(&r);
        assert_eq!(c["account"], "a@x.de", "{answer:?}");
        assert_eq!(c.as_object().unwrap().keys().next_back().map(String::as_str), Some("account"));
        assert_eq!(v.account.as_deref(), Some("a@x.de"));
    }
    let unknown = rig(vec![reply(200, None, "{}")]);
    unknown.poller.get_usage(false);
    assert!(cache(&unknown)["account"].is_null(), "unbekannt steht als null drin, wie in Python");
}

#[test]
fn a_cache_of_the_same_account_lives_on_without_a_poll() {
    let r = rig_with(FixedTokens::of("a@x.de", vec!["t"]), vec![reply(200, None, "{}")]);
    living_cache_of(&r, "a@x.de");
    let v = r.poller.get_usage(false);
    assert_eq!(r.http.calls.load(Ordering::SeqCst), 0);
    assert_eq!((v.data, v.account.as_deref()), (Some(json!({"a": 40})), Some("a@x.de")));
}

#[test]
fn a_cache_of_another_account_is_discarded_and_only_the_interval_stays() {
    let r = rig_with(FixedTokens::of("b@x.de", vec!["t"]), vec![reply(200, None, r#"{"b": 1}"#)]);
    living_cache_of(&r, "a@x.de");
    let v = r.poller.get_usage(false);
    assert_eq!(r.http.calls.load(Ordering::SeqCst), 1, "die fremde Frist gilt nicht");
    let c = cache(&r);
    assert_eq!((c["account"].as_str(), c["data"].clone()), (Some("b@x.de"), json!({"b": 1})));
    assert_eq!((c["n429"].as_u64(), c["interval"].as_f64()), (Some(0), Some(360.0)), "der gelernte Takt (400) bleibt");
    assert_eq!(v.account.as_deref(), Some("b@x.de"));
    let log = std::fs::read_to_string(r.poller.cache_path().with_file_name(LOG_FILE)).unwrap();
    assert!(log.contains("Kontowechsel a@x.de -> b@x.de: Cache verworfen"), "{log}");
}

#[test]
fn after_a_switch_a_failed_poll_shows_no_number_of_the_old_account() {
    let r = rig_with(FixedTokens::of("b@x.de", vec!["t"]), vec![reply(429, None, "")]);
    living_cache_of(&r, "a@x.de");
    let v = r.poller.get_usage(false);
    assert_eq!((v.data, v.data_ts, v.error.as_deref()), (None, None, Some("Rate-Limit")));
    assert_eq!(cache(&r)["n429"], 1, "der Sperrzähler des alten Tokens zählt nicht mit");
}

#[test]
fn while_another_process_polls_no_number_beats_a_foreign_one() {
    let r = rig_with(FixedTokens::of("b@x.de", vec!["t"]), vec![reply(200, None, "{}")]);
    living_cache_of(&r, "a@x.de");
    r.lock.busy.store(true, Ordering::SeqCst);
    let v = r.poller.get_usage(true);
    assert_eq!(r.http.calls.load(Ordering::SeqCst), 0);
    assert_eq!((v.data, v.account.as_deref(), v.error.as_deref()), (None, Some("b@x.de"), Some(SWITCH_ERROR)));
    let same = rig_with(FixedTokens::of("a@x.de", vec!["t"]), vec![reply(200, None, "{}")]);
    living_cache_of(&same, "a@x.de");
    same.lock.busy.store(true, Ordering::SeqCst);
    assert_eq!(same.poller.get_usage(true).data, Some(json!({"a": 40})), "dasselbe Konto: letzter Wert");
}

#[test]
fn a_cache_without_account_is_foreign_once_the_account_is_known() {
    // So sah jeder Cache vor dem Konto-Feld aus (auch der des alten Rust-Decks).
    let r = rig_with(FixedTokens::of("a@x.de", vec!["t"]), vec![reply(200, None, "{}")]);
    std::fs::write(r.poller.cache_path(), format!(r#"{{"data": {{"a": 1}}, "data_ts": 5.0, "next_ts": {}}}"#, NOW + 60.0)).unwrap();
    r.poller.get_usage(false);
    assert_eq!(r.http.calls.load(Ordering::SeqCst), 1);
    assert_eq!(cache(&r)["account"], "a@x.de");
}
