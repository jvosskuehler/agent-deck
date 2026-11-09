//! Statuszeile: wie lange der Prompt-Cache noch warm ist (Python `test_hooks_report.py`,
//! Teil Cache). Transcripts als Wegwerf-Dateien, keine echte Session.

use std::path::Path;

use deck_core::claude::hook_setup::STATUSLINE_REFRESH_S;
use deck_core::i18n::Lang;
use deck_core::isotime::parse_iso;
use deck_core::paths::{live_path, load_json, save_json, state_path};
use deck_hooks::cache_scan::*;
use deck_hooks::env::HookEnv;
use deck_hooks::statusline::{cache_part, extract, line, run};
use serde_json::{Map, Value, json};

fn obj(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

fn epoch(s: &str) -> f64 {
    parse_iso(s).unwrap()
}

fn request(ts: &str, ttl: &str, source: &str) -> String {
    let marks = json!([{"path": ["system", 1], "value": {"type": "ephemeral", "ttl": "1h"}},
                       {"path": ["messages", 9], "value": {"type": "ephemeral", "ttl": ttl}}]);
    json!({"type": "api-request", "timestamp": ts, "querySource": source, "cacheControl": marks}).to_string()
}

fn req(ts: &str) -> String {
    request(ts, "1h", "repl_main_thread")
}

fn answer(ts: &str, h1: u32, m5: u32, sidechain: bool) -> String {
    let usage = json!({"cache_creation": {"ephemeral_1h_input_tokens": h1, "ephemeral_5m_input_tokens": m5}});
    json!({"type": "assistant", "timestamp": ts, "isSidechain": sidechain, "message": {"usage": usage}}).to_string()
}

fn compact(ts: &str) -> String {
    json!({"type": "system", "subtype": "compact_boundary", "timestamp": ts, "isSidechain": false}).to_string()
}

fn scan_of(lines: &[String]) -> Option<Scan> {
    scan(lines.iter().map(String::as_str))
}

fn write(dir: &Path, lines: &[String]) -> String {
    let path = dir.join("t.jsonl");
    std::fs::write(&path, lines.join("\n") + "\n").unwrap();
    path.to_string_lossy().into_owned()
}

#[test]
fn the_cache_counts_from_when_the_last_request_went_out() {
    // Nicht ab der Antwort. Die kürzeste cache_control-Marke zählt: mit ihr läuft das Gespräch ab.
    let t0 = epoch("2026-11-08T22:00:00Z");
    let lines = [
        req("2026-11-08T21:00:00Z"),
        answer("2026-11-08T21:00:30Z", 5, 0, false),
        req("2026-11-08T22:00:00Z"),
        answer("2026-11-08T22:01:00Z", 5, 0, false),
        request("2026-11-08T22:05:00Z", "1h", "sdk"), // fremde Anfrage
        answer("2026-11-08T22:06:00Z", 5, 0, true),   // Subagent
    ];
    assert_eq!(scan_of(&lines), Some(Scan::Sent(t0, 3600)));
    assert_eq!(scan_of(&[request("2026-11-08T22:00:00Z", "5m", "repl_main_thread")]), Some(Scan::Sent(t0, 300)));
}

#[test]
fn without_a_logged_request_the_last_answer_counts() {
    let t = epoch("2026-11-08T22:01:00Z");
    assert_eq!(scan_of(&[answer("2026-11-08T22:01:00Z", 0, 7, false)]), Some(Scan::Sent(t, 300)));
    assert_eq!(scan_of(&[answer("2026-11-08T22:01:00Z", 7, 0, false)]), Some(Scan::Sent(t, 3600)));
    assert_eq!(scan_of(&[answer("2026-11-08T22:01:00Z", 0, 0, false)]), None, "nichts geschrieben");
    assert_eq!(scan_of(&["{kaputt".into(), String::new(), "null".into()]), None);
}

#[test]
fn the_cache_is_found_even_behind_a_huge_tool_result() {
    // Gelesen wird nur das Ende; ein Tool-Ergebnis größer als der erste Happen darf die
    // letzte Anfrage nicht verstecken.
    let dir = tempfile::tempdir().unwrap();
    let huge = json!({"type": "user", "toolUseResult": "x".repeat(TAIL_STEPS[0] as usize * 2)}).to_string();
    let path = write(dir.path(), &[req("2026-11-08T22:00:00Z"), huge]);
    let until = epoch("2026-11-08T22:00:00Z") + 3600.0;
    assert_eq!(from_transcript(Some(&json!(path))), Some(Scan::Sent(until, 3600)));
    assert_eq!(from_transcript(Some(&json!(format!("{path}.fehlt")))), None);
    assert_eq!(from_transcript(None), None);
    // Ohne prompt_cache im stdin (frisch fortgesetzte Session) gilt das Transcript.
    assert_eq!(cache_until(&obj(json!({"transcript_path": path}))), Some((until, 3600)));
}

#[test]
fn prompt_cache_from_stdin_beats_the_transcript() {
    let data = obj(json!({"prompt_cache": {"warm": true, "ttl": "1h", "expires_at": 1_794_182_611},
                          "transcript_path": "gibt-es-nicht.jsonl"}));
    assert_eq!(cache_until(&data), Some((1_794_182_611.0, 3600)));
    let cold = obj(json!({"prompt_cache": {"warm": false, "ttl": "5m", "expires_at": null}}));
    assert_eq!(cache_until(&cold), Some((0.0, 300)));
    assert_eq!(cache_until(&obj(json!({"prompt_cache": {"ttl": "1h"}}))), None, "ohne Zeit, ohne Transcript");
    assert_eq!(cache_until(&obj(json!({}))), None);
}

#[test]
fn the_display_counts_to_the_next_beat_and_warns_when_it_gets_short() {
    let now = 1_000_000.0;
    let beat = STATUSLINE_REFRESH_S as f64;
    let de = Lang::German;
    assert_eq!(cache_part(now + 42.0 * 60.0 + beat + 5.0, 3600.0, now, de), "cache 42 min");
    let low = cache_part(now + 4.0 * 60.0 + beat, 3600.0, now, de);
    assert!(low.contains("cache 4 min") && low.starts_with("\x1b[33m"), "gelb");
    assert!(cache_part(now + 30.0 + beat, 3600.0, now, de).contains("cache <1 min"));
    // Bis zum nächsten Takt läuft er ab: lieber jetzt schon „kalt“ als zu viel versprochen.
    let cold = cache_part(now + beat - 1.0, 3600.0, now, de);
    assert!(cold.contains("cache kalt") && cold.starts_with("\x1b[2m"), "gedimmt");
    assert!(cache_part(now, 3600.0, now, Lang::English).contains("cache cold"));
    // Beim 5-Minuten-Cache wird erst in der letzten Minute gewarnt.
    assert_eq!(cache_part(now + 3.0 * 60.0 + beat, 300.0, now, de), "cache 3 min");
    let rec = obj(json!({"model": "opus", "cache_until": now + 600.0 + beat, "cache_ttl": 3600, "ts": now}));
    assert_eq!(line(&rec, "", &Default::default(), de), "opus  ·  cache 10 min");
}

#[test]
fn after_a_compact_there_is_no_timer_until_the_next_request() {
    // Nach /compact ist der alte Cache nichts mehr wert. Gesehen 2026-11-09: direkt nach
    // /compact stand „1 min“ da, weil die Compact-Anfrage nicht als api-request im Transcript steht.
    let before = vec![req("2026-11-08T22:06:00Z"), answer("2026-11-08T22:06:10Z", 5, 0, false)];
    let mut compacted = before.clone();
    compacted.push(compact("2026-11-08T23:03:47Z"));
    assert_eq!(scan_of(&compacted), Some(Scan::Compacted));
    // Mit deiner nächsten Nachricht geht eine Anfrage raus: der Timer ist wieder da.
    let mut after = compacted.clone();
    after.push(req("2026-11-08T23:05:50Z"));
    assert_eq!(scan_of(&after), Some(Scan::Sent(epoch("2026-11-08T23:05:50Z"), 3600)));
    // Der Compact schlägt auch Claude Codes eigene Buchführung im stdin.
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), &compacted);
    let data = obj(json!({"prompt_cache": {"warm": true, "ttl": "1h", "expires_at": 1_794_182_611}, "transcript_path": path}));
    assert_eq!(cache_until(&data), None);
    let rec = extract(&data, 7.0);
    assert!(rec["cache_until"].is_null() && !line(&rec, "", &Default::default(), Lang::German).contains("cache"));
}

