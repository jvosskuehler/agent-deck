//! Wann eine Rückfrage ohne Hook endet (`query_end.rs`). Die Einträge haben die Form, die
//! Claude Code 2.1.331 schreibt (aus einer Test-VM, gekürzt). Der wichtigere Teil sind die
//! Fälle, in denen eine offene Rückfrage stehen bleiben MUSS.

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use deck_core::isotime::parse_iso;
use deck_core::query_end::{QueryEnd, QueryWatch, SLACK_S, TAIL_BYTES, scan};
use deck_core::slot_state::SlotState;
use serde_json::{Value, json};

/// Die Rückfrage meldete der Hook um 12:50:54,96 (PermissionRequest).
fn since() -> f64 {
    parse_iso("2026-11-24T12:50:54.961Z").unwrap()
}

fn at(t: &str) -> String {
    format!("2026-11-24T{t}Z")
}

fn line(v: Value) -> String {
    v.to_string()
}

fn prompt(t: &str) -> String {
    line(json!({"type": "user", "isSidechain": false, "timestamp": at(t), "message": {"role": "user", "content": "Lege die Datei an."}}))
}

fn tool_use(t: &str) -> String {
    line(json!({"type": "assistant", "isSidechain": false, "timestamp": at(t),
        "message": {"role": "assistant", "content": [{"type": "tool_use", "id": "toolu_1", "name": "Write", "input": {"file_path": "/tmp/x"}}]}}))
}

fn rejected(t: &str, sidechain: bool) -> String {
    line(json!({"type": "user", "isSidechain": sidechain, "timestamp": at(t), "toolUseResult": "User rejected tool use",
        "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_1", "is_error": true,
            "content": "The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written to the file). STOP what you are doing and wait for the user to tell you how to proceed."}]}}))
}

fn interrupted(t: &str) -> String {
    line(json!({"type": "user", "isSidechain": false, "timestamp": at(t),
        "message": {"role": "user", "content": [{"type": "text", "text": "[Request interrupted by user for tool use]"}]}}))
}

/// Was Claude auch bei offenem Dialog oder kurz davor schreibt.
fn noise(t: &str) -> Vec<String> {
    vec![
        line(json!({"type": "file-history-snapshot", "messageId": "m1", "snapshot": {}})),
        line(json!({"type": "attachment", "isSidechain": false, "timestamp": at(t), "attachment": {"type": "hook_success"}})),
        line(json!({"type": "api-request", "timestamp": at(t)})),
        line(json!({"type": "mode", "mode": "default"})),
        line(json!({"type": "permission-mode", "permissionMode": "default"})),
        line(json!({"type": "system", "subtype": "informational", "isSidechain": false, "timestamp": at(t), "content": "Mode"})),
        line(json!({"type": "file-history-delta", "timestamp": at(t)})),
        // Ergebnis eines parallelen Werkzeugs, auch als Fehler: keine Antwort auf den Dialog.
        line(json!({"type": "user", "isSidechain": false, "timestamp": at(t),
            "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_0", "content": "3"}]}})),
        line(json!({"type": "user", "isSidechain": false, "timestamp": at(t),
            "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_2", "is_error": true, "content": "Exit code 1"}]}})),
        "{kaputt".to_string(),
    ]
}

fn text(lines: &[String]) -> String {
    lines.iter().map(|l| format!("{l}\n")).collect()
}

#[test]
fn a_rejection_after_the_question_ends_it() {
    let t = text(&[prompt("12:50:52.999"), tool_use("12:50:54.697"), rejected("12:52:27.217", false), interrupted("12:52:27.227")]);
    assert_eq!(scan(&t, since()), Some(QueryEnd::Rejected));
}

#[test]
fn an_interrupt_alone_and_a_compact_also_end_it() {
    assert_eq!(scan(&text(&[interrupted("12:52:27.227")]), since()), Some(QueryEnd::Interrupted));
    let compact = line(json!({"type": "system", "subtype": "compact_boundary", "isSidechain": false, "timestamp": at("12:59:48.927")}));
    assert_eq!(scan(&text(&[compact]), since()), Some(QueryEnd::Compacted));
}

#[test]
fn an_open_dialog_stays_open_whatever_else_lands_in_the_transcript() {
    let mut lines = vec![prompt("12:50:52.999"), tool_use("12:50:54.697")];
    lines.extend(noise("12:51:30.000"));
    assert_eq!(scan(&text(&lines), since()), None);
}

#[test]
fn a_subagents_rejection_does_not_end_the_main_question() {
    assert_eq!(scan(&text(&[rejected("12:52:27.217", true)]), since()), None);
}

#[test]
fn the_rejection_of_an_earlier_question_does_not_count() {
    // Abgelehnt, neuer Prompt, neue Rückfrage: die alte Ablehnung liegt davor.
    let t = text(&[rejected("12:40:00.000", false), interrupted("12:40:00.010"), prompt("12:50:52.999"), tool_use("12:50:54.697")]);
    assert_eq!(scan(&t, since()), None);
    // Ohne Zeitstempel weiß niemand, wann: zählt nicht.
    let undated = line(json!({"type": "user", "toolUseResult": "User rejected tool use"}));
    assert_eq!(scan(&text(&[undated]), since()), None);
}

#[test]
fn an_esc_while_the_hook_was_still_starting_belongs_to_this_question() {
    let just_before = since() - SLACK_S / 2.0;
    let rec = json!({"type": "user", "isSidechain": false, "timestamp": stamp(just_before), "toolUseResult": "User rejected tool use"});
    assert_eq!(scan(&text(&[line(rec)]), since()), Some(QueryEnd::Rejected));
}

