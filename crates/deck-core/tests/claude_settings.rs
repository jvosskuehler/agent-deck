//! settings.json: nur die gesteuerten Schlüssel anfassen, Hooks und statusLine überleben
//! (Python `test_claude_settings.py`).

use std::fs;
use std::time::{Duration, SystemTime};

use deck_core::claude::settings::*;
use deck_core::i18n::{Lang, status_label};
use serde_json::{Value, json};

fn set_mtime(p: &std::path::Path, secs: u64) {
    let f = fs::OpenOptions::new().write(true).open(p).unwrap();
    f.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs)).unwrap();
}

#[test]
fn write_values_merges_only_the_controlled_keys_and_keeps_permissions_allow() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("settings.json");
    fs::write(&p, r#"{"hooks": {"Stop": [1]}, "statusLine": {"x": 1}, "model": "sonnet", "permissions": {"allow": ["Read"], "defaultMode": "plan"}}"#).unwrap();
    write_values(&WriteValues { model: Some("opus[1m]"), mode: Some("auto"), effort: Some("xhigh"), language: Some("english"), ..Default::default() }, &p).unwrap();
    let d: Value = serde_json::from_str(&fs::read_to_string(&p).unwrap()).unwrap();
    assert_eq!(d["model"], "opus[1m]");
    assert_eq!(d["effortLevel"], "xhigh");
    assert_eq!(d["language"], "english");
    assert_eq!(d["permissions"], json!({"allow": ["Read"], "defaultMode": "auto"}));
    assert_eq!(d["hooks"], json!({"Stop": [1]}));
    assert_eq!(d["statusLine"], json!({"x": 1}));
}

#[test]
fn writing_keeps_the_key_order_of_the_hand_kept_file() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("settings.json");
    fs::write(&p, r#"{"zeta": 1, "model": "sonnet", "alpha": 2}"#).unwrap();
    write_values(&WriteValues { model: Some("fable"), ..Default::default() }, &p).unwrap();
    assert_eq!(fs::read_to_string(&p).unwrap(), "{\n  \"zeta\": 1,\n  \"model\": \"fable\",\n  \"alpha\": 2\n}");
}

#[test]
fn a_missing_file_is_created_and_untouched_keys_stay_absent() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("nested").join("settings.json");
    write_values(&WriteValues { model: Some("fable"), ..Default::default() }, &p).unwrap();
    let d: Value = serde_json::from_str(&fs::read_to_string(&p).unwrap()).unwrap();
    assert_eq!(d, json!({"model": "fable"}));
    let v = read_values(&p);
    assert_eq!(v.model, Some(json!("fable")));
    assert_eq!(v.mode, None);
}

#[test]
fn reading_a_missing_file_is_safe() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(read_values(&dir.path().join("nope.json")), ClaudeValues::default());
}

#[test]
fn default_mode_rereads_only_after_a_change_and_keeps_the_last_good_value() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("settings.json");
    let mut cache = DefaultModeCache::new();
    assert_eq!(cache.get(&p), None);
    write_values(&WriteValues { mode: Some("bypassPermissions"), ..Default::default() }, &p).unwrap();
    assert_eq!(cache.get(&p), Some(json!("bypassPermissions")));
    fs::write(&p, r#"{"permissions": {"defaultMo"#).unwrap();
    set_mtime(&p, 1000);
    assert_eq!(cache.get(&p), Some(json!("bypassPermissions")), "halb geschrieben: letzter guter Stand");
    write_values(&WriteValues { mode: Some("auto"), ..Default::default() }, &p).unwrap();
    set_mtime(&p, 2000);
    assert_eq!(cache.get(&p), Some(json!("auto")));
    fs::write(&p, r#"{"model": "fable"}"#).unwrap();
    set_mtime(&p, 3000);
    assert_eq!(cache.get(&p), None);
}

#[test]
fn model_choices_stay_version_free() {
    let version = regex::Regex::new(r"\d+[.\-]\d").unwrap();
    let label_version = regex::Regex::new(r"[A-Za-z]\s+\d").unwrap();
    for (label, val) in MODEL_CHOICES {
        let base = val.split('[').next().unwrap();
        assert!(["opus", "fable", "sonnet", "haiku"].contains(&base), "{val}");
        assert!(!version.is_match(val), "{val}");
        assert!(!label_version.is_match(label), "{label}");
    }
}

#[test]
fn effort_labels_map_to_level_and_ultracode_wins() {
    assert_eq!(effort_spec("max"), ("max", false));
    assert_eq!(effort_spec("Ultracode"), ("xhigh", true));
    assert_eq!(effort_spec("xhigh"), ("xhigh", false));
    assert_eq!(effort_spec("unbekannt"), ("xhigh", false));
    assert_eq!(effort_label(Some("max"), false), "max");
    assert_eq!(effort_label(Some("xhigh"), true), "Ultracode");
    assert_eq!(effort_label(Some("high"), false), "high");
    assert_eq!(effort_label(Some("bogus"), false), "xhigh");
}

#[test]
fn effort_and_ultracode_round_trip_through_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("settings.json");
    write_values(&WriteValues { effort: Some("xhigh"), ultracode: Some(true), ..Default::default() }, &p).unwrap();
    let v = read_values(&p);
    assert_eq!((v.effort, v.ultracode), (Some(json!("xhigh")), true));
    write_values(&WriteValues { effort: Some("max"), ultracode: Some(false), ..Default::default() }, &p).unwrap();
    let v = read_values(&p);
    assert_eq!((v.effort, v.ultracode), (Some(json!("max")), false));
}

#[test]
fn the_deck_language_follows_the_agent_language_and_defaults_to_german() {
    assert_eq!(Lang::normalize(Some(&json!("english"))), Lang::English);
    assert_eq!(Lang::normalize(Some(&json!("EN-us"))), Lang::English);
    assert_eq!(Lang::normalize(Some(&json!("german"))), Lang::German);
    assert_eq!(Lang::normalize(None), Lang::German);
    assert_eq!(Lang::normalize(Some(&json!("klingon"))), Lang::German);
    assert_eq!(Lang::English.l("Speichern", "Save"), "Save");
    assert_eq!(Lang::German.l("Speichern", "Save"), "Speichern");
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("settings.json");
    assert_eq!(Lang::from_settings(&p), Lang::German);
    write_values(&WriteValues { language: Some("english"), ..Default::default() }, &p).unwrap();
    assert_eq!(Lang::from_settings(&p), Lang::English);
}

#[test]
fn status_labels_are_localized_and_running_reads_as_thinking() {
    assert_eq!(status_label(Lang::German, "waiting"), "Rückfrage");
    assert_eq!(status_label(Lang::English, "done"), "unread");
    assert_eq!(status_label(Lang::German, "running"), "denkt");
    assert_eq!(status_label(Lang::German, "lost"), "getrennt");
    assert_eq!(status_label(Lang::German, "bogus"), "idle");
}
