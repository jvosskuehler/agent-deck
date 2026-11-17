use super::*;

/// Eine Antwortzeile; `tool` = Eingabe eines Werkzeugaufrufs (JSON-Text) oder leer.
pub(crate) fn reply(id: &str, ts: &str, model: &str, tool: &str) -> String {
    let content = if tool.is_empty() {
        r#"[{"type":"text","text":"x"}]"#.to_string()
    } else {
        format!(r#"[{{"type":"tool_use","name":"Bash","input":{tool}}}]"#)
    };
    format!(
        r#"{{"type":"assistant","timestamp":"{ts}","cwd":"C:\\Users\\J\\Repos","gitBranch":"HEAD","message":{{"id":"{id}","model":"{model}","content":{content},"usage":{{"input_tokens":1000,"output_tokens":1000,"cache_creation_input_tokens":3000,"cache_read_input_tokens":1000000,"cache_creation":{{"ephemeral_1h_input_tokens":1000,"ephemeral_5m_input_tokens":2000}},"speed":"standard"}}}}}}"#
    )
}

#[test]
fn opus_costs_follow_the_published_prices_in_opus_input_tokens() {
    let f = read_line(&reply("m", "2026-11-16T08:00:00Z", "claude-opus-5-5", ""), "ORPH").unwrap();
    let mix = f.reply.unwrap().mix;
    // Input 1×, Output 5×, Cache schreiben 5 Min. 1,25× und 1 Std. 2×, Cache lesen 0,05×.
    assert_eq!(mix.input, 1000.0);
    assert_eq!(mix.output, 5000.0);
    assert_eq!(mix.write, 2000.0 * 1.25 + 1000.0 * 2.0);
    assert!((mix.read - 50_000.0).abs() < 1e-6, "Cache lesen kostet bei Opus 5.5 nur 0,05×: {}", mix.read);
}

#[test]
fn fable_counts_twice_opus_and_reads_its_cache_at_a_fortieth() {
    let f = read_line(&reply("m", "2026-11-16T08:00:00Z", "claude-fable-5-2", ""), "ORPH").unwrap();
    let mix = f.reply.unwrap().mix;
    assert_eq!(mix.input, 2000.0);
    assert_eq!(mix.output, 10_000.0);
    assert!((mix.read - 50_000.0).abs() < 1e-6, "0,20 $ je Mio. wie Opus: {}", mix.read);
}

#[test]
fn haiku_with_a_long_prompt_costs_five_times_more() {
    let short = price_for("claude-haiku-5-5", 90_000, None).unwrap();
    let long = price_for("claude-haiku-5-5", 120_000, Some("standard")).unwrap();
    assert_eq!(long.input, short.input * 5.0);
}

#[test]
fn unknown_models_and_fast_mode_are_not_priced_but_reported() {
    assert_eq!(price_for("claude-opus-5-5", 10, Some("fast")), None);
    let f = read_line(&reply("m", "2026-11-16T08:00:00Z", "claude-neu-9", ""), "ORPH").unwrap();
    assert_eq!(f.reply, None);
    assert_eq!(f.unpriced.as_deref(), Some("claude-neu-9"));
}

#[test]
fn worktree_folders_name_their_ticket() {
    assert_eq!(ticket_of_worktree("4258-api", "ORPH").as_deref(), Some("ORPH-4258"));
    assert_eq!(ticket_of_worktree("4113", "ORPH").as_deref(), Some("ORPH-4113"));
    assert_eq!(ticket_of_worktree("deck-dashboard", "ORPH").as_deref(), Some("agent-deck"));
    assert_eq!(ticket_of_worktree("cs-231-mail", "ORPH").as_deref(), Some("CS-231"));
    assert_eq!(ticket_of_worktree("mono", "ORPH"), None);
    assert_eq!(ticket_of_worktree("manual49-client", "ORPH"), None);
}

#[test]
fn tool_calls_name_the_ticket_and_an_overview_call_does_not() {
    let one = reply("m", "2026-11-16T08:00:00Z", "claude-opus-5-5", r#"{"command":"cd C:\\Repos\\wt\\4258-api && dotnet test"}"#);
    assert_eq!(read_line(&one, "ORPH").unwrap().hits, ["ORPH-4258"]);
    let deck = reply("m", "2026-11-16T08:00:00Z", "claude-opus-5-5", r#"{"file_path":"C:/Users/J/Repos/agent-deck/src/x.rs"}"#);
    assert_eq!(read_line(&deck, "ORPH").unwrap().hits, ["agent-deck"]);
    let overview =
        reply("m", "2026-11-16T08:00:00Z", "claude-opus-5-5", r#"{"command":"ls wt/4258-api wt/4259-api wt/4260-api wt/4261-ui"}"#);
    assert!(read_line(&overview, "ORPH").unwrap().hits.is_empty());
}

#[test]
fn paths_in_tool_results_and_prose_do_not_count() {
    let user = r#"{"type":"user","timestamp":"2026-11-16T08:00:00Z","message":{"content":[{"type":"tool_result","content":"wt/4258-api\nwt/4259-api"}]}}"#;
    assert!(read_line(user, "ORPH").unwrap().hits.is_empty());
    let prose = reply("m", "2026-11-16T08:00:00Z", "claude-opus-5-5", "").replace(r#""text":"x""#, r#""text":"siehe wt/4258-api""#);
    assert!(read_line(&prose, "ORPH").unwrap().hits.is_empty());
}

#[test]
fn a_branch_with_the_jira_prefix_names_the_ticket() {
    let l = reply("m", "2026-11-16T08:00:00Z", "claude-opus-5-5", "").replace(r#""gitBranch":"HEAD""#, r#""gitBranch":"fix/ORPH-4189-sync""#);
    assert_eq!(read_line(&l, "ORPH").unwrap().hits, ["ORPH-4189"]);
}

#[test]
fn lines_without_reply_or_path_are_skipped_cheaply() {
    assert_eq!(read_line(r#"{"type":"user","timestamp":"2026-11-16T08:00:00Z","message":{"content":"hallo"}}"#, "ORPH"), None);
    assert_eq!(read_line("kaputt", "ORPH"), None);
}
