use super::*;

fn rules() -> TitleRules {
    TitleRules { first_at: 3, every: 3, confirm: 2 }
}

fn state(title: &str, cand: &str, hits: u32, checked_at: usize) -> TitleState {
    TitleState { title: title.into(), cand: cand.into(), hits, checked_at }
}

#[test]
fn before_the_third_prompt_claude_codes_title_stays() {
    let s = decide(&TitleState::default(), Some("Mails: landen im Spam"), 2, rules());
    assert_eq!(s, TitleState::default());
}

#[test]
fn the_first_own_title_applies_at_once() {
    let s = decide(&TitleState::default(), Some("Mails: landen im Spam"), 3, rules());
    assert_eq!(s, state("Mails: landen im Spam", "", 0, 3));
}

#[test]
fn between_checks_proposals_are_ignored() {
    let s = state("Mails: landen im Spam", "", 0, 3);
    assert_eq!(decide(&s, Some("Matrixbot: Sprachnachrichten"), 5, rules()), s);
}

#[test]
fn a_side_question_seen_once_does_not_change_the_title() {
    let s0 = state("Kundenservice: Prompt für LLM", "", 0, 3);
    let s1 = decide(&s0, Some("Monorepo: Management API archivieren"), 6, rules());
    assert_eq!(s1.title, "Kundenservice: Prompt für LLM");
    assert_eq!(s1.cand, "Monorepo: Management API archivieren");
    // Bei der nächsten Prüfung ist das Thema wieder das alte: der Vormerker fällt weg.
    let s2 = decide(&s1, Some("Kundenservice: Prompt für LLM"), 9, rules());
    assert_eq!(s2, state("Kundenservice: Prompt für LLM", "", 0, 9));
}

#[test]
fn a_new_topic_wins_after_two_checks_in_a_row() {
    let s0 = state("Kundenservice: Verlauf", "", 0, 3);
    let s1 = decide(&s0, Some("Kundenservice: Prompt für LLM"), 6, rules());
    let s2 = decide(&s1, Some("kundenservice:  Prompt für LLM"), 9, rules());
    assert_eq!(s2.title, "kundenservice:  Prompt für LLM");
    assert_eq!((s2.cand.as_str(), s2.hits), ("", 0));
}

#[test]
fn two_different_new_proposals_do_not_add_up() {
    let s0 = state("Kundenservice: Verlauf", "", 0, 3);
    let s1 = decide(&s0, Some("Mails: Spam"), 6, rules());
    let s2 = decide(&s1, Some("Matrixbot: Sprachnachrichten"), 9, rules());
    assert_eq!(s2.title, "Kundenservice: Verlauf");
    assert_eq!((s2.cand.as_str(), s2.hits), ("Matrixbot: Sprachnachrichten", 1));
}

#[test]
fn a_missing_proposal_is_no_check() {
    let s = state("Mails: Spam", "", 0, 3);
    assert_eq!(decide(&s, None, 6, rules()), s);
}

#[test]
fn clean_keeps_area_and_topic() {
    assert_eq!(clean("Kundenservice: Prompt für LLM").as_deref(), Some("Kundenservice: Prompt für LLM"));
    assert_eq!(clean("  **„Mails: landen im Spam.“** ").as_deref(), Some("Mails: landen im Spam"));
}

#[test]
fn clean_drops_ticket_and_pr_numbers() {
    assert_eq!(clean("Matrixbot: DEMO-4267 Sprachnachrichten").as_deref(), Some("Matrixbot: Sprachnachrichten"));
    assert_eq!(clean("Kundenservice: Prompt für LLM (PR #359)").as_deref(), Some("Kundenservice: Prompt für LLM"));
    assert_eq!(clean("Kundenservice: Tiefe für $42664").as_deref(), Some("Kundenservice: Tiefe für"));
}

#[test]
fn clean_replaces_dashes() {
    assert_eq!(clean("Mails: Spam – DMARC").as_deref(), Some("Mails: Spam, DMARC"));
}

#[test]
fn clean_rejects_titles_without_area() {
    assert_eq!(clean("Prompt für LLM"), None);
    assert_eq!(clean(": Prompt"), None);
    assert_eq!(clean("Kundenservice:"), None);
}

#[test]
fn clean_caps_the_length() {
    let t = clean(&format!("Kundenservice: {}", "sehr lang ".repeat(10))).unwrap();
    assert_eq!(char_len(&t), TITLE_MAX);
    assert!(t.ends_with('…'));
}

#[test]
fn areas_are_listed_newest_first_and_old_ones_drop_out() {
    let dir = tempfile::tempdir().unwrap();
    let day = 86_400.0;
    remember_area(dir.path(), "Mails: Spam", 1000.0 * day);
    remember_area(dir.path(), "Kundenservice: Verlauf", 1010.0 * day);
    remember_area(dir.path(), "Matrixbot: Sprachnachrichten", 1020.0 * day);
    assert_eq!(recent_areas(dir.path(), 1035.0 * day), vec!["Matrixbot", "Kundenservice"]);
}

#[test]
fn an_area_in_other_case_replaces_the_old_spelling() {
    let dir = tempfile::tempdir().unwrap();
    remember_area(dir.path(), "kundenservice: Verlauf", 10.0);
    remember_area(dir.path(), "Kundenservice: Prompt", 20.0);
    assert_eq!(recent_areas(dir.path(), 30.0), vec!["Kundenservice"]);
}

#[test]
fn the_instruction_offers_title_pending_title_and_areas() {
    let s = state("Kundenservice: Verlauf", "Mails: Spam", 1, 6);
    let text = instruction("german", &s, &["Kundenservice".into(), "Mails".into()]);
    assert!(text.contains("Titel: Bereich: Thema"));
    assert!(text.contains("Schon benutzte Bereiche: Kundenservice, Mails."));
    assert!(text.contains("Aktueller Titel: Kundenservice: Verlauf."));
    assert!(text.contains("Vorgemerkter neuer Titel: Mails: Spam."));
}

#[test]
fn the_title_line_is_read_and_stays_out_of_the_briefing() {
    let raw = "Ziel: Spam beheben\nStand: Test 7,8\nWartet auf dich: nichts\nNächster Schritt: DMARC setzen\nTitel: Mails: landen im Spam";
    let b = crate::claude::briefing::parse(raw).unwrap();
    assert_eq!(b.next, "DMARC setzen");
    assert_eq!(crate::claude::briefing::parse_title(raw).as_deref(), Some("Mails: landen im Spam"));
    assert_eq!(crate::claude::briefing::parse_title("Ziel: x\nStand: y"), None);
}