fn stamp(unix: f64) -> String {
    let base = since();
    let ms = ((unix - base) * 1000.0).round() as i64 + 54_961;
    format!("2026-11-24T12:50:{:02}.{:03}Z", ms / 1000, ms % 1000)
}

// ── Der Beobachter über die Takte ─────────────────────────────────────────────

fn waiting(transcript: &Path, ts: f64) -> SlotState {
    json!({"slot": "A4", "status": "waiting", "ts": ts, "transcript": transcript.to_string_lossy()}).as_object().unwrap().clone()
}

fn states(st: SlotState) -> HashMap<String, SlotState> {
    HashMap::from([("A4".to_string(), st)])
}

fn status(m: &HashMap<String, SlotState>) -> &str {
    m["A4"]["status"].as_str().unwrap()
}

fn append(p: &Path, lines: &[String]) {
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(p).unwrap();
    f.write_all(text(lines).as_bytes()).unwrap();
}

#[test]
fn the_tile_falls_back_to_idle_once_the_rejection_is_written_and_is_reported_once() {
    let dir = tempfile::tempdir().unwrap();
    let tr = dir.path().join("s.jsonl");
    append(&tr, &[prompt("12:50:52.999"), tool_use("12:50:54.697")]);
    let mut w = QueryWatch::default();
    let mut m = states(waiting(&tr, since()));
    assert!(w.settle(&mut m).is_empty());
    assert_eq!(status(&m), "waiting");

    append(&tr, &noise("12:51:30.000"));
    let mut m = states(waiting(&tr, since()));
    assert!(w.settle(&mut m).is_empty());
    assert_eq!(status(&m), "waiting", "nur Rauschen: die Rückfrage bleibt");

    append(&tr, &[rejected("12:52:27.217", false), interrupted("12:52:27.227")]);
    let mut m = states(waiting(&tr, since()));
    assert_eq!(w.settle(&mut m), [("A4".to_string(), QueryEnd::Rejected)]);
    assert_eq!(status(&m), "idle");

    // Die Datei sagt weiter `waiting`: jeder Takt bleibt idle, gemeldet wird nur einmal.
    let mut m = states(waiting(&tr, since()));
    assert!(w.settle(&mut m).is_empty());
    assert_eq!(status(&m), "idle");
}

#[test]
fn a_new_question_after_a_rejection_is_shown_again() {
    let dir = tempfile::tempdir().unwrap();
    let tr = dir.path().join("s.jsonl");
    append(&tr, &[tool_use("12:50:54.697"), rejected("12:52:27.217", false)]);
    let mut w = QueryWatch::default();
    let mut m = states(waiting(&tr, since()));
    w.settle(&mut m);
    assert_eq!(status(&m), "idle");

    append(&tr, &[prompt("12:55:00.000"), tool_use("12:55:02.000")]);
    let mut m = states(waiting(&tr, parse_iso(&at("12:55:02.200")).unwrap()));
    assert!(w.settle(&mut m).is_empty());
    assert_eq!(status(&m), "waiting");
}

#[test]
fn without_a_transcript_the_question_stays() {
    let dir = tempfile::tempdir().unwrap();
    let mut w = QueryWatch::default();
    let mut st = waiting(&dir.path().join("fehlt.jsonl"), since());
    let mut m = states(st.clone());
    w.settle(&mut m);
    assert_eq!(status(&m), "waiting", "Datei fehlt");
    st.remove("transcript");
    let mut m = states(st);
    w.settle(&mut m);
    assert_eq!(status(&m), "waiting", "Meldung eines alten Hooks ohne Feld");
}

#[test]
fn other_states_are_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let tr = dir.path().join("s.jsonl");
    append(&tr, &[rejected("12:52:27.217", false)]);
    let mut w = QueryWatch::default();
    for s in ["thinking", "running", "done"] {
        let mut st = waiting(&tr, since());
        st.insert("status".into(), s.into());
        let mut m = states(st);
        assert!(w.settle(&mut m).is_empty());
        assert_eq!(status(&m), s);
    }
}

#[test]
fn a_rejection_behind_a_huge_entry_is_found_and_a_cut_first_line_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let tr = dir.path().join("s.jsonl");
    // Ein Eintrag größer als das gelesene Ende, darin eine Ablehnung als Text: angeschnitten
    // darf er nicht als eigene Zeile zählen.
    let blob = format!("{{\"type\":\"api-request-blob\",\"x\":\"{}\",\"y\":{}}}", "a".repeat(TAIL_BYTES as usize), rejected("12:52:00.000", false));
    append(&tr, &[tool_use("12:50:54.697"), blob]);
    let mut w = QueryWatch::default();
    let mut m = states(waiting(&tr, since()));
    w.settle(&mut m);
    assert_eq!(status(&m), "waiting");
    append(&tr, &[rejected("12:52:27.217", false)]);
    let mut m = states(waiting(&tr, since()));
    w.settle(&mut m);
    assert_eq!(status(&m), "idle");
}

#[test]
fn a_half_written_last_line_is_read_once_it_is_complete() {
    let dir = tempfile::tempdir().unwrap();
    let tr = dir.path().join("s.jsonl");
    let full = rejected("12:52:27.217", false);
    let (a, b) = full.split_at(40);
    std::fs::write(&tr, format!("{}\n{a}", tool_use("12:50:54.697"))).unwrap();
    let mut w = QueryWatch::default();
    let mut m = states(waiting(&tr, since()));
    w.settle(&mut m);
    assert_eq!(status(&m), "waiting");
    let mut f = std::fs::OpenOptions::new().append(true).open(&tr).unwrap();
    f.write_all(format!("{b}\n").as_bytes()).unwrap();
    let mut m = states(waiting(&tr, since()));
    w.settle(&mut m);
    assert_eq!(status(&m), "idle");
}
