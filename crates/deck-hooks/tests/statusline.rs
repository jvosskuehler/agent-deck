//! statusline (Python `test_hooks_report.py`, Teil Statuszeile): Kopfzeile mit Slot,
//! Ticket und Überschrift, Zahlenzeile wie Python, Live-Datei mit Python-Typen.

use std::path::Path;

use deck_core::paths::{live_path, load_json, save_json};
use deck_hooks::env::HookEnv;
use deck_hooks::statusline::*;
use serde_json::{Map, Value, json};

fn env_in(dir: &Path, slot: Option<&str>) -> HookEnv {
    HookEnv {
        state_dir: dir.join("state"),
        app_dir: dir.to_path_buf(),
        agent_slot: slot.map(String::from),
        cwd: None,
        now: 1000.5,
        ancestors: Box::new(Vec::new),
    }
}

fn obj(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

#[test]
fn the_status_line_starts_with_slot_ticket_and_title_in_bold() {
    let rec = obj(json!({"model": "opus", "effort": "xhigh", "ctx_pct": 41.2, "cost_usd": 1.5, "session_name": "Schrift-Einstellung im Client"}));
    let out = line(&rec, "B2 · ORPH-4116");
    let (head, body) = out.split_once('\n').unwrap();
    assert_eq!(head, "\x1b[1mB2 · ORPH-4116 · Schrift-Einstellung im Client\x1b[0m");
    assert_eq!(body, "opus  ·  effort xhigh  ·  ctx 41%  ·  $1.50");
    assert_eq!(line(&obj(json!({"model": "opus"})), ""), "opus");
    let long = line(&obj(json!({"session_name": "x".repeat(200)})), "B1");
    assert!(long.chars().count() < 100 && long.contains('…'));
}

#[test]
fn numbers_round_like_python() {
    assert_eq!(line(&obj(json!({"ctx_pct": 2.5, "cost_usd": 0.125})), ""), "ctx 2%  ·  $0.12");
    assert_eq!(line(&obj(json!({"ctx_pct": 41, "cost_usd": 0})), ""), "ctx 41%  ·  $0.00");
    assert_eq!(line(&obj(json!({"ctx_pct": null, "model": "", "effort": null})), ""), "");
}

#[test]
fn the_label_takes_the_assigned_ticket_first_then_the_reported_one() {
    let dir = tempfile::tempdir().unwrap();
    let env = env_in(dir.path(), None);
    std::fs::create_dir_all(&env.state_dir).unwrap();
    std::fs::write(env.state_dir.join("Z9.ticket"), "ORPH-1\n").unwrap();
    assert_eq!(label("Z9", &env).as_deref(), Some("Z9 · ORPH-1"));
    assert_eq!(label("Z8", &env).as_deref(), Some("Z8"));
    save_json(&dir.path().join("tickets.json"), &json!({"Z9": "ORPH-2"})).unwrap();
    assert_eq!(label("Z9", &env).as_deref(), Some("Z9 · ORPH-2"));
}

#[test]
fn live_values_keep_their_json_types_and_python_field_order() {
    let data = obj(json!({"model": {"display_name": "Opus 5", "id": "claude-opus-5"}, "effort": {"level": "xhigh"},
        "context_window": {"used_percentage": 41, "current_usage": {"input_tokens": 10, "output_tokens": 2.5}},
        "cost": {"total_cost_usd": 1.5}, "session_name": "Titel"}));
    let rec = extract(&data, 7.0).unwrap();
    assert_eq!(Value::Object(rec.clone()), json!({"model": "Opus 5", "effort": "xhigh", "ctx_pct": 41, "msg_tokens": 12.5,
        "cost_usd": 1.5, "session_name": "Titel", "ts": 7.0}));
    assert_eq!(rec.keys().collect::<Vec<_>>(), ["model", "effort", "ctx_pct", "msg_tokens", "cost_usd", "session_name", "ts"]);
}

#[test]
fn message_tokens_fall_back_to_total_output_and_falsy_parts_count_as_zero() {
    let cw = |v: Value| extract(&obj(json!({"context_window": v})), 0.0).unwrap()["msg_tokens"].clone();
    assert_eq!(cw(json!({"current_usage": {"input": 3, "output": 4}})), json!(7));
    assert_eq!(cw(json!({"current_usage": {"input_tokens": 0.0, "output_tokens": 5}})), json!(5));
    assert_eq!(cw(json!({"total_output_tokens": 9})), json!(9));
    assert_eq!(cw(json!({})), json!(null));
    assert!(extract(&obj(json!({"context_window": {"current_usage": [1]}})), 0.0).is_none(), "Python scheitert hier");
}

#[test]
fn a_run_writes_the_live_file_and_always_returns_a_line() {
    let dir = tempfile::tempdir().unwrap();
    let env = env_in(dir.path(), Some("A1"));
    let stdin = json!({"model": {"display_name": "Opus"}, "session_name": "Eigenes Terminal für Claude Code"}).to_string();
    let out = run(&env, Some(stdin.as_bytes())).unwrap();
    assert_eq!(out, "\x1b[1mA1 · Eigenes Terminal für Claude Code\x1b[0m\nOpus");
    assert_eq!(load_json(&live_path(&env.state_dir, "A1")).unwrap()["model"], "Opus");
    let no_slot = env_in(dir.path(), None);
    assert_eq!(run(&no_slot, Some(b"{}")).as_deref(), Some(""));
    assert_eq!(run(&no_slot, None).as_deref(), Some(""));
    assert_eq!(run(&no_slot, Some(b"[1]")), None, "kein Objekt: Python gibt nichts aus");
}
