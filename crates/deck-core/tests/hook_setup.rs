//! Der Hook-Merge in Claudes settings.json (Python `test_claude_hook_setup.py`), auf die
//! neuen Programme übertragen, dazu die Erkennung der alten Python-Einträge.
//!
//! Diese Datei fasst die eine Stelle an, an der ein Fehler den AGENTEN blockiert: ein
//! kaputter Eintrag unter UserPromptSubmit oder PreToolUse gilt als Veto.

use deck_core::claude::hook_setup::*;
use serde_json::{Map, Value, json};

const BIN: &str = r"C:\Users\x\AppData\Local\agent-deck";
const OTHER: &str = r"D:\alt\agent-deck";

fn obj(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

fn fremd() -> Value {
    json!({"hooks": [{"type": "command", "command": "python C:\\tools\\lint.py"}]})
}

fn legacy_cmd(status: &str) -> String {
    format!("python \"C:\\Users\\x\\Repos\\agent-deck\\report.py\" {status} || exit 0")
}

fn cmds(s: &Map<String, Value>, event: &str) -> Vec<String> {
    s["hooks"][event]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|g| g["hooks"].as_array().unwrap().iter().map(|h| h["command"].as_str().unwrap().to_string()))
        .collect()
}

fn merged(s: Value) -> (Map<String, Value>, Vec<Note>) {
    merge(&obj(s), BIN, MergeOptions::default())
}

#[test]
fn a_fresh_file_gets_all_six_hooks_and_the_statusline() {
    let (out, notes) = merged(json!({}));
    let events: Vec<&String> = out["hooks"].as_object().unwrap().keys().collect();
    assert_eq!(events, HOOKS.iter().map(|h| h.0).collect::<Vec<_>>());
    assert_eq!(out["statusLine"]["command"], statusline_command(BIN));
    assert_eq!(notes.len(), HOOKS.len() + 1);
}

#[test]
fn every_entry_ends_with_the_outer_shell_and_has_no_cmd_c() {
    let (out, _) = merged(json!({}));
    for (event, status, _) in HOOKS {
        for c in cmds(&out, event) {
            assert!(c.ends_with("|| exit 0"), "{event}");
            assert!(!c.to_lowercase().contains("cmd /c"));
            assert_eq!(c, format!("\"C:/Users/x/AppData/Local/agent-deck/deck-report.exe\" {status} || exit 0"));
        }
    }
    assert!(out["statusLine"]["command"].as_str().unwrap().ends_with("deck-statusline.exe\" || exit 0"));
}

#[test]
fn only_the_two_tool_events_have_a_matcher() {
    let (out, _) = merged(json!({}));
    for (event, _, matcher) in HOOKS {
        let has = out["hooks"][event].as_array().unwrap().iter().any(|g| g.get("matcher").is_some());
        assert_eq!(has, matcher.is_some(), "{event}");
    }
}

#[test]
fn a_second_run_changes_nothing() {
    let (once, _) = merged(json!({}));
    let (twice, notes) = merge(&once, BIN, MergeOptions::default());
    assert_eq!(twice, once);
    assert!(notes.is_empty());
}

#[test]
fn foreign_hooks_stay_in_other_events_and_next_to_ours() {
    let (out, _) = merged(json!({"hooks": {"PreCompact": [fremd()], "Stop": [fremd()]}}));
    assert_eq!(out["hooks"]["PreCompact"], json!([fremd()]));
    let stop = cmds(&out, "Stop");
    assert!(stop.contains(&"python C:\\tools\\lint.py".to_string()));
    assert!(stop.contains(&hook_command(BIN, "done")));
}

#[test]
fn a_foreign_command_in_our_group_survives() {
    let before = json!({"hooks": {"Stop": [{"hooks": [
        {"type": "command", "command": hook_command(OTHER, "done")},
        {"type": "command", "command": "python C:\\tools\\lint.py"}]}]}});
    let (out, _) = merged(before);
    let stop = cmds(&out, "Stop");
    assert!(stop.contains(&"python C:\\tools\\lint.py".to_string()));
    assert!(stop.contains(&hook_command(BIN, "done")));
    assert!(!stop.contains(&hook_command(OTHER, "done")));
}

#[test]
fn a_moved_program_folder_is_replaced_not_doubled() {
    let (old, _) = merge(&Map::new(), OTHER, MergeOptions::default());
    let (new, notes) = merge(&old, BIN, MergeOptions::default());
    for (event, status, _) in HOOKS {
        assert_eq!(cmds(&new, event), [hook_command(BIN, status)], "{event}");
    }
    assert!(notes.iter().any(|(_, t)| t.contains("ersetzt")));
}

#[test]
fn an_empty_foreign_group_is_not_cleaned_up() {
    let (out, _) = merged(json!({"hooks": {"Stop": [{"matcher": "*", "hooks": []}]}}));
    assert!(out["hooks"]["Stop"].as_array().unwrap().contains(&json!({"matcher": "*", "hooks": []})));
}

