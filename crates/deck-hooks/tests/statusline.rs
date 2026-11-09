//! statusline (aus Python `test_hooks_report.py`, Teil Statuszeile): Kopfzeile mit Slot,
//! Ticket und Überschrift, Zahlenzeile, Live-Datei mit festen Feldern und Zahlentypen.

use std::path::Path;

use deck_core::i18n::Lang;
use deck_core::paths::{live_path, load_json, save_json};
use deck_hooks::env::HookEnv;
use deck_core::tile_refs::{RefEntry, TileRefs, refs_path};
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
    }
}

fn obj(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

fn none() -> TileRefs {
    TileRefs::default()
}

/// Ticket (und PR) wie vom Deck geschrieben; `detected` = nur geraten.
fn refs(ticket: Option<(&str, bool)>, pr: Option<(&str, bool)>) -> TileRefs {
    let e = |(t, d): (&str, bool)| RefEntry { text: t.into(), source: None, detected: d };
    TileRefs { ticket: ticket.map(e), pr: pr.map(e) }
}

#[test]
fn the_status_line_starts_with_slot_ticket_and_title_in_bold() {
    let rec = obj(json!({"model": "opus", "effort": "xhigh", "ctx_pct": 41.2, "cost_usd": 1.5, "session_name": "Schrift-Einstellung im Client"}));
    let out = line(&rec, "B2", &refs(Some(("ORPH-4116", false)), None), Lang::German);
    let (head, body) = out.split_once('\n').unwrap();
    assert_eq!(head, "\x1b[1mB2 · ORPH-4116 · Schrift-Einstellung im Client\x1b[0m");
    assert_eq!(body, "opus  ·  effort xhigh  ·  ctx 41%", "Kosten stehen nur in live.json");
    assert_eq!(line(&obj(json!({"model": "opus"})), "", &none(), Lang::German), "opus");
    let long = line(&obj(json!({"session_name": "x".repeat(200)})), "B1", &none(), Lang::German);
    assert!(long.chars().count() < 100 && long.contains('…'));
}

#[test]
fn percent_rounds_to_whole_numbers_and_cost_stays_out_of_the_line() {
    assert_eq!(line(&obj(json!({"ctx_pct": 2.5, "cost_usd": 0.125})), "", &none(), Lang::German), "ctx 2%");
    assert_eq!(line(&obj(json!({"ctx_pct": 41, "cost_usd": 0})), "", &none(), Lang::German), "ctx 41%");
    assert_eq!(line(&obj(json!({"ctx_pct": null, "model": "", "effort": null})), "", &none(), Lang::German), "");
}

#[test]
fn the_ticket_comes_only_from_the_decks_refs_file() {
    let dir = tempfile::tempdir().unwrap();
    let env = env_in(dir.path(), None);
    std::fs::create_dir_all(&env.state_dir).unwrap();
    // Was der Hook früher selbst las, zählt nicht mehr: eine Quelle der Wahrheit.
    std::fs::write(env.state_dir.join("Z9.ticket"), "ORPH-1\n").unwrap();
    save_json(&dir.path().join("tickets.json"), &json!({"Z9": "ORPH-2"})).unwrap();
    assert_eq!(tile_refs("Z9", &env), TileRefs::default());
    save_json(&refs_path(&env.state_dir, "Z9"), &json!({"ticket": "ORPH-3", "ticket_source": "branch", "ticket_detected": false})).unwrap();
    assert_eq!(tile_refs("Z9", &env).ticket.map(|t| t.text).as_deref(), Some("ORPH-3"));
    std::fs::write(refs_path(&env.state_dir, "Z9"), "{kaputt").unwrap();
    assert_eq!(tile_refs("Z9", &env), TileRefs::default(), "eine kaputte Datei zeigt nichts");
}

#[test]
fn live_values_keep_their_json_types_and_field_order() {
    let data = obj(json!({"model": {"display_name": "Opus 5", "id": "claude-opus-5"}, "effort": {"level": "xhigh"},
        "context_window": {"used_percentage": 41, "current_usage": {"input_tokens": 10, "output_tokens": 2.5}},
        "cost": {"total_cost_usd": 1.5}, "session_name": "Titel", "cwd": "C:/r"}));
    let rec = extract(&data, 7.0);
    assert_eq!(Value::Object(rec.clone()), json!({"model": "Opus 5", "effort": "xhigh", "ctx_pct": 41, "ctx_tokens": 10,
        "msg_tokens": 13, "cost_usd": 1.5, "session_name": "Titel", "cache_until": null, "cache_ttl": null, "pr": null,
        "cwd": "C:/r", "ts": 7.0}));
    assert_eq!(
        rec.keys().collect::<Vec<_>>(),
        ["model", "effort", "ctx_pct", "ctx_tokens", "msg_tokens", "cost_usd", "session_name", "cache_until", "cache_ttl", "pr", "cwd", "ts"]
    );
}

#[test]
fn the_live_folder_is_the_current_workspace_dir_and_falls_back_to_cwd() {
    let cwd = |v: Value| extract(&obj(v), 0.0)["cwd"].clone();
    assert_eq!(cwd(json!({"cwd": "C:/a", "workspace": {"current_dir": "C:/wt/4116-ui", "project_dir": "C:/a"}})), json!("C:/wt/4116-ui"));
    assert_eq!(cwd(json!({"cwd": "C:/a", "workspace": {"current_dir": 5}})), json!("C:/a"));
    assert_eq!(cwd(json!({"workspace": "C:/b"})), json!(null), "kein Objekt zählt als fehlend");
    assert_eq!(cwd(json!({})), json!(null));
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

// ── PR (aus Python `test_hooks_report.py`, Stand 2856494) ──────────────────────────────

#[test]
fn the_quota_is_neither_shown_nor_written() {
    // Kontingent zeigt das Deck selbst; in jedem Pane war es nur Rauschen (Nov. 2026).
    let now = 1_794_180_000.0;
    let raw = json!({"model": "opus", "rate_limits": {"five_hour": {"used_percentage": 18, "resets_at": now + 600.0}}});
    let rec = extract(&obj(raw), now);
    assert!(rec.get("limits").is_none(), "niemand liest limits aus live.json");
    assert_eq!(line(&rec, "", &none(), Lang::German), "opus");
}

#[test]
fn the_pr_stands_in_the_header_and_shortens_the_title() {
    let pr = extract(&obj(json!({"pr": {"number": 347, "url": "u", "review_state": "approved", "kind": "x"}})), 0.0)["pr"].clone();
    assert_eq!(pr, json!({"number": 347, "url": "u", "review_state": "approved"}));
    let head = header("B2", &refs(Some(("ORPH-4116", false)), None), Some("Schrift"), Some(&pr), Lang::German);
    assert!(head.starts_with("\x1b[1mB2 · ORPH-4116 · PR #347 \x1b[32mfreigegeben\x1b[39m · Schrift"), "{head:?}");
    assert_eq!(head.matches("\x1b[0m").count(), 1, "fett bleibt bis zum Ende");
    let long = "x".repeat(200);
    let without = header("B2", &none(), Some(&long), None, Lang::German);
    let with = header("B2", &none(), Some(&long), Some(&json!({"number": 1, "review_state": "changes_requested"})), Lang::German);
    let visible = |s: &str| s.replace("\x1b[33m", "").replace("\x1b[39m", "").chars().count();
    assert!(visible(&with) <= visible(&without), "die Zeile wird nicht länger");
    assert!(with.contains("Änderungen gewünscht"));
    assert!(header("", &none(), Some(""), Some(&json!({"number": 5, "review_state": "pending"})), Lang::German).contains("PR #5 offen"));
    assert!(header("", &none(), None, Some(&json!({"number": 5, "review_state": "draft"})), Lang::English).contains("PR #5 draft"));
    assert_eq!(header("", &none(), Some(""), Some(&json!({"number": 5})), Lang::German), "\x1b[1mPR #5\x1b[0m");
    for junk in [json!({"number": "347"}), json!({"number": 3.5}), json!({"url": "u"}), json!("PR")] {
        assert!(extract(&obj(json!({"pr": junk})), 0.0)["pr"].is_null(), "keine ganze Zahl, kein PR: {junk}");
    }
}

#[test]
fn a_run_shows_the_pr_in_the_language_of_the_settings_and_writes_it() {
    let dir = tempfile::tempdir().unwrap();
    let env = env_in(dir.path(), Some("A1"));
    std::fs::write(&env.claude_settings, r#"{"language": "english"}"#).unwrap();
    let stdin = json!({"session_name": "T", "pr": {"number": 9, "url": "u", "review_state": "approved"},
        "rate_limits": {"five_hour": {"used_percentage": 20, "resets_at": 99999.0}}}).to_string();
    let out = run(&env, Some(stdin.as_bytes()));
    assert!(out.starts_with("\x1b[1mA1 · PR #9 \x1b[32mapproved\x1b[39m · T"), "{out:?}");
    assert!(!out.contains("5h"), "{out:?}");
    let live = load_json(&live_path(&env.state_dir, "A1")).unwrap();
    assert_eq!(live["pr"], json!({"number": 9, "url": "u", "review_state": "approved"}));
}

#[test]
fn a_guessed_ticket_or_pr_from_the_deck_stands_dimmed_and_the_claude_code_pr_wins() {
    let guessed = refs(Some(("ORPH-7", true)), Some(("#12", true)));
    assert_eq!(
        header("B3", &guessed, Some("T"), None, Lang::German),
        "\x1b[1mB3 · \x1b[22;2mORPH-7\x1b[22;1m · \x1b[22;2mPR #12\x1b[22;1m · T\x1b[0m"
    );
    let pr = json!({"number": 347, "review_state": "approved"});
    assert_eq!(
        header("B3", &refs(Some(("ORPH-7", false)), Some(("#12", true))), Some("T"), Some(&pr), Lang::German),
        "\x1b[1mB3 · ORPH-7 · PR #347 \x1b[32mfreigegeben\x1b[39m · T\x1b[0m"
    );
}

#[test]
fn ticket_and_pr_shorten_the_title_so_the_line_never_grows() {
    let long = "Überschrift ".repeat(20);
    let visible = |s: &str| {
        let mut out = String::new();
        let mut esc = false;
        for c in s.chars() {
            match (esc, c) {
                (false, '\x1b') => esc = true,
                (true, 'm') => esc = false,
                (false, c) => out.push(c),
                _ => {}
            }
        }
        out.chars().count()
    };
    let bare = header("B3", &none(), Some(&long), None, Lang::German);
    let pr = json!({"number": 347, "review_state": "changes_requested"});
    for r in [refs(Some(("ORPH-4116", false)), None), refs(Some(("ORPH-4116", true)), Some(("#1234", false))), refs(None, Some(("#1", true)))] {
        let with = header("B3", &r, Some(&long), None, Lang::German);
        assert_eq!(visible(&with), visible(&bare), "{with:?}");
        let with_pr = header("B3", &r, Some(&long), Some(&pr), Lang::German);
        assert!(visible(&with_pr) <= visible(&bare), "{with_pr:?}");
        assert!(with.contains('…'));
    }
    // Ein kurzer Titel wird nicht gekürzt, nur weil ein Ticket dazukommt.
    assert!(header("B3", &refs(Some(("ORPH-4116", false)), None), Some("Kurz"), None, Lang::German).ends_with("Kurz\x1b[0m"));
}
