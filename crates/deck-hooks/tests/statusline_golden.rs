//! Goldene Statuszeilen: ein vollständiges stdin, wie Claude Code es schickt, und die
//! Zeilen und `live.json`, die daraus werden. Aufgenommen vor dem Umbau von der
//! Python-Nachbildung auf tolerantes serde-Lesen (Nov. 2026); bei normaler Eingabe darf
//! sich daran nichts ändern.

use std::path::Path;

use deck_core::paths::{live_path, load_json, save_json};
use deck_core::tile_refs::refs_path;
use deck_hooks::env::HookEnv;
use deck_hooks::statusline::run;
use serde_json::{Value, json};

const NOW: f64 = 1_794_180_000.0;

fn env_in(dir: &Path, slot: Option<&str>) -> HookEnv {
    HookEnv {
        state_dir: dir.join("state"),
        app_dir: dir.to_path_buf(),
        claude_settings: dir.join("settings.json"),
        agent_slot: slot.map(String::from),
        cwd: None,
        now: NOW,
    }
}

fn full() -> Value {
    json!({
        "hook_event_name": "Status", "session_id": "abc", "cwd": "C:\\x",
        "workspace": {"current_dir": "C:\\x\\wt\\4116-ui", "project_dir": "C:\\x"},
        "model": {"id": "claude-opus-5-5", "display_name": "Opus 5.5"},
        "effort": {"level": "xhigh"},
        "context_window": {"used_percentage": 41.6, "context_window_size": 1_000_000, "total_output_tokens": 5000,
            "current_usage": {"input_tokens": 3, "output_tokens": 812, "cache_creation_input_tokens": 1200, "cache_read_input_tokens": 415_000}},
        "cost": {"total_cost_usd": 12.3456, "total_duration_ms": 1000},
        "session_name": "Statuszeile für Wörter",
        "prompt_cache": {"warm": true, "ttl": "1h", "expires_at": NOW + 1800.0},
        "rate_limits": {"five_hour": {"used_percentage": 63.4, "resets_at": NOW + 3600.0}, "seven_day": {"used_percentage": 12, "resets_at": 1_794_266_400}},
        "pr": {"number": 12, "url": "https://x/pull/12", "review_state": "changes_requested"}
    })
}

fn run_with(env: &HookEnv, stdin: &Value) -> String {
    run(env, Some(stdin.to_string().as_bytes()))
}

/// `refs.json`, wie das Deck sie schreibt (`deck_core::tile_refs`).
fn refs(ticket: &str, detected: bool, pr: Option<(&str, bool)>) -> Value {
    json!({"ticket": ticket, "ticket_source": if detected { "chat" } else { "branch" }, "ticket_detected": detected,
        "pr": pr.map(|p| p.0), "pr_source": pr.map(|p| if p.1 { "chat" } else { "statusline" }), "pr_detected": pr.is_some_and(|p| p.1)})
}

/// Die Kopfzeile eines Laufs mit `stdin` und (wenn gegeben) der `refs.json` des Decks.
fn head_of(stdin: &Value, refs_json: Option<Value>) -> String {
    let dir = tempfile::tempdir().unwrap();
    let env = env_in(dir.path(), Some("B3"));
    std::fs::create_dir_all(&env.state_dir).unwrap();
    if let Some(r) = refs_json {
        save_json(&refs_path(&env.state_dir, "B3"), &r).unwrap();
    }
    run_with(&env, stdin).split('\n').next().unwrap().to_string()
}

#[test]
fn the_header_shows_the_decks_ticket_with_and_without_pr_and_shortens_a_long_title() {
    let title = "Agent Deck: Ticket aus der Kachel auch in der Statuszeile zeigen, ohne zweite Erkennung im Hook";
    let base = json!({"session_name": title});
    let with_pr = json!({"session_name": title, "pr": {"number": 347, "url": "u", "review_state": "approved"}});
    let cases = [
        // ohne Ticket, ohne PR: der Titel allein, auf 70 Zeichen gekürzt
        (base.clone(), None, "\x1b[1mB3 · Agent Deck: Ticket aus der Kachel auch in der Statuszeile zeigen, ohn…\x1b[0m"),
        // mit Ticket: es steht vorn, der Titel wird um genau so viel kürzer
        (
            base.clone(),
            Some(refs("ORPH-4116", false, None)),
            "\x1b[1mB3 · ORPH-4116 · Agent Deck: Ticket aus der Kachel auch in der Statuszeile…\x1b[0m",
        ),
        // nur geratenes Ticket: gedimmt
        (
            base.clone(),
            Some(refs("ORPH-4116", true, None)),
            "\x1b[1mB3 · \x1b[22;2mORPH-4116\x1b[22;1m · Agent Deck: Ticket aus der Kachel auch in der Statuszeile…\x1b[0m",
        ),
        // Ticket und PR aus dem stdin
        (
            with_pr.clone(),
            Some(refs("ORPH-4116", false, Some(("#12", true)))),
            "\x1b[1mB3 · ORPH-4116 · PR #347 \x1b[32mfreigegeben\x1b[39m · Agent Deck: Ticket aus der Kachel a…\x1b[0m",
        ),
        // PR ohne Ticket
        (with_pr, None, "\x1b[1mB3 · PR #347 \x1b[32mfreigegeben\x1b[39m · Agent Deck: Ticket aus der Kachel auch in der S…\x1b[0m"),
        // kein PR im stdin: der des Decks, hier geraten und gedimmt
        (
            base,
            Some(refs("ORPH-4116", false, Some(("#12", true)))),
            "\x1b[1mB3 · ORPH-4116 · \x1b[22;2mPR #12\x1b[22;1m · Agent Deck: Ticket aus der Kachel auch in der St…\x1b[0m",
        ),
    ];
    for (stdin, refs_json, want) in cases {
        assert_eq!(head_of(&stdin, refs_json.clone()), want, "{refs_json:?}");
    }
}

