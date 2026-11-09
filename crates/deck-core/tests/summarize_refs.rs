//! Hover-Zusammenfassung, Ticket- und PR-Nummer aus dem Chat (Python
//! `test_claude_summarize.py`). Bei der Erkennung sind Falsch-Positive das Problem.

use deck_core::claude::refs::*;
use deck_core::claude::summarize::*;
use deck_core::claude::summary_cache::*;
use deck_core::paths::save_json;
use serde_json::json;

fn t(role: &str, text: &str) -> Turn {
    (role.to_string(), text.to_string())
}
fn user(text: &str) -> Vec<Turn> {
    vec![t("user", text)]
}

#[test]
fn only_typed_questions_and_answer_text_count_as_turns() {
    let lines = vec![
        json!({"type": "user", "message": {"role": "user", "content": "Bau Feature X"}}).to_string(),
        json!({"type": "assistant", "message": {"role": "assistant", "content": [
            {"type": "thinking", "thinking": "hmm"}, {"type": "text", "text": "Mache ich."},
            {"type": "tool_use", "name": "Bash", "input": {}}]}}).to_string(),
        json!({"type": "user", "message": {"role": "user", "content": [{"type": "tool_result", "content": "total 5"}]}}).to_string(),
        json!({"type": "user", "message": {"role": "user", "content": "<command-name>/x"}}).to_string(),
        json!({"type": "user", "message": {"role": "user", "content": "Caveat: nope"}}).to_string(),
        json!({"type": "system", "subtype": "info"}).to_string(),
        "   ".to_string(),
        "{kaputt".to_string(),
    ];
    assert_eq!(extract_turns(&lines), [t("user", "Bau Feature X"), t("assistant", "Mache ich.")]);
    assert_eq!(extract_turns_from_values(&[json!({"type": "user", "message": {"role": "user", "content": "Hallo"}})]), [t("user", "Hallo")]);
}

#[test]
fn the_digest_keeps_the_topic_and_the_latest_turns() {
    assert_eq!(build_digest(&[], 3500, 600), "");
    let mut turns = vec![t("user", "Taaaaa")];
    turns.extend((1..6).map(|i| t("user", &format!("u{i}"))));
    turns.push(t("assistant", "letzte Antwort"));
    let d = build_digest(&turns, 60, 600);
    let lines: Vec<&str> = d.lines().collect();
    assert!(lines[0].starts_with("User: T"));
    assert!(lines.contains(&"…"));
    assert_eq!(*lines.last().unwrap(), "Assistant: letzte Antwort");
}

#[test]
fn the_digest_has_no_gap_when_everything_fits_and_caps_each_turn() {
    let d = build_digest(&[t("user", "Thema"), t("assistant", "ok"), t("user", "weiter")], 3500, 600);
    assert_eq!(d.lines().collect::<Vec<_>>(), ["User: Thema", "Assistant: ok", "User: weiter"]);
    let d = build_digest(&[t("user", &"x".repeat(900))], 3500, 100);
    assert!(d.starts_with("User: ") && d.ends_with('…') && d.chars().count() <= 120);
}

#[test]
fn the_model_answer_becomes_one_clean_line() {
    assert_eq!(clean_summary("  Bottom-Bar bauen  ", 200), "Bottom-Bar bauen");
    assert_eq!(clean_summary("\"Feature X umsetzen\"", 200), "Feature X umsetzen");
    assert_eq!(clean_summary("„Deutsches Zitat“", 200), "Deutsches Zitat");
    assert_eq!(clean_summary("Zusammenfassung: Y machen", 200), "Y machen");
    assert_eq!(clean_summary("erste Zeile\nzweite", 200), "erste Zeile");
    assert_eq!(clean_summary("a\n\n  b", 200), "a");
    assert_eq!(clean_summary("", 200), "");
    let long = clean_summary(&"z".repeat(300), 50);
    assert!(long.ends_with('…') && long.chars().count() == 51);
}

#[test]
fn the_instruction_follows_the_language_with_german_fallback() {
    assert!(instruction("english").contains("English sentence"));
    assert!(instruction("german").contains("deutschen Satz"));
    assert_eq!(instruction("klingon"), instruction("german"));
}

#[test]
fn the_project_folder_name_replaces_everything_but_ascii_alnum() {
    assert_eq!(enc_cwd(r"C:\Users\Max Müßig\Projekte\agent-deck"), "C--Users-Max-M--ig-Projekte-agent-deck");
    assert_eq!(enc_cwd(""), "");
}