#[test]
fn a_foreign_statusline_stays_with_a_warning_unless_forced() {
    let before = json!({"statusLine": {"type": "command", "command": "python C:\\tools\\bar.py"}});
    let (out, notes) = merged(before.clone());
    assert_eq!(out["statusLine"], before["statusLine"]);
    assert_eq!(notes.iter().filter(|(g, _)| *g == "warn").count(), 1);
    let (forced, _) = merge(&obj(before), BIN, MergeOptions { force_statusline: true, ..Default::default() });
    assert_eq!(forced["statusLine"]["command"], statusline_command(BIN));
}

#[test]
fn done_is_info_and_left_out_is_warn() {
    let (_, notes) = merged(json!({}));
    assert!(notes.iter().all(|(g, _)| *g == "info"));
}

#[test]
fn our_own_statusline_with_an_extra_tail_is_not_rewritten_but_one_from_elsewhere_is() {
    let own = format!("{} --x", statusline_command(BIN));
    let (out, notes) = merged(json!({"statusLine": {"type": "command", "command": own}}));
    assert_eq!(out["statusLine"]["command"], own);
    assert!(!notes.iter().any(|(_, t)| t.contains("statusLine")));
    let (out, _) = merged(json!({"statusLine": {"type": "command", "command": statusline_command(OTHER)}}));
    assert_eq!(out["statusLine"]["command"], statusline_command(BIN));
}

#[test]
fn other_keys_stay_untouched_and_the_input_is_not_mutated() {
    let before = obj(json!({"model": "opus[1m]", "permissions": {"allow": ["Bash(git*)"]}, "hooks": {"Stop": [fremd()]}}));
    let copy = before.clone();
    let (out, _) = merge(&before, BIN, MergeOptions::default());
    assert_eq!(before, copy);
    assert_eq!(out["model"], "opus[1m]");
    assert_eq!(out["permissions"], json!({"allow": ["Bash(git*)"]}));
}

#[test]
fn legacy_python_entries_are_recognized_and_replaced_by_default() {
    let before = json!({"hooks": {"Stop": [{"hooks": [{"type": "command", "command": legacy_cmd("done")}]}]},
        "statusLine": {"type": "command", "command": "python \"C:\\Users\\x\\Repos\\agent-deck\\statusline.py\""}});
    assert!(has_legacy(&obj(before.clone())));
    let (out, _) = merged(before);
    assert_eq!(cmds(&out, "Stop"), [hook_command(BIN, "done")]);
    assert_eq!(out["statusLine"]["command"], statusline_command(BIN));
    assert!(!has_legacy(&out));
}

#[test]
fn during_the_switch_legacy_entries_can_stay_until_the_new_path_is_proven() {
    let before = json!({"hooks": {"Stop": [{"hooks": [{"type": "command", "command": legacy_cmd("done")}]}]},
        "statusLine": {"type": "command", "command": "python \"C:\\x\\statusline.py\""}});
    let (out, notes) = merge(&obj(before), BIN, MergeOptions { keep_legacy: true, ..Default::default() });
    assert_eq!(cmds(&out, "Stop"), [legacy_cmd("done"), hook_command(BIN, "done")]);
    assert_eq!(out["statusLine"]["command"], "python \"C:\\x\\statusline.py\"");
    assert!(notes.iter().any(|(g, _)| *g == "warn"));
    let (clean, _) = remove_legacy(&out);
    assert_eq!(cmds(&clean, "Stop"), [hook_command(BIN, "done")]);
    assert!(clean.get("statusLine").is_none());
}

#[test]
fn remove_takes_out_only_our_entries() {
    let (full, _) = merged(json!({"hooks": {"Stop": [fremd()]}}));
    let (out, notes) = remove(&full);
    assert_eq!(out["hooks"], json!({"Stop": [fremd()]}));
    assert!(out.get("statusLine").is_none());
    assert!(!notes.is_empty());
    let (empty, _) = remove(&merged(json!({})).0);
    assert!(empty.is_empty());
    let foreign = obj(json!({"statusLine": {"type": "command", "command": "python C:\\tools\\bar.py"}}));
    assert_eq!(remove(&foreign).0, foreign);
}

#[test]
fn ownership_is_recognized_by_file_name_with_boundaries() {
    assert_eq!(owner("python C:\\tools\\my_report.py"), Owner::Foreign);
    assert_eq!(owner("python C:\\tools\\report.pyc"), Owner::Foreign);
    assert_eq!(owner("python \"C:\\x\\report.py\" idle || exit 0"), Owner::Legacy);
    assert_eq!(owner("python C:/x/statusline.py"), Owner::Legacy);
    assert_eq!(owner("\"C:/x/deck-report.exe\" idle || exit 0"), Owner::Deck);
    assert_eq!(owner("C:\\x\\deck-statusline.exe"), Owner::Deck);
    assert_eq!(owner("C:\\x\\my-deck-report.exe"), Owner::Foreign);
}