#[test]
fn the_context_size_in_tokens_for_auto_compact() {
    // Prozent taugen nicht als Schwelle: das Fenster ist 200k oder 1M groß.
    let tokens = |cw: Value| extract(&obj(json!({"context_window": cw})), 0.0)["ctx_tokens"].clone();
    let cw = json!({"current_usage": {"input_tokens": 3, "cache_creation_input_tokens": 1000,
                                      "cache_read_input_tokens": 199_000, "output_tokens": 500},
                    "used_percentage": 20, "context_window_size": 1_000_000});
    assert_eq!(tokens(cw), json!(200_003), "Ausgabe zählt nicht mit");
    assert_eq!(tokens(json!({"used_percentage": 20, "context_window_size": 1_000_000})), json!(200_000), "ganze Tokens");
    assert_eq!(tokens(json!({})), Value::Null);
}

#[test]
fn while_the_agent_works_the_line_shows_no_timer_but_the_live_file_keeps_it() {
    let dir = tempfile::tempdir().unwrap();
    let env = HookEnv {
        state_dir: dir.path().join("state"),
        app_dir: dir.path().to_path_buf(),
        claude_settings: dir.path().join("settings.json"),
        agent_slot: Some("A1".into()),
        cwd: None,
        now: 1_794_180_000.0,
    };
    std::fs::create_dir_all(&env.state_dir).unwrap();
    let stdin = json!({"model": "opus", "prompt_cache": {"warm": true, "ttl": "1h", "expires_at": 1_794_181_000}}).to_string();
    assert_eq!(run(&env, Some(stdin.as_bytes())), "\x1b[1mA1\x1b[0m\nopus  ·  cache 16 min");
    save_json(&state_path(&env.state_dir, "A1"), &json!({"status": "running", "ts": env.now - 30.0})).unwrap();
    assert_eq!(run(&env, Some(stdin.as_bytes())), "\x1b[1mA1\x1b[0m\nopus");
    assert_eq!(load_json(&live_path(&env.state_dir, "A1")).unwrap()["cache_until"], json!(1_794_181_000.0));
}
