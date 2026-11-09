//! report-Hook (aus Python `test_hooks_report.py`):
//! welche Notification eine echte Rückfrage ist, welche Felder fortgetragen werden und
//! wie der Slot gefunden wird. Alles gegen Wegwerf-Ordner.

use std::path::Path;

use deck_core::paths::{load_json, save_json, state_path};
use deck_hooks::env::{HookEnv, load_pidmap, slot_from_procs};
use deck_hooks::report::*;
use serde_json::{Map, Value, json};

fn env_in(dir: &Path, slot: Option<&str>) -> HookEnv {
    HookEnv {
        state_dir: dir.join("state"),
        app_dir: dir.to_path_buf(),
        claude_settings: dir.join("settings.json"),
        agent_slot: slot.map(String::from),
        cwd: Some(r"C:\Users\x\Repos\deck".into()),
        now: 1000.5,
        ancestors: Box::new(Vec::new),
    }
}

fn obj(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

fn run_with(env: &HookEnv, status: &str, stdin: Value) -> Option<Value> {
    let bytes = stdin.to_string().into_bytes();
    run(status, env, || Some(bytes)).map(|p| load_json(&p).unwrap())
}

#[test]
fn the_prompt_is_trimmed_and_capped_at_500_characters() {
    assert_eq!(prompt_of(&obj(json!({"prompt": "  Hallo Welt  "}))).as_deref(), Some("Hallo Welt"));
    assert_eq!(prompt_of(&obj(json!({"prompt": "   "}))), None);
    assert_eq!(prompt_of(&obj(json!({}))), None);
    assert_eq!(prompt_of(&obj(json!({"prompt": 123}))), None);
    let got = prompt_of(&obj(json!({"prompt": "x".repeat(600)}))).unwrap();
    assert!(got.ends_with('…') && got.chars().count() == 501);
}

#[test]
fn only_three_notification_types_are_a_real_question() {
    for nt in ["permission_prompt", "elicitation_dialog", "agent_needs_input"] {
        assert!(is_real_query(&obj(json!({"notification_type": nt}))), "{nt}");
    }
    for nt in ["idle_prompt", "agent_completed", "auth_success"] {
        assert!(!is_real_query(&obj(json!({"notification_type": nt}))), "{nt}");
    }
    assert!(is_real_query(&obj(json!({"message": "Bash: Allow this command?"}))));
    assert!(is_real_query(&obj(json!({"message": "Claude needs your permission to use Bash"}))));
    assert!(!is_real_query(&obj(json!({"message": "Claude is waiting for your input"}))));
    assert!(!is_real_query(&obj(json!({}))));
}

#[test]
fn activity_names_the_tool_and_its_first_line_capped_at_42() {
    assert_eq!(activity_of(&obj(json!({"tool_name": "Bash", "tool_input": {"command": "ls\nrm"}}))).as_deref(), Some("Bash: ls"));
    let long = activity_of(&obj(json!({"tool_name": "Read", "tool_input": {"file_path": "x".repeat(50)}}))).unwrap();
    assert_eq!(long, format!("Read: {}…", "x".repeat(42)));
    assert_eq!(activity_of(&obj(json!({"tool_name": "Task"}))).as_deref(), Some("Task"));
    assert_eq!(activity_of(&obj(json!({}))), None);
}

#[test]
fn permission_modes_get_deck_names() {
    assert_eq!(mode_of(&obj(json!({"permission_mode": "acceptEdits"}))).as_deref(), Some("accept"));
    assert_eq!(mode_of(&obj(json!({"permission_mode": "default"}))).as_deref(), Some("manual"));
    assert_eq!(mode_of(&obj(json!({"permission_mode": "bypassPermissions"}))).as_deref(), Some("bypass"));
    assert_eq!(mode_of(&obj(json!({"permission_mode": "neu"}))).as_deref(), Some("neu"));
    assert_eq!(mode_of(&obj(json!({}))), None);
}

#[test]
fn a_report_writes_its_fields_in_a_fixed_order_as_utf8() {
    let dir = tempfile::tempdir().unwrap();
    let env = env_in(dir.path(), Some("A1"));
    let rec = run_with(&env, "thinking", json!({"hook_event_name": "UserPromptSubmit", "session_id": "s1",
        "permission_mode": "plan", "effort": {"level": "xhigh"}, "prompt": "Mach mal für mich"})).unwrap();
    assert_eq!(rec, json!({"slot": "A1", "status": "thinking", "ts": 1000.5, "mode": "plan", "effort": "xhigh",
        "activity": "", "session_id": "s1", "cwd": r"C:\Users\x\Repos\deck", "prompt": "Mach mal für mich"}));
    let raw = std::fs::read_to_string(state_path(&env.state_dir, "A1")).unwrap();
    assert!(raw.starts_with(r#"{"slot":"A1","status":"thinking","ts":1000.5,"mode":"plan""#), "{raw}");
    assert!(raw.contains("Mach mal für mich"), "Umlaute als UTF-8: {raw}");
}

#[test]
fn fields_only_some_events_deliver_are_carried_over_and_stop_clears_the_activity() {
    let dir = tempfile::tempdir().unwrap();
    let env = env_in(dir.path(), Some("A1"));
    run_with(&env, "thinking", json!({"session_id": "s1", "permission_mode": "auto", "prompt": "Frage"}));
    let rec = run_with(&env, "running", json!({"tool_name": "Bash", "tool_input": {"command": "ls"}})).unwrap();
    assert_eq!((rec["mode"].clone(), rec["session_id"].clone(), rec["prompt"].clone()), (json!("auto"), json!("s1"), json!("Frage")));
    assert_eq!(rec["activity"], "Bash: ls");
    let rec = run_with(&env, "thinking", json!({})).unwrap();
    assert_eq!(rec["activity"], "Bash: ls", "ohne Tool bleibt die Aktivität");
    let rec = run_with(&env, "done", json!({"hook_event_name": "Stop"})).unwrap();
    assert_eq!(rec["activity"], "");
}

#[test]
fn an_idle_notification_and_a_non_startup_session_start_leave_the_file_alone() {
    let dir = tempfile::tempdir().unwrap();
    let env = env_in(dir.path(), Some("A1"));
    assert!(run_with(&env, "waiting", json!({"notification_type": "idle_prompt"})).is_none());
    assert!(run_with(&env, "idle", json!({"hook_event_name": "SessionStart", "source": "clear"})).is_none());
    assert!(load_json(&state_path(&env.state_dir, "A1")).is_none());
    assert!(run_with(&env, "idle", json!({"hook_event_name": "SessionStart", "source": "startup"})).is_some());
    assert!(run_with(&env, "idle", json!({"hook_event_name": "SessionStart"})).is_some());
    assert_eq!(run_with(&env, "waiting", json!({"notification_type": "permission_prompt"})).unwrap()["status"], "waiting");
}

#[test]
fn broken_missing_or_non_object_stdin_still_reports() {
    let dir = tempfile::tempdir().unwrap();
    let env = env_in(dir.path(), Some("A1"));
    assert!(run("done", &env, || Some(b"{kaputt".to_vec())).is_some());
    assert!(run("done", &env, || None).is_some());
    assert!(run("done", &env, || Some(b"[1, 2]".to_vec())).is_some());
    let rec = run_with(&env, "running", json!({"tool_name": 5, "permission_mode": ["x"], "session_id": 7})).unwrap();
    assert_eq!((rec["activity"].clone(), rec.get("mode"), rec.get("session_id")), (json!(""), None, None), "falsche Typen fallen weg");
}

#[test]
fn stdin_is_decoded_as_utf8_whatever_the_console_codepage() {
    let dir = tempfile::tempdir().unwrap();
    let env = env_in(dir.path(), Some("A1"));
    let p = run("thinking", &env, || Some(r#"{"prompt": "Größe ändern"}"#.as_bytes().to_vec())).unwrap();
    assert_eq!(load_json(&p).unwrap()["prompt"], "Größe ändern");
}

#[test]
fn without_a_slot_nothing_is_written_and_stdin_is_not_read() {
    let dir = tempfile::tempdir().unwrap();
    let env = env_in(dir.path(), None);
    assert!(run("done", &env, || panic!("stdin darf ohne Slot nicht gelesen werden")).is_none());
}

#[test]
fn the_slot_comes_from_the_first_ancestor_in_a_pidmap() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    save_json(&state.join("pidmap-A.json"), &json!({"100": "A1", "x": "A9", "200": "A2"})).unwrap();
    save_json(&state.join("pidmap-B.json"), &json!({"300": "B1", "400": 7})).unwrap();
    std::fs::write(state.join("pidmap-C.json"), "{halb").unwrap();
    let pm = load_pidmap(&state);
    assert_eq!(pm.len(), 3, "Schlüssel ohne PID und Werte ohne Slot zählen nicht: {pm:?}");
    assert_eq!(slot_from_procs(&state, &|| vec![5, 300, 100]).as_deref(), Some("B1"));
    assert_eq!(slot_from_procs(&state, &|| vec![400, 200]).as_deref(), Some("A2"), "ein Nicht-Text ist kein Slot");
    assert_eq!(slot_from_procs(&state, &|| vec![1, 2]), None);
    assert_eq!(slot_from_procs(&dir.path().join("fehlt"), &|| panic!("ohne pidmap keine Prozessliste")), None);
    let mut env = env_in(dir.path(), None);
    env.ancestors = Box::new(|| vec![100]);
    assert_eq!(env.resolve_slot().as_deref(), Some("A1"));
}

#[test]
fn the_real_process_chain_starts_with_this_process() {
    let chain = deck_hooks::procs::ancestor_pids();
    if cfg!(windows) {
        assert_eq!(chain.first().copied(), Some(std::process::id()));
        assert!(chain.len() >= 2, "ein Testprozess hat einen Elternprozess");
    }
}