#[test]
fn the_program_path_is_read_with_and_without_quotes() {
    assert_eq!(command_path("python \"C:\\a b\\report.py\" idle").as_deref(), Some("C:\\a b\\report.py"));
    assert_eq!(command_path("python C:\\a\\report.py idle").as_deref(), Some("C:\\a\\report.py"));
    assert_eq!(command_path("\"C:/a b/deck-report.exe\" idle || exit 0").as_deref(), Some("C:/a b/deck-report.exe"));
    assert_eq!(command_path("echo nichts"), None);
}

fn audit_ok(s: &Map<String, Value>) -> Vec<Finding> {
    audit(s, BIN, &|_| true, &|p| p.to_string())
}

#[test]
fn a_complete_set_yields_exactly_one_ok() {
    let f = audit_ok(&merged(json!({})).0);
    assert_eq!(f, [("ok", format!("6 Hooks und die statusLine zeigen auf {BIN}"))]);
}

#[test]
fn a_missing_hook_cmd_c_and_a_missing_tail_are_failures() {
    let mut s = merged(json!({})).0;
    s["hooks"].as_object_mut().unwrap().shift_remove("Notification");
    s["hooks"]["Stop"] = json!([{"hooks": [{"type": "command", "command": format!("cmd /c {}", hook_command(BIN, "done"))}]}]);
    s["hooks"]["UserPromptSubmit"] = json!([{"hooks": [{"type": "command", "command": "\"C:/Users/x/AppData/Local/agent-deck/deck-report.exe\" thinking"}]}]);
    let f = audit_ok(&s);
    assert!(f.iter().any(|(g, t)| *g == "fail" && t.contains("Notification")));
    assert!(f.iter().any(|(g, t)| *g == "fail" && t.contains("cmd /c")));
    assert!(f.iter().any(|(g, t)| *g == "fail" && t.contains("exit 0")));
}

#[test]
fn a_missing_program_fails_and_another_folder_warns() {
    let s = merged(json!({})).0;
    assert!(audit(&s, BIN, &|_| false, &|p| p.to_string()).iter().any(|(g, t)| *g == "fail" && t.contains("gibt es nicht")));
    let other = merge(&Map::new(), OTHER, MergeOptions::default()).0;
    let f = audit_ok(&other);
    assert!(f.iter().any(|(g, t)| *g == "warn" && t.contains("anderen Ordner")));
    assert!(!f.iter().any(|(g, _)| *g == "fail"));
}

#[test]
fn short_and_long_windows_paths_are_the_same_folder() {
    let s = merge(&Map::new(), r"C:\Users\JORRIT~1\deck", MergeOptions::default()).0;
    let f = audit(&s, r"C:\Users\Jorrit-lang\deck", &|_| true, &|p| p.replace("JORRIT~1", "Jorrit-lang"));
    assert_eq!(f, [("ok", r"6 Hooks und die statusLine zeigen auf C:\Users\Jorrit-lang\deck".to_string())]);
}

#[test]
fn two_reporters_under_one_event_warn_also_when_one_is_the_old_python_hook() {
    let mut s = merged(json!({})).0;
    s["hooks"]["Stop"].as_array_mut().unwrap().push(json!({"hooks": [{"type": "command", "command": hook_command(OTHER, "done")}]}));
    assert!(audit_ok(&s).iter().any(|(g, t)| *g == "warn" && t.contains("gegeneinander")));
    let mut s = merged(json!({})).0;
    s["hooks"]["Stop"].as_array_mut().unwrap().push(json!({"hooks": [{"type": "command", "command": legacy_cmd("done")}]}));
    assert!(audit_ok(&s).iter().any(|(g, t)| *g == "warn" && t.contains("report.py")));
}

#[test]
fn a_missing_or_legacy_statusline_is_a_warning_not_a_failure() {
    let mut s = merged(json!({})).0;
    s.shift_remove("statusLine");
    let f = audit_ok(&s);
    assert!(f.iter().any(|(g, t)| *g == "warn" && t.contains("statusLine")));
    assert!(!f.iter().any(|(g, _)| *g == "fail"));
    s.insert("statusLine".into(), json!({"type": "command", "command": "python C:\\x\\statusline.py"}));
    assert!(audit_ok(&s).iter().any(|(g, t)| *g == "warn" && t.contains("Python")));
}

#[test]
fn an_empty_settings_file_reports_every_hook_separately() {
    let f = audit_ok(&Map::new());
    assert_eq!(f.iter().filter(|(g, _)| *g == "fail").count(), HOOKS.len());
}