#[test]
fn a_plain_jira_key_is_found_with_or_without_configured_project() {
    assert_eq!(find_ticket(&user("Wir machen PROJ-2691 fertig"), Some("PROJ"), 2), "PROJ-2691");
    assert_eq!(find_ticket(&user("bitte ABC-123 anschauen"), None, 2), "ABC-123");
    assert_eq!(find_ticket(&[], None, 2), "");
    assert_eq!(find_ticket(&user("kein Ticket hier"), None, 2), "");
}

#[test]
fn technical_lookalikes_are_no_tickets() {
    let noise = "UTF-8, SHA-256, ISO-8601, RFC-2119, CVE-2021, AES-256, GPT-5, python-3, top-10";
    assert_eq!(find_ticket(&user(noise), Some("PROJ"), 2), "");
}

#[test]
fn the_configured_project_is_found_lowercase_and_after_ticket_with_a_bare_number() {
    assert_eq!(find_ticket(&user("schau in ticket/proj-2691"), Some("PROJ"), 2), "PROJ-2691");
    assert_eq!(find_ticket(&user("Ticket 2701 bitte"), Some("PROJ"), 2), "PROJ-2701");
    assert_eq!(find_ticket(&user("Issue #42 ist offen"), Some("PROJ"), 2), "PROJ-42");
    assert_eq!(find_ticket(&user("Ticket 2701 bitte"), None, 2), "");
}

#[test]
fn a_single_digit_key_needs_ticket_context() {
    assert_eq!(find_ticket(&user("Ticket PROJ-1 bitte"), None, 2), "PROJ-1");
    assert_eq!(find_ticket(&user("der Wert PROJ-1 steht da"), None, 2), "");
}

#[test]
fn the_first_typed_task_outweighs_later_mentions_and_ties_go_to_the_latest() {
    let turns = [t("user", "ORPH-3728 bitte umsetzen"), t("assistant", "ok"), t("user", "nebenbei ORPH-4062"), t("assistant", "ORPH-4062, verstanden")];
    assert_eq!(find_ticket(&turns, Some("ORPH"), 2), "ORPH-3728");
    assert_eq!(find_ticket(&[t("user", "erst ABC-11"), t("user", "jetzt ABC-22")], None, 2), "ABC-11");
    let later = [t("user", "los geht es"), t("user", "erst ABC-11"), t("user", "jetzt ABC-22")];
    assert_eq!(find_ticket(&later, None, 2), "ABC-22", "gleiches Gewicht: der zuletzt genannte");
    assert_eq!(find_ticket(&user("PROJ-2691 vs FOO-2692"), Some("PROJ"), 2), "PROJ-2691");
}

#[test]
fn what_only_the_agent_says_needs_several_answers() {
    let side = |n: usize| (0..n).map(|i| t("assistant", &format!("Antwort {i}: ABC-99 erledigt"))).collect::<Vec<_>>();
    assert_eq!(find_ticket(&side(1), None, 2), "");
    assert_eq!(find_ticket(&side(3), None, 2), "", "drei Antworten reichen nicht");
    assert_eq!(find_ticket(&side(4), None, 2), "ABC-99");
    let same_turn = vec![t("assistant", "ABC-99, ABC-99, ABC-99 und nochmal ABC-99")];
    assert_eq!(find_ticket(&same_turn, None, 2), "", "je Antwort zählt ein Key einmal");
    assert_eq!(find_ticket(&user("mach ABC-99"), None, 2), "ABC-99");
}

#[test]
fn an_answer_listing_many_tickets_is_an_overview_and_does_not_count() {
    // Gesehen am 09.11.2026: Auftrag ORPH-3728, die Antworten zählten Ticket-Bereiche
    // auf („257 Tickets (ORPH-1212 bis ORPH-3715)“), die Kachel zeigte ORPH-1212.
    let list = "257 Tickets (ORPH-1212 bis ORPH-3715), 100 Tickets (ORPH-716 bis ORPH-1211)";
    let mut turns = vec![t("user", "ORPH-3728 kannst du das umsetzen?")];
    turns.extend((0..6).map(|_| t("assistant", list)));
    assert_eq!(find_ticket(&turns, Some("ORPH"), 2), "ORPH-3728");
    let only_lists: Vec<Turn> = (0..6).map(|_| t("assistant", list)).collect();
    assert_eq!(find_ticket(&only_lists, Some("ORPH"), 2), "");
}