#[test]
fn a_full_stdin_gives_the_golden_lines_in_both_languages() {
    let dir = tempfile::tempdir().unwrap();
    let env = env_in(dir.path(), Some("A1"));
    std::fs::create_dir_all(&env.state_dir).unwrap();
    save_json(&refs_path(&env.state_dir, "A1"), &refs("ORPH-4116", false, None)).unwrap();
    assert_eq!(
        run_with(&env, &full()),
        "\x1b[1mA1 · ORPH-4116 · PR #12 \x1b[33mÄnderungen gewünscht\x1b[39m · Statuszeile für Wörter\x1b[0m\n\
         Opus 5.5  ·  effort xhigh  ·  ctx 42%  ·  cache 29 min"
    );
    std::fs::write(&env.claude_settings, r#"{"language": "english"}"#).unwrap();
    save_json(&refs_path(&env.state_dir, "A1"), &refs("ORPH-1", false, None)).unwrap();
    assert_eq!(
        run_with(&env, &full()),
        "\x1b[1mA1 · ORPH-1 · PR #12 \x1b[33mchanges requested\x1b[39m · Statuszeile für Wörter\x1b[0m\n\
         Opus 5.5  ·  effort xhigh  ·  ctx 42%  ·  cache 29 min"
    );
}

#[test]
fn a_full_stdin_gives_the_golden_live_file() {
    // Die App und `auto_compact` lesen diese Datei: Feldnamen, Reihenfolge und Zahlentypen.
    let dir = tempfile::tempdir().unwrap();
    let env = env_in(dir.path(), Some("A1"));
    run_with(&env, &full());
    let raw = std::fs::read_to_string(live_path(&env.state_dir, "A1")).unwrap();
    let live: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        live,
        json!({"model": "Opus 5.5", "effort": "xhigh", "ctx_pct": 41.6, "ctx_tokens": 416_203, "msg_tokens": 815,
            "cost_usd": 12.3456, "session_name": "Statuszeile für Wörter", "cache_until": 1_794_181_800.0, "cache_ttl": 3600,
            "pr": {"number": 12, "url": "https://x/pull/12", "review_state": "changes_requested"},
            "cwd": "C:\\x\\wt\\4116-ui", "ts": NOW})
    );
    assert_eq!(
        live.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["model", "effort", "ctx_pct", "ctx_tokens", "msg_tokens", "cost_usd", "session_name", "cache_until", "cache_ttl", "pr", "cwd", "ts"]
    );
    assert!(live["ctx_tokens"].is_u64() && live["msg_tokens"].is_u64() && live["cache_ttl"].is_u64(), "Tokens bleiben ganz");
    assert!(live["ts"].is_f64() && live["cache_until"].is_f64(), "Zeiten bleiben Float");
    assert_eq!(load_json(&live_path(&env.state_dir, "A1")).unwrap(), live);
}

#[test]
fn small_inputs_without_a_slot_give_the_golden_number_lines() {
    let dir = tempfile::tempdir().unwrap();
    let env = env_in(dir.path(), None);
    let cases = [
        (json!({"model": "sonnet"}), "sonnet"),
        (json!({"model": {"id": "claude-haiku"}, "context_window": {"used_percentage": 7}, "cost": {"total_cost_usd": 0}}), "claude-haiku  ·  ctx 7%"),
        (json!({"context_window": {"used_percentage": 2.5}, "cost": {"total_cost_usd": 0.125}}), "ctx 2%"),
        (
            json!({"model": {"display_name": "Opus"}, "effort": "high", "context_window": {"used_percentage": 99.5}, "prompt_cache": {"warm": false, "ttl": "5m"}}),
            "Opus  ·  effort high  ·  ctx 100%",
        ),
    ];
    for (stdin, want) in cases {
        assert_eq!(run_with(&env, &stdin), want, "{stdin}");
    }
}
