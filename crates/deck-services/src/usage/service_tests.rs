//! Tests zum Usage-Dienst (aus `tests/test_claude_usage.py`: Übernahme aus dem
//! gemeinsamen Poller; dazu Start und Stopp des Threads ohne Netz).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use serde_json::json;

use super::shared::tests::{FakeLock, FixedTokens, ScriptHttp, reply};
use super::*;

fn iso(ts: f64) -> String {
    let secs = ts as i64;
    chrono::DateTime::from_timestamp(secs, 0).unwrap().to_rfc3339()
}

#[test]
fn the_data_time_and_error_of_the_shared_poller_are_taken_over() {
    // Die Naht, an der es riss: bei 429 bleibt der LETZTE Wert stehen und nur der
    // Fehler wird vermerkt. Wer 'data_ts' durch „jetzt“ ersetzt und 'error' verschluckt,
    // macht daraus eine frische, falsche Zahl.
    let now = 1_800_000_000.0;
    let data = json!({"limits": [
        {"kind": "session", "group": "session", "percent": 48, "severity": "normal",
         "resets_at": iso(now - 1800.0), "is_active": true},
        {"kind": "weekly_all", "group": "weekly", "percent": 9, "severity": "normal",
         "resets_at": iso(now + 86400.0), "is_active": false}]});
    let old_ts = now - 3600.0;
    let v = SharedView { data: Some(data), error: Some("Rate-Limit".into()), data_ts: Some(old_ts), ..Default::default() };
    let mut snap = UsageSnapshot::default();
    apply_view(&mut snap, &v, now, Lang::German);
    assert_eq!(snap.ts, Some(old_ts), "die Datenzeit muss die des ABRUFS sein");
    assert_eq!(snap.error.as_deref(), Some("Rate-Limit"), "der Fehler darf nicht verschluckt werden");
    assert_eq!(snap.session_percent, None, "48 % aus dem alten Fenster sind keine Zahl");
    assert_eq!(snap.session_severity, "");
    let week = snap.limits.iter().find(|l| l.kind == "weekly_all").unwrap();
    assert_eq!(week.percent, Some(9), "die Woche läuft weiter und bleibt gültig");
}

#[test]
fn an_invalid_token_hides_the_number() {
    let mut snap = UsageSnapshot { session_percent: Some(40), ..Default::default() };
    let v = SharedView { data: None, error: Some("Token ungueltig – 'claude auth login'".into()), data_ts: None, ..Default::default() };
    apply_view(&mut snap, &v, 0.0, Lang::English);
    assert_eq!(snap.session_percent, None);
    assert_eq!(snap.error.as_deref(), Some("Token invalid, run 'claude auth login'"));
}

#[test]
fn a_rate_limit_without_data_keeps_the_last_number() {
    let mut snap = UsageSnapshot { session_percent: Some(40), ..Default::default() };
    let v = SharedView { data: None, error: Some("Rate-Limit".into()), data_ts: None, ..Default::default() };
    apply_view(&mut snap, &v, 0.0, Lang::German);
    assert_eq!((snap.session_percent, snap.state.as_str()), (Some(40), "error"));
}

#[test]
fn missing_tokens_point_to_the_cli_login() {
    let mut snap = UsageSnapshot::default();
    let v = SharedView { data: None, error: Some("NoTokenError".into()), data_ts: None, ..Default::default() };
    apply_view(&mut snap, &v, 0.0, Lang::German);
    assert!(snap.error.unwrap().contains("claude auth login"));
}

#[test]
fn texts_for_the_surface_have_no_dashes() {
    for (err, lang) in [
        ("Token ungueltig", Lang::German),
        ("NoTokenError", Lang::English),
        ("Rate-Limit", Lang::German),
        ("", Lang::English),
        (shared::SWITCH_ERROR, Lang::German),
        (shared::SWITCH_ERROR, Lang::English),
    ] {
        let mut snap = UsageSnapshot::default();
        apply_view(&mut snap, &SharedView { data: None, error: Some(err.into()), data_ts: None, ..Default::default() }, 0.0, lang);
        let e = snap.error.unwrap();
        assert!(!e.contains('–') && !e.contains('—'), "{e}");
    }
}