#[test]
fn a_bare_number_counts_only_when_typed_and_a_spoken_project_name_counts_too() {
    assert_eq!(find_ticket(&user("Kannst du mal ORPHEUS 4112 für mich machen?"), Some("ORPH"), 2), "ORPH-4112");
    assert_eq!(find_ticket(&user("orph 4116 bitte"), Some("ORPH"), 2), "ORPH-4116");
    assert_eq!(find_ticket(&user("ORPHEUS 4.1.0 ist raus"), Some("ORPH"), 2), "", "eine Version ist kein Ticket");
    let links: Vec<Turn> = (0..5).map(|_| t("assistant", "siehe Issue #24384 im Tracker")).collect();
    assert_eq!(find_ticket(&links, Some("ORPH"), 2), "", "Issue-Nummern in Antworten sind keine Tickets");
}

#[test]
fn no_key_is_cut_out_of_a_longer_code() {
    assert_eq!(find_ticket(&[t("user", "Dockle meldet CIS-DI-0006"), t("user", "CIS-DI-0006 behoben")], None, 2), "");
}

#[test]
fn turns_without_text_are_skipped() {
    assert_eq!(find_ticket(&[t("user", ""), t("assistant", ""), t("user", "ABC-123")], None, 2), "ABC-123");
}

#[test]
fn a_pr_is_found_by_keyword_or_url() {
    assert_eq!(find_pr(&user("Fix die Bugs aus PR #62"), 3), "62");
    assert_eq!(find_pr(&user("siehe https://github.com/acme/webapp/pull/128"), 3), "128");
    assert_eq!(find_pr(&user("pull request 903 reviewen"), 3), "903");
    assert_eq!(find_pr(&user("merge request 12 anschauen"), 3), "12");
    assert_eq!(find_pr(&user("nichts davon hier"), 3), "");
    assert_eq!(find_pr(&[], 3), "");
}

#[test]
fn a_bare_hash_needs_two_mentions() {
    let once = user("schau dir #62 an");
    assert_eq!(find_pr(&once, 3), "");
    assert_eq!(find_pr(&[once, user("#62 ist noch offen")].concat(), 3), "62");
    assert_eq!(find_pr(&user("schau dir PR #62 an"), 3), "62");
}

#[test]
fn issue_line_and_color_hashes_are_no_prs() {
    assert_eq!(find_pr(&[t("user", "Issue #42 offen"), t("user", "Issue #42 noch offen")], 3), "");
    assert_eq!(find_pr(&[t("user", "in Zeile #42"), t("user", "Zeile #42 nochmal")], 3), "");
    assert_eq!(find_pr(&[t("user", "Farbe #6289ab"), t("user", "wieder #6289ab")], 3), "");
}

#[test]
fn ticket_and_pr_can_both_apply() {
    let got = find_refs(&user("Bugs aus PR #62 zum Ticket PROJ-2651 fixen"), Some("PROJ"));
    assert_eq!(got, Refs { ticket: "PROJ-2651".into(), pr: "62".into(), ..Refs::default() });
    assert_eq!(find_refs(&user("nur reden"), Some("PROJ")), Refs::default());
}

fn write_transcript(projects: &std::path::Path, cwd: &str, sid: &str, turns: &[(&str, &str)]) -> std::path::PathBuf {
    let dir = projects.join(enc_cwd(cwd));
    std::fs::create_dir_all(&dir).unwrap();
    let body: String = turns
        .iter()
        .map(|(r, x)| json!({"type": r, "message": {"role": r, "content": if *r == "user" { json!(x) } else { json!([{"type": "text", "text": x}]) }}}).to_string() + "\n")
        .collect();
    let p = dir.join(format!("{sid}.jsonl"));
    std::fs::write(&p, body).unwrap();
    p
}

#[test]
fn the_transcript_is_found_directly_or_by_session_id_in_any_project() {
    let projects = tempfile::tempdir().unwrap();
    let p = write_transcript(projects.path(), r"C:\repo", "s1", &[("user", "hi")]);
    assert_eq!(transcript_path(projects.path(), "s1", Some(r"C:\repo")), Some(p.clone()));
    assert_eq!(transcript_path(projects.path(), "s1", Some(r"C:\repo.wt\x")), Some(p));
    assert_eq!(transcript_path(projects.path(), "fehlt", None), None);
    assert_eq!(transcript_path(projects.path(), "", None), None);
}

