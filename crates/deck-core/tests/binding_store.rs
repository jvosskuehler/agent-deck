//! binding: Fenster ↔ Repo, Platzhalter, Ticket-Helfer (Python `test_domain_binding.py`),
//! dazu die selbstheilende Persistenz, die Pfade und die Zustandsdateien.

use deck_core::bind_store::BindStore;
use deck_core::binding::*;
use deck_core::paths::{self, load_json, save_json};
use deck_core::slot_state::*;
use serde_json::{Value, json};

#[test]
fn empty_and_unknown_workspaces_are_placeholders() {
    assert!(is_placeholder_ws(Some("")));
    assert!(is_placeholder_ws(None));
    assert!(is_placeholder_ws(Some("unknown")));
    assert!(is_placeholder_ws(Some("  Unknown  ")));
    assert!(!is_placeholder_ws(Some("myrepo")));
}

#[test]
fn the_repo_name_is_the_part_before_the_vscode_marker() {
    assert_eq!(repo_from_title("main.py - myrepo - Visual Studio Code"), "myrepo");
    assert_eq!(repo_from_title("● app.tsx - acme-client - Visual Studio Code"), "acme-client");
    assert_eq!(repo_from_title("Visual Studio Code"), "Visual Studio Code");
}

#[test]
fn only_known_remote_markers_are_stripped() {
    assert_eq!(strip_remote_tag("agent-deck [SSH: 10.8.0.10]"), "agent-deck");
    assert_eq!(strip_remote_tag("jorrit [wsl: Ubuntu]"), "jorrit");
    assert_eq!(strip_remote_tag("release [alt]"), "release [alt]");
    assert_eq!(window_from_title("x - jorrit [SSH: h] - Visual Studio Code"), "jorrit [SSH: h]");
    assert_eq!(repo_from_title("x - jorrit [SSH: h] - Visual Studio Code"), "jorrit");
}

#[test]
fn ticket_slug_is_lowercase_alnum_with_single_hyphens() {
    assert_eq!(ticket_slug("ABC-123"), "abc-123");
    assert_eq!(ticket_slug("ABC-123: Login fix"), "abc-123-login-fix");
    assert_eq!(ticket_slug("  #42  "), "42");
    assert_eq!(ticket_slug("a//b__c"), "a-b-c");
    assert_eq!(ticket_slug(""), "");
    assert_eq!(ticket_slug("---"), "");
}

#[test]
fn ticket_branch_is_prefix_plus_slug_or_nothing() {
    assert_eq!(ticket_branch("ABC-123", None), "ticket/abc-123");
    assert_eq!(ticket_branch("ABC-123", Some("feat/")), "feat/abc-123");
    assert_eq!(ticket_branch("", None), "");
    assert_eq!(ticket_branch("!!!", None), "");
}

#[test]
fn a_bare_number_gets_the_project_prefix_and_a_key_its_project_uppercased() {
    assert_eq!(jira_key("2701", Some("PROJ")), "PROJ-2701");
    assert_eq!(jira_key("  #42 ", Some("PROJ")), "PROJ-42");
    assert_eq!(jira_key("PROJ-2701", Some("PROJ")), "PROJ-2701");
    assert_eq!(jira_key("abc-123", Some("PROJ")), "ABC-123");
    assert_eq!(jira_key("2701", Some("")), "2701");
    assert_eq!(jira_key("", Some("PROJ")), "");
}

#[test]
fn placeholder_bindings_are_dropped_on_load_and_the_file_heals() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join(paths::BINDINGS_FILE);
    save_json(&p, &json!({"A": "repo-a", "B": "unknown", "C": ""})).unwrap();
    let store = BindStore::load(dir.path());
    assert_eq!(store.bindings.len(), 1);
    assert_eq!(store.bindings["A"], "repo-a");
    assert_eq!(load_json(&p), Some(json!({"A": "repo-a"})));
}

