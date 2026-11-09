//! statusline (aus Python `test_hooks_report.py`, Teil Statuszeile): Kopfzeile mit Slot,
//! Ticket und Überschrift, Zahlenzeile, Live-Datei mit festen Feldern und Zahlentypen.

use std::path::Path;

use deck_core::i18n::Lang;
use deck_core::paths::{live_path, load_json, save_json};
use deck_hooks::env::HookEnv;
use deck_hooks::statusline::*;
use serde_json::{Map, Value, json};

fn env_in(dir: &Path, slot: Option<&str>) -> HookEnv {
    HookEnv {
        state_dir: dir.join("state"),
        app_dir: dir.to_path_buf(),
        claude_settings: dir.join("settings.json"),
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
    let out = line(&rec, "B2 · ORPH-4116", Lang::German);
    let (head, body) = out.split_once('\n').unwrap();
    assert_eq!(head, "\x1b[1mB2 · ORPH-4116 · Schrift-Einstellung im Client\x1b[0m");
    assert_eq!(body, "opus  ·  effort xhigh  ·  ctx 41%  ·  $1.50");
    assert_eq!(line(&obj(json!({"model": "opus"})), "", Lang::German), "opus");
    let long = line(&obj(json!({"session_name": "x".repeat(200)})), "B1", Lang::German);
    assert!(long.chars().count() < 100 && long.contains('…'));
}

#[test]
fn percent_rounds_to_whole_numbers_and_cost_to_cents() {
    assert_eq!(line(&obj(json!({"ctx_pct": 2.5, "cost_usd": 0.125})), "", Lang::German), "ctx 2%  ·  $0.12");
    assert_eq!(line(&obj(json!({"ctx_pct": 41, "cost_usd": 0})), "", Lang::German), "ctx 41%  ·  $0.00");
    assert_eq!(line(&obj(json!({"ctx_pct": null, "model": "", "effort": null})), "", Lang::German), "");
}

#[test]
fn the_label_takes_the_assigned_ticket_first_then_the_reported_one() {
    let dir = tempfile::tempdir().unwrap();
    let env = env_in(dir.path(), None);
    std::fs::create_dir_all(&env.state_dir).unwrap();
    std::fs::write(env.state_dir.join("Z9.ticket"), "ORPH-1\n").unwrap();
    assert_eq!(label("Z9", &env), "Z9 · ORPH-1");
    assert_eq!(label("Z8", &env), "Z8");
    save_json(&dir.path().join("tickets.json"), &json!({"Z9": "ORPH-2"})).unwrap();
    assert_eq!(label("Z9", &env), "Z9 · ORPH-2");
    save_json(&dir.path().join("tickets.json"), &json!(["kaputt"])).unwrap();
    assert_eq!(label("Z9", &env), "Z9 · ORPH-1", "eine kaputte tickets.json zählt nicht");
}

#[test]
fn live_values_keep_their_json_types_and_field_order() {
    let data = obj(json!({"model": {"display_name": "Opus 5", "id": "claude-opus-5"}, "effort": {"level": "xhigh"},
        "context_window": {"used_percentage": 41, "current_usage": {"input_tokens": 10, "output_tokens": 2.5}},
        "cost": {"total_cost_usd": 1.5}, "session_name": "Titel"}));
    let rec = extract(&data, 7.0);
    assert_eq!(Value::Object(rec.clone()), json!({"model": "Opus 5", "effort": "xhigh", "ctx_pct": 41, "ctx_tokens": 10,
        "msg_tokens": 13, "cost_usd": 1.5, "session_name": "Titel", "cache_until": null, "cache_ttl": null, "limits": {}, "pr": null, "ts": 7.0}));
    assert_eq!(
        rec.keys().collect::<Vec<_>>(),
        ["model", "effort", "ctx_pct", "ctx_tokens", "msg_tokens", "cost_usd", "session_name", "cache_until", "cache_ttl", "limits", "pr", "ts"]
    );
}

#[test]
fn message_tokens_fall_back_to_total_output_and_a_missing_part_counts_as_zero() {
    let cw = |v: Value| extract(&obj(json!({"context_window": v})), 0.0)["msg_tokens"].clone();
    assert_eq!(cw(json!({"current_usage": {"input": 3, "output": 4}})), json!(7));
    assert_eq!(cw(json!({"current_usage": {"input_tokens": 0.0, "output_tokens": 5}})), json!(5));
    assert_eq!(cw(json!({"total_output_tokens": 9})), json!(9));
    assert_eq!(cw(json!({})), json!(null));
    assert_eq!(cw(json!({"current_usage": {"input": "3", "output": 4}})), json!(4), "ein Text ist keine Zahl");
    assert_eq!(cw(json!({"current_usage": [1], "total_output_tokens": 9})), json!(9), "kein Objekt zählt als fehlend");
}

#[test]
fn a_run_writes_the_live_file_and_always_returns_a_line() {
    let dir = tempfile::tempdir().unwrap();
    let env = env_in(dir.path(), Some("A1"));
    let stdin = json!({"model": {"display_name": "Opus"}, "session_name": "Eigenes Terminal für Claude Code"}).to_string();
    let out = run(&env, Some(stdin.as_bytes()));
    assert_eq!(out, "\x1b[1mA1 · Eigenes Terminal für Claude Code\x1b[0m\nOpus");
    assert_eq!(load_json(&live_path(&env.state_dir, "A1")).unwrap()["model"], "Opus");
    let no_slot = env_in(dir.path(), None);
    assert_eq!(run(&no_slot, Some(b"{}")), "");
    assert_eq!(run(&no_slot, None), "");
    assert_eq!(run(&no_slot, Some(b"[1]")), "", "kein Objekt: wie leeres stdin");
    let junk = json!({"model": 5, "effort": {"level": 3}, "context_window": "x", "cost": {"total_cost_usd": "1"}, "session_name": 7});
    assert_eq!(run(&no_slot, Some(junk.to_string().as_bytes())), "", "falsche Typen fallen weg, statt zu scheitern");
}

// ── Kontingent und PR (aus Python `test_hooks_report.py`, Stand 2856494) ──────────────────

#[test]
fn the_quota_stands_in_the_number_line_and_drops_out_behind_its_reset() {
    // Claude Code kennt nur den Stand der letzten Antwort. Nach dem Reset wäre die Zahl
    // eine Behauptung, also fällt sie weg (wie `expire_limits` im Deck).
    let now = 1_794_180_000.0;
    let raw = json!({"rate_limits": {"five_hour": {"used_percentage": 18, "resets_at": now + 600.0},
        "seven_day": {"used_percentage": 3, "resets_at": now + 86400.0},
        "spend_limit": {"used_percentage": 99}}});
    let rec = extract(&obj(raw), now);
    let limits = rec["limits"].clone();
    assert_eq!(limits.as_object().unwrap().keys().collect::<Vec<_>>(), ["five_hour", "seven_day"], "unbekannte Limits nicht");
    assert_eq!(limits["five_hour"], json!({"pct": 18, "resets_at": now + 600.0}));
    assert_eq!(limits_part(Some(&limits), now), "5h 18% · 7d 3%");
    assert_eq!(limits_part(Some(&limits), now + 601.0), "7d 3%", "das 5h-Fenster ist um");
    let high = extract(&obj(json!({"rate_limits": {"five_hour": {"used_percentage": 63}, "seven_day": {"used_percentage": 85}}})), now);
    let part = limits_part(high.get("limits"), now);
    assert!(part.contains("\x1b[33m5h 63%") && part.contains("\x1b[31m7d 85%"), "gelb ab 50, rot ab 80: {part:?}");
    assert_eq!(limits_part(Some(&json!({})), now), "");
    assert_eq!(limits_part(None, now), "");
    let line_rec = obj(json!({"model": "opus", "cost_usd": 1.0, "ts": now, "limits": limits}));
    assert_eq!(line(&line_rec, "", Lang::German), "opus  ·  5h 18% · 7d 3%  ·  $1.00");
}

#[test]
fn the_pr_stands_in_the_header_and_shortens_the_title() {
    let pr = extract(&obj(json!({"pr": {"number": 347, "url": "u", "review_state": "approved", "kind": "x"}})), 0.0)["pr"].clone();
    assert_eq!(pr, json!({"number": 347, "url": "u", "review_state": "approved"}));
    let head = header("B2 · ORPH-4116", Some("Schrift"), Some(&pr), Lang::German);
    assert!(head.starts_with("\x1b[1mB2 · ORPH-4116 · PR #347 \x1b[32mfreigegeben\x1b[39m · Schrift"), "{head:?}");
    assert_eq!(head.matches("\x1b[0m").count(), 1, "fett bleibt bis zum Ende");
    let long = "x".repeat(200);
    let without = header("B2", Some(&long), None, Lang::German);
    let with = header("B2", Some(&long), Some(&json!({"number": 1, "review_state": "changes_requested"})), Lang::German);
    let visible = |s: &str| s.replace("\x1b[33m", "").replace("\x1b[39m", "").chars().count();
    assert!(visible(&with) <= visible(&without), "die Zeile wird nicht länger");
    assert!(with.contains("Änderungen gewünscht"));
    assert!(header("", Some(""), Some(&json!({"number": 5, "review_state": "pending"})), Lang::German).contains("PR #5 offen"));
    assert!(header("", None, Some(&json!({"number": 5, "review_state": "draft"})), Lang::English).contains("PR #5 draft"));
    assert_eq!(header("", Some(""), Some(&json!({"number": 5})), Lang::German), "\x1b[1mPR #5\x1b[0m");
    for junk in [json!({"number": "347"}), json!({"number": 3.5}), json!({"url": "u"}), json!("PR")] {
        assert!(extract(&obj(json!({"pr": junk})), 0.0)["pr"].is_null(), "keine ganze Zahl, kein PR: {junk}");
    }
}

#[test]
fn a_run_shows_the_pr_in_the_language_of_the_settings_and_writes_quota_and_pr() {
    let dir = tempfile::tempdir().unwrap();
    let env = env_in(dir.path(), Some("A1"));
    std::fs::write(&env.claude_settings, r#"{"language": "english"}"#).unwrap();
    let stdin = json!({"session_name": "T", "pr": {"number": 9, "url": "u", "review_state": "approved"},
        "rate_limits": {"five_hour": {"used_percentage": 20, "resets_at": 99999.0}}}).to_string();
    let out = run(&env, Some(stdin.as_bytes()));
    assert!(out.starts_with("\x1b[1mA1 · PR #9 \x1b[32mapproved\x1b[39m · T"), "{out:?}");
    assert!(out.ends_with("5h 20%"), "{out:?}");
    let live = load_json(&live_path(&env.state_dir, "A1")).unwrap();
    assert_eq!(live["pr"], json!({"number": 9, "url": "u", "review_state": "approved"}));
    assert_eq!(live["limits"], json!({"five_hour": {"pct": 20, "resets_at": 99999.0}}));
}