#[test]
fn refs_are_cached_by_size_and_found_values_survive_an_empty_rescan() {
    let projects = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let p = write_transcript(projects.path(), "C:/r", "s1", &[("user", "mach PROJ-7 und PR #5"), ("user", "PR #5 bitte")]);
    let r = ensure_refs(projects.path(), cache.path(), "s1", Some("C:/r"), Some("PROJ"));
    assert_eq!(r, Refs { ticket: "PROJ-7".into(), pr: "5".into(), ..Refs::default() });
    assert_eq!(cached_refs(cache.path(), "s1", Some("PROJ")), r);
    std::fs::write(&p, json!({"type": "user", "message": {"content": "nichts mehr"}}).to_string()).unwrap();
    assert_eq!(ensure_refs(projects.path(), cache.path(), "s1", Some("C:/r"), Some("PROJ")), r);
}

#[test]
fn cached_refs_of_an_older_calculation_or_another_project_do_not_count() {
    // Sonst hielte sich ein früher falsch geratenes Ticket für immer.
    let projects = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    write_transcript(projects.path(), "C:/r", "s1", &[("user", "nur reden")]);
    save_json(&cache.path().join("s1.json"), &json!({"refs_size": 1, "ticket": "ORPH-1212", "pr": "20068"})).unwrap();
    assert_eq!(cached_refs(cache.path(), "s1", Some("ORPH")), Refs::default());
    assert_eq!(ensure_refs(projects.path(), cache.path(), "s1", Some("C:/r"), Some("ORPH")), Refs::default());
    write_transcript(projects.path(), "C:/r", "s2", &[("user", "Ticket 4116 bitte")]);
    assert_eq!(ensure_refs(projects.path(), cache.path(), "s2", Some("C:/r"), Some("PROJ")).ticket, "PROJ-4116");
    assert_eq!(cached_refs(cache.path(), "s2", Some("ORPH")), Refs::default(), "anderes Projekt");
    assert_eq!(ensure_refs(projects.path(), cache.path(), "s2", Some("C:/r"), Some("ORPH")).ticket, "ORPH-4116");
}

#[test]
fn injected_user_lines_are_no_typed_turns_but_pasted_text_blocks_are() {
    let lines = vec![
        json!({"type": "user", "isMeta": true, "message": {"content": "Base directory for this skill: ORPH-1"}}).to_string(),
        json!({"type": "user", "isCompactSummary": true, "message": {"content": "Summary: ORPH-2, ORPH-3"}}).to_string(),
        json!({"type": "user", "isSidechain": true, "message": {"content": "Subagent: ORPH-4"}}).to_string(),
        json!({"type": "user", "toolUseResult": {"stdout": "x"}, "message": {"content": "ORPH-5"}}).to_string(),
        json!({"type": "user", "message": {"content": [{"type": "tool_result", "content": "ORPH-6"}, {"type": "text", "text": "ORPH-7"}]}}).to_string(),
        json!({"type": "user", "message": {"content": [{"type": "text", "text": "ORPH-4116 bitte"}, {"type": "image", "source": {}}]}}).to_string(),
    ];
    assert_eq!(extract_turns(&lines), [t("user", "ORPH-4116 bitte")]);
}

#[test]
fn a_summary_is_regenerated_only_on_growth_after_cooldown_or_language_change() {
    let projects = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    write_transcript(projects.path(), "C:/r", "s1", &[("user", "Baue das Deck neu"), ("assistant", "ok")]);
    let gates = SummaryGates::default();
    let plan = plan_summary(projects.path(), cache.path(), "s1", Some("C:/r"), "german", 1000.0, gates);
    let SummaryPlan::Generate { prompt, size, cached } = plan else { panic!("ohne Cache muss erzeugt werden") };
    assert!(prompt.starts_with(instruction("german")) && prompt.ends_with("User: Baue das Deck neu\nAssistant: ok"));
    assert_eq!(cached, None);
    assert_eq!(finish_summary(cache.path(), "s1", Some("\"Deck neu bauen\"\n"), size, "german", 1000.0, None).as_deref(), Some("Deck neu bauen"));
    let again = plan_summary(projects.path(), cache.path(), "s1", Some("C:/r"), "german", 5000.0, gates);
    assert_eq!(again, SummaryPlan::Cached(Some("Deck neu bauen".into())));
    let english = plan_summary(projects.path(), cache.path(), "s1", Some("C:/r"), "english", 1001.0, gates);
    assert!(matches!(english, SummaryPlan::Generate { .. }));
    assert_eq!(finish_summary(cache.path(), "s1", None, size, "english", 1001.0, Some("alt".into())).as_deref(), Some("alt"));
    assert_eq!(cached_summary(cache.path(), "s1").as_deref(), Some("Deck neu bauen"));
}