#[test]
fn broken_runtime_files_load_as_empty_and_junk_entries_drop_out() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    std::fs::write(d.join(paths::EFFORT_FILE), "{kaputt").unwrap();
    save_json(&d.join(paths::TICKETS_FILE), &json!({"A1": "ABC-1", "A2": "  ", "A3": 5})).unwrap();
    save_json(&d.join(paths::ORDER_FILE), &json!({"A": ["A2", "", 7, "A1"], "B": "x"})).unwrap();
    save_json(&d.join(paths::WIN_ORDER_FILE), &json!(["deck", "deck", "", 3, "api"])).unwrap();
    let s = BindStore::load(d);
    assert!(s.effort.is_empty());
    assert_eq!(s.tickets.into_iter().collect::<Vec<_>>(), [("A1".to_string(), "ABC-1".to_string())]);
    assert_eq!(s.order.into_iter().collect::<Vec<_>>(), [("A".to_string(), vec!["A2".to_string(), "A1".to_string()])]);
    assert_eq!(s.win_order, ["deck", "api"]);
}

#[test]
fn saved_runtime_files_survive_a_reload() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = BindStore::load(dir.path());
    s.bindings.insert("A".into(), "deck".into());
    s.effort.insert("A1".into(), "ultracode".into());
    s.win_order = vec!["deck".into()];
    s.settings.insert("terminal_home".into(), "C:/Repos".into());
    s.save_bindings();
    s.save_effort();
    s.save_win_order();
    s.save_settings();
    let back = BindStore::load(dir.path());
    assert_eq!(back.bindings["A"], "deck");
    assert_eq!(back.effort["A1"], "ultracode");
    assert_eq!(back.win_order, ["deck"]);
    assert_eq!(back.settings["terminal_home"], "C:/Repos");
}

#[test]
fn write_state_keeps_extra_fields_and_flips_only_status_and_time() {
    let dir = tempfile::tempdir().unwrap();
    save_json(&paths::state_path(dir.path(), "A1"), &json!({"slot": "A1", "status": "done", "ts": 1.0, "prompt": "Hallo"})).unwrap();
    write_state(dir.path(), "A1", "idle", 5.5).unwrap();
    let back = load_json(&paths::state_path(dir.path(), "A1")).unwrap();
    assert_eq!(back, json!({"slot": "A1", "status": "idle", "ts": 5.5, "prompt": "Hallo"}));
    clear_state(dir.path(), "A1");
    clear_state(dir.path(), "A1");
    assert!(load_json(&paths::state_path(dir.path(), "A1")).is_none());
}

#[test]
fn read_all_skips_live_files_pidmaps_and_broken_files() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    save_json(&d.join("A1.json"), &json!({"slot": "A1", "status": "done"})).unwrap();
    save_json(&d.join("B2.json"), &json!({"status": "idle"})).unwrap();
    save_json(&d.join("A1.live.json"), &json!({"model": "Opus"})).unwrap();
    save_json(&d.join("pidmap-A.json"), &json!({"123": "A1"})).unwrap();
    std::fs::write(d.join("C3.json"), "{halb").unwrap();
    let all = read_all(d);
    let mut keys: Vec<&String> = all.keys().collect();
    keys.sort();
    assert_eq!(keys, ["A1", "B2"]);
    assert_eq!(read_live(d)["A1"]["model"], Value::from("Opus"));
    assert!(read_all(&d.join("fehlt")).is_empty());
}

#[test]
fn marker_files_yield_their_first_line_trimmed_and_capped() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    std::fs::write(d.join("A1.ticket"), "  ORPH-1  \r\nzweite\n").unwrap();
    std::fs::write(d.join("A2.ticket"), "   \n").unwrap();
    std::fs::write(d.join("A3.ticket"), "x".repeat(80)).unwrap();
    std::fs::write(d.join("A1.worktree"), "C:/Repos/wt 1\n").unwrap();
    let t = read_found_tickets(d);
    assert_eq!(t["A1"], "ORPH-1");
    assert!(!t.contains_key("A2"));
    assert_eq!(t["A3"].len(), 60);
    assert_eq!(read_found_worktrees(d)["A1"], "C:/Repos/wt 1");
}
