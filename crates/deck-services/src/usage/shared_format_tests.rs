//! Das Dateiformat von `%LOCALAPPDATA%\claude-usage\usage.json`.
//!
//! Die Datei teilt sich das Deck mit dem Tray-Poller in Python
//! (`claude-usage-shared/usage_poller.py`), der weiter läuft. Er liest mit `json.load`
//! (UTF-8), nimmt also jedes gültige JSON; Trenner und Maskierung sind egal. Was zählt:
//! dieselben Felder, `account` in jedem Schreibweg, `null` statt fehlender Werte und die
//! Zahlentypen (`n429` ganz, Zeiten als Float, die API-Antwort in `data` unverändert).

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use serde_json::{Value, json};

use super::super::fetch::{HttpReply, Transport};
use super::tests::{FakeLock, FixedTokens, ScriptHttp, reply};
use super::*;

const NOW: f64 = 1_800_000_000.5;
const KEYS: [&str; 7] = ["data", "data_ts", "error", "next_ts", "n429", "interval", "account"];

/// Ein Abruf mit dieser Antwort; zurück kommt die geschriebene Datei, roh und gelesen.
fn written(answer: Result<HttpReply, Transport>, before: Option<&str>) -> (String, Value, SharedView) {
    let dir = tempfile::tempdir().unwrap();
    let lock = Arc::new(FakeLock { busy: AtomicBool::new(false) });
    let tokens = Arc::new(FixedTokens::of("a@x.de", vec!["t"]));
    let http = Arc::new(ScriptHttp::new(vec![answer]));
    let poller = SharedPoller::new(dir.path().to_path_buf(), lock, tokens, http, Arc::new(|| NOW), Arc::new(|| 0.0));
    if let Some(b) = before {
        std::fs::write(poller.cache_path(), b).unwrap();
    }
    let view = poller.get_usage(false);
    let raw = String::from_utf8(std::fs::read(poller.cache_path()).unwrap()).expect("UTF-8, wie json.load es liest");
    let c: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(c.as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>(), KEYS, "Felder wie im Python-Poller");
    assert!(c["next_ts"].is_f64() && c["interval"].is_number() && c["n429"].is_u64(), "{raw}");
    assert_eq!(c["account"], "a@x.de");
    (raw, c, view)
}

#[test]
fn a_success_writes_the_api_answer_unchanged_with_float_times_and_an_integer_counter() {
    let body = r#"{"five_hour": {"utilization": 42, "resets_at": "2026-11-09T12:00:00.123+00:00"}, "seven_day": {"utilization": 3.0}, "extra_usage": null}"#;
    let (_, c, _) = written(reply(200, None, body), None);
    assert_eq!(c["data"], serde_json::from_str::<Value>(body).unwrap());
    assert!(c["data"]["five_hour"]["utilization"].is_u64(), "eine ganze Zahl bleibt ganz");
    assert!(c["data"]["seven_day"]["utilization"].is_f64(), "3.0 bleibt Float");
    assert!(c["data"]["extra_usage"].is_null());
    assert_eq!((c["data_ts"].as_f64(), c["error"].clone(), c["n429"].as_u64()), (Some(NOW), Value::Null, Some(0)));
}

#[test]
fn a_rate_limit_keeps_the_last_answer_and_counts_up() {
    let before = json!({"data": {"a": 1}, "data_ts": 5.0, "error": null, "next_ts": 0, "n429": 2, "interval": 300, "account": "a@x.de"});
    let (_, c, _) = written(reply(429, None, ""), Some(&before.to_string()));
    assert_eq!((c["data"].clone(), c["data_ts"].as_f64()), (json!({"a": 1}), Some(5.0)));
    assert!(c["data_ts"].is_f64());
    assert_eq!((c["error"].as_str(), c["n429"].as_u64()), (Some("Rate-Limit"), Some(3)));
}

#[test]
fn a_rejected_token_writes_nulls_and_the_error_text_as_utf8() {
    let (raw, c, _) = written(reply(401, None, ""), None);
    assert_eq!((c["data"].clone(), c["data_ts"].clone()), (Value::Null, Value::Null));
    assert_eq!(c["error"], "Token ungueltig – 'claude auth login'");
    assert!(raw.contains("\"data\":null") || raw.contains("\"data\": null"), "null steht drin, fehlt nicht: {raw}");
}

#[test]
fn a_cache_the_python_poller_wrote_is_read_with_the_same_meaning() {
    // Python schreibt mit `", "`, maskiert Nicht-ASCII als \uXXXX und `interval` als int.
    let py = format!(
        "{{\"data\": {{\"five_hour\": {{\"utilization\": 7}}}}, \"data_ts\": 5.0, \"error\": \"Kontowechsel \\u2013 lade neu\\u2026\", \
         \"next_ts\": {}, \"n429\": 0, \"interval\": 300, \"account\": \"a@x.de\"}}",
        NOW + 60.0
    );
    let (_, c, view) = written(reply(200, None, "{}"), Some(&py));
    assert_eq!(view.data, Some(json!({"five_hour": {"utilization": 7}})), "frisch: gelesen, nicht abgerufen");
    assert_eq!((view.error.as_deref(), view.data_ts, view.account.as_deref()), (Some("Kontowechsel – lade neu…"), Some(5.0), Some("a@x.de")));
    assert_eq!(c["interval"], 300, "die Datei bleibt unberührt");
}
