//! Ticket und PR einer Kachel aus Branch, Ordner, Statuszeile, Titel und Chat, die
//! stärkste Quelle gewinnt.

use deck_core::claude::refs::Refs;
use deck_core::i18n::Lang;
use deck_core::ref_sources::*;
use deck_core::view::RefSource;

const DE: Lang = Lang::German;

#[test]
fn a_branch_names_the_ticket_by_key_or_by_leading_number() {
    assert_eq!(ticket_from_branch("feature/ORPH-4116-taskbar", "ORPH").as_deref(), Some("ORPH-4116"));
    assert_eq!(ticket_from_branch("ORPH-4116", "orph").as_deref(), Some("ORPH-4116"));
    assert_eq!(ticket_from_branch("ticket/orph-4116", "ORPH").as_deref(), Some("ORPH-4116"), "so baut das Deck selbst Branches");
    assert_eq!(ticket_from_branch("4116-taskbar", "ORPH").as_deref(), Some("ORPH-4116"));
    assert_eq!(ticket_from_branch("feature/4116_ui", "ORPH").as_deref(), Some("ORPH-4116"));
    assert_eq!(ticket_from_branch("fix/CS-231-mikro", "ORPH").as_deref(), Some("CS-231"), "fremdes Projekt, groß geschrieben");
    assert_eq!(ticket_from_branch("ticket/cs-231", "ORPH").as_deref(), Some("CS-231"));
}

#[test]
fn ordinary_branches_name_no_ticket() {
    for b in ["main", "master", "develop", "HEAD", "release/2026-11-09", "release/1.2.3", "hotfix/login-2", "renovate/node-18",
        "fix/utf-8-bom", "dependabot/npm_and_yarn/vite-5.4.2", "feature/4116-2fa"] {
        assert_eq!(ticket_from_branch(b, "ORPH"), None, "{b}");
    }
    assert_eq!(ticket_from_branch("4116-ui", ""), None, "ohne Projekt bleibt eine Nummer keine");
}

#[test]
fn the_worktree_folder_names_the_ticket() {
    assert_eq!(ticket_from_folder(r"C:\Users\x\Repos\wt\4116-ui", "ORPH").as_deref(), Some("ORPH-4116"));
    assert_eq!(ticket_from_folder("C:/Users/x/Repos/wt/4062-api/src/Deep", "ORPH").as_deref(), Some("ORPH-4062"));
    assert_eq!(ticket_from_folder(r"C:\Repos\wt-4116-client", "ORPH").as_deref(), Some("ORPH-4116"));
    assert_eq!(ticket_from_folder(r"C:\Repos\agent-deck\.claude\worktrees\orph-3773", "ORPH").as_deref(), Some("ORPH-3773"));
    assert_eq!(ticket_from_folder(r"C:\Repos\wt\CS-231-mail", "ORPH").as_deref(), Some("CS-231"));
    for d in [r"C:\Repos\orpheus-management-api", r"C:\Repos\wt-deck-tauri", r"C:\Repos\agent-deck\.claude\worktrees\agent-af2abf1b", r"C:\Repos\wt", r"C:\v4116\x"] {
        assert_eq!(ticket_from_folder(d, "ORPH"), None, "{d}");
    }
}

#[test]
fn the_title_counts_like_a_typed_message_and_its_pr_needs_the_word_pr() {
    assert_eq!(ticket_from_title("ORPH-4062 Gratismonat: Testuhr", "ORPH").as_deref(), Some("ORPH-4062"));
    assert_eq!(ticket_from_title("Ticket 3773 Rechtekonzept", "ORPH").as_deref(), Some("ORPH-3773"));
    assert_eq!(ticket_from_title("UTF-8 in der Statuszeile", "ORPH"), None);
    assert_eq!(pr_from_title("PR 180 Review-Threads schließen").as_deref(), Some("180"));
    assert_eq!(pr_from_title("Fix #12 im Login"), None, "eine bloße Raute kann ein Issue sein");
}

#[test]
fn the_folders_of_an_agent_come_worktree_first_without_doubles() {
    assert_eq!(ref_dirs(Some("C:/wt/1-a"), Some("C:/r"), Some("C:/r")), ["C:/wt/1-a", "C:/r"]);
    assert_eq!(ref_dirs(None, Some(" "), None), Vec::<String>::new());
}

type Seen = Option<(String, RefSource, bool)>;

fn pick(ev: &RefEvidence) -> (Seen, Seen) {
    let f = |r: Option<deck_core::view::RefView>| r.map(|r| (r.text, r.source, r.detected));
    (f(pick_ticket(ev, DE)), f(pick_pr(ev, DE)))
}

#[test]
fn the_strongest_source_wins_for_ticket_and_pr() {
    let chat = Refs { ticket: "ORPH-1212".into(), pr: "20068".into(), ..Refs::default() };
    let branches = vec!["feature/ORPH-4062-x".to_string()];
    let dirs = vec!["C:/Repos/wt/4116-ui".to_string()];
    let all = RefEvidence {
        assigned: Some("3773"),
        branches: &branches,
        dirs: &dirs,
        statusline_pr: Some(347),
        title: Some("ORPH-3192 PR 180"),
        chat: Some(&chat),
        worktree_branch: None,
        prefix: "ORPH",
    };
    let s = |t: &str, src: RefSource, d: bool| Some((t.to_string(), src, d));
    assert_eq!(pick(&all), (s("ORPH-3773", RefSource::Assigned, false), s("#347", RefSource::Statusline, false)));
    let ev = RefEvidence { assigned: None, statusline_pr: None, ..all };
    assert_eq!(pick(&ev), (s("ORPH-4062", RefSource::Branch, false), s("#180", RefSource::Title, true)));
    let ev = RefEvidence { branches: &[], ..ev };
    assert_eq!(pick(&ev).0, s("ORPH-4116", RefSource::Folder, false));
    let ev = RefEvidence { dirs: &[], ..ev };
    assert_eq!(pick(&ev).0, s("ORPH-3192", RefSource::Title, true));
    let ev = RefEvidence { title: None, ..ev };
    assert_eq!(pick(&ev), (s("ORPH-1212", RefSource::Chat, true), s("#20068", RefSource::Chat, true)));
    assert_eq!(pick(&RefEvidence { prefix: "ORPH", ..RefEvidence::default() }), (None, None));
}

#[test]
fn a_main_branch_falls_through_to_the_folder_and_long_tickets_are_cut() {
    let branches = vec!["main".to_string()];
    let dirs = vec!["C:/Repos/wt/4116-ui".to_string()];
    let ev = RefEvidence { branches: &branches, dirs: &dirs, prefix: "ORPH", ..RefEvidence::default() };
    assert_eq!(pick_ticket(&ev, DE).unwrap().source, RefSource::Folder);
    let long = RefEvidence { assigned: Some("SEHR-LANGES-TICKET-1"), prefix: "ORPH", ..RefEvidence::default() };
    let t = pick_ticket(&long, Lang::English).unwrap();
    assert_eq!((t.text.as_str(), t.origin.as_str()), ("SEHR-LANGES-T…", "assigned"));
    assert_eq!(t.text.chars().count(), TICKET_MAX_CHARS);
}