#[test]
fn the_number_of_the_old_account_is_dropped() {
    // Kommt nach dem Wechsel erst einmal keine Zahl (gemeinsamer Poller belegt, 429),
    // darf die alte nicht als „letzter Wert“ stehen bleiben.
    let now = 1_800_000_000.0;
    let data = json!({"limits": [{"kind": "session", "group": "session", "percent": 40, "severity": "normal",
                                  "resets_at": iso(now + 3600.0), "is_active": true}]});
    let mut snap = UsageSnapshot::default();
    let first = SharedView { data: Some(data), data_ts: Some(now), account: Some("a@x.de".into()), ..Default::default() };
    apply_view(&mut snap, &first, now, Lang::German);
    assert_eq!(snap.session_percent, Some(40));
    let switched = SharedView { error: Some(shared::SWITCH_ERROR.into()), account: Some("b@x.de".into()), ..Default::default() };
    apply_view(&mut snap, &switched, now, Lang::German);
    assert_eq!(snap.account.as_deref(), Some("b@x.de"));
    assert_eq!(snap.session_percent, None, "40 % gehören dem alten Konto");
    assert!(snap.limits.is_empty());
    assert_eq!(snap.error.as_deref(), Some("Kontowechsel, lade neu…"));
}

#[test]
fn the_rhythm_is_quick_at_start_patient_on_errors_and_calm_with_a_number() {
    let fresh = UsageSnapshot::default();
    assert_eq!(next_delay(&fresh, 120.0, 0.0), 5.0);
    let errored = UsageSnapshot { error: Some("x".into()), ..Default::default() };
    assert_eq!(next_delay(&errored, 120.0, 0.0), 30.0);
    let ok = UsageSnapshot { session_percent: Some(3), ..Default::default() };
    assert_eq!(next_delay(&ok, 120.0, 1.0), 38.0);
    assert_eq!(next_delay(&ok, 120.0, -1.0), 22.0);
}

fn service(dir: &std::path::Path, http: Arc<ScriptHttp>) -> UsageService {
    let poller = SharedPoller::new(
        dir.to_path_buf(),
        Arc::new(FakeLock { busy: AtomicBool::new(false) }),
        Arc::new(FixedTokens::new(vec!["t"])),
        http,
        Arc::new(now_unix),
        Arc::new(|| 0.0),
    );
    UsageService::new(poller, Lang::German, 120.0)
}

#[test]
fn the_service_delivers_updates_and_stops_without_calling_out_while_the_cache_lives() {
    let d = tempfile::tempdir().unwrap();
    let data = r#"{"five_hour": {"utilization": 12.0, "resets_at": "2099-01-01T00:00:00+00:00"}}"#;
    std::fs::write(
        d.path().join(shared::CACHE_FILE),
        format!(r#"{{"data": {data}, "data_ts": {}, "error": null, "next_ts": {}}}"#, now_unix(), now_unix() + 600.0),
    )
    .unwrap();
    let http = Arc::new(ScriptHttp::new(vec![reply(500, None, "")]));
    let svc = service(d.path(), http.clone());
    let calls = Arc::new(AtomicUsize::new(0));
    let c = calls.clone();
    svc.start(Box::new(move |s| {
        assert_eq!(s.session_percent, Some(12));
        c.fetch_add(1, Ordering::SeqCst);
    }));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while calls.load(Ordering::SeqCst) == 0 && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    svc.stop();
    assert!(calls.load(Ordering::SeqCst) >= 1);
    assert_eq!(http.calls.load(Ordering::SeqCst), 0, "ein lebender Cache wird nur gelesen");
    assert_eq!(svc.snapshot().session_percent, Some(12));
}

#[test]
fn a_manual_refresh_polls_even_a_living_cache() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join(shared::CACHE_FILE), format!(r#"{{"data": null, "next_ts": {}}}"#, now_unix() + 600.0)).unwrap();
    let http = Arc::new(ScriptHttp::new(vec![reply(200, None, r#"{"five_hour": {"utilization": 7.0}}"#)]));
    let svc = service(d.path(), http.clone());
    assert_eq!(svc.poll_once(false).session_percent, None);
    assert_eq!(svc.poll_once(true).session_percent, Some(7));
    assert_eq!(http.calls.load(Ordering::SeqCst), 1);
}
