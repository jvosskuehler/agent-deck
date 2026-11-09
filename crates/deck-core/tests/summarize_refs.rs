//! Hover-Zusammenfassung, Ticket- und PR-Nummer aus dem Chat (Python
//! `test_claude_summarize.py`). Bei der Erkennung sind Falsch-Positive das Problem.

use deck_core::claude::refs::*;
use deck_core::claude::summarize::*;
use deck_core::claude::summary_cache::*;
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
fn frequency_beats_order_and_ties_go_to_the_latest() {
    let turns = [t("user", "nebenbei ABC-12"), t("assistant", "ok"), t("user", "eigentlich geht es um XYZ-77"), t("assistant", "XYZ-77, verstanden")];
    assert_eq!(find_ticket(&turns, None, 2), "XYZ-77");
    assert_eq!(find_ticket(&[t("user", "erst ABC-11"), t("user", "jetzt ABC-22")], None, 2), "ABC-22");
    assert_eq!(find_ticket(&user("PROJ-2691 vs FOO-2692"), Some("PROJ"), 2), "PROJ-2691");
}

#[test]
fn a_single_side_remark_of_the_agent_is_not_enough() {
    let side = vec![t("assistant", "das behebt uebrigens ABC-99")];
    assert_eq!(find_ticket(&side, None, 2), "");
    let twice = [side.clone(), vec![t("assistant", "ABC-99 ist damit durch")]].concat();
    assert_eq!(find_ticket(&twice, None, 2), "ABC-99");
    assert_eq!(find_ticket(&user("mach ABC-99"), None, 2), "ABC-99");
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
    assert_eq!(got, Refs { ticket: "PROJ-2651".into(), pr: "62".into() });
    assert_eq!(find_refs(&user("nur reden"), Some("PROJ")), Refs::default());
    assert_eq!(refs_label(&got), "Ticket: PROJ-2651 · PR #62");
    assert_eq!(refs_card_label(&got, 14), "PROJ-2651 #62");
    assert_eq!(refs_card_label(&Refs { ticket: "PROJECT-12345".into(), pr: "62".into() }, 14), "PROJECT-12345");
    assert_eq!(refs_card_label(&Refs { ticket: String::new(), pr: "62".into() }, 14), "#62");
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
    assert_eq!(r, Refs { ticket: "PROJ-7".into(), pr: "5".into() });
    assert_eq!(cached_refs(cache.path(), "s1"), r);
    std::fs::write(&p, json!({"type": "user", "message": {"content": "nichts mehr"}}).to_string()).unwrap();
    assert_eq!(ensure_refs(projects.path(), cache.path(), "s1", Some("C:/r"), Some("PROJ")), r);
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
