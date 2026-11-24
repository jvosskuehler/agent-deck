//! Ticket und PR einer Kachel aus Branch, Ordner, Statuszeile, Titel und Chat, die
//! stärkste Quelle gewinnt.

use deck_core::claude::refs::Refs;
use deck_core::i18n::Lang;
use deck_core::ref_sources::*;
use deck_core::view::RefSource;

const DE: Lang = Lang::German;

#[test]
fn a_branch_names_the_ticket_by_key_or_by_leading_number() {
    assert_eq!(ticket_from_branch("feature/DEMO-4116-taskbar", "DEMO").as_deref(), Some("DEMO-4116"));
    assert_eq!(ticket_from_branch("DEMO-4116", "demo").as_deref(), Some("DEMO-4116"));
    assert_eq!(ticket_from_branch("ticket/demo-4116", "DEMO").as_deref(), Some("DEMO-4116"), "so baut das Deck selbst Branches");
    assert_eq!(ticket_from_branch("4116-taskbar", "DEMO").as_deref(), Some("DEMO-4116"));
    assert_eq!(ticket_from_branch("feature/4116_ui", "DEMO").as_deref(), Some("DEMO-4116"));
    assert_eq!(ticket_from_branch("fix/CS-231-mikro", "DEMO").as_deref(), Some("CS-231"), "fremdes Projekt, groß geschrieben");
    assert_eq!(ticket_from_branch("ticket/cs-231", "DEMO").as_deref(), Some("CS-231"));
}

#[test]
fn ordinary_branches_name_no_ticket() {
    for b in ["main", "master", "develop", "HEAD", "release/2026-11-09", "release/1.2.3", "hotfix/login-2", "renovate/node-18",
        "fix/utf-8-bom", "dependabot/npm_and_yarn/vite-5.4.2", "feature/4116-2fa"] {
        assert_eq!(ticket_from_branch(b, "DEMO"), None, "{b}");
    }
    assert_eq!(ticket_from_branch("4116-ui", ""), None, "ohne Projekt bleibt eine Nummer keine");
}

/// Ohne Projektkürzel (die Team-Ausgabe kennt keins) zählt nur ein ausgeschriebener Key
/// `ABC-123` im Branch. Finden ist leicht, schwer ist, nichts zu erfinden: Versionen,
/// Normen und Kleingeschriebenes bleiben ohne Ticket.
#[test]
fn without_a_project_a_branch_names_only_an_upper_case_key() {
    for (b, want) in [
        ("feature/ABC-123-login", "ABC-123"),
        ("ABC-123", "ABC-123"),
        ("fix/CS-231-mikro", "CS-231"),
        ("bugfix/DATA2-77_csv", "DATA2-77"),
        ("ticket/abc-42", "ABC-42"),
    ] {
        assert_eq!(ticket_from_branch(b, "").as_deref(), Some(want), "{b}");
    }
}

#[test]
fn without_a_project_look_alikes_in_a_branch_name_no_ticket() {
    for b in [
        "main", "develop", "release/2026-11-09", "release/1.2.3", "release/v1-2", "4116-ui", "feature/4116_ui",
        "fix/UTF-8-bom", "fix/utf-8-bom", "chore/SHA-256-hashes", "build/ES-2020-target", "deps/TLS-13", "feature/ISO-8601",
        "feature/HTTP-2", "bump/NODE-18", "renovate/node-18", "feature/WIN-11-installer", "feature/RC-12", "feature/AB-1",
        "feature/Abc-123", "hotfix/login-2", "dependabot/npm_and_yarn/vite-5.4.2", "feature/x86-64", "feature/MP3-320",
    ] {
        assert_eq!(ticket_from_branch(b, ""), None, "{b}");
    }
}

#[test]
fn the_worktree_folder_names_the_ticket() {
    assert_eq!(ticket_from_folder(r"C:\Users\x\Repos\wt\4116-ui", "DEMO").as_deref(), Some("DEMO-4116"));
    assert_eq!(ticket_from_folder("C:/Users/x/Repos/wt/4062-api/src/Deep", "DEMO").as_deref(), Some("DEMO-4062"));
    assert_eq!(ticket_from_folder(r"C:\Repos\wt-4116-client", "DEMO").as_deref(), Some("DEMO-4116"));
    assert_eq!(ticket_from_folder(r"C:\Repos\agent-deck\.claude\worktrees\demo-3773", "DEMO").as_deref(), Some("DEMO-3773"));
    assert_eq!(ticket_from_folder(r"C:\Repos\wt\CS-231-mail", "DEMO").as_deref(), Some("CS-231"));
    for d in [r"C:\Repos\demoprojekt-management-api", r"C:\Repos\wt-deck-tauri", r"C:\Repos\agent-deck\.claude\worktrees\agent-af2abf1b", r"C:\Repos\wt", r"C:\v4116\x"] {
        assert_eq!(ticket_from_folder(d, "DEMO"), None, "{d}");
    }
}

#[test]
fn the_title_counts_like_a_typed_message_and_its_pr_needs_the_word_pr() {
    assert_eq!(ticket_from_title("DEMO-4062 Gratismonat: Testuhr", "DEMO").as_deref(), Some("DEMO-4062"));
    assert_eq!(ticket_from_title("Ticket 3773 Rechtekonzept", "DEMO").as_deref(), Some("DEMO-3773"));
    assert_eq!(ticket_from_title("UTF-8 in der Statuszeile", "DEMO"), None);
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
    let chat = Refs { ticket: "DEMO-1212".into(), pr: "20068".into(), ..Refs::default() };
    let branches = vec!["feature/DEMO-4062-x".to_string()];
    let dirs = vec!["C:/Repos/wt/4116-ui".to_string()];
    let all = RefEvidence {
        assigned: Some("3773"),
        branches: &branches,
        dirs: &dirs,
        statusline_pr: Some(347),
        title: Some("DEMO-3192 PR 180"),
        chat: Some(&chat),
        worktree_branch: None,
        prefix: "DEMO",
    };
    let s = |t: &str, src: RefSource, d: bool| Some((t.to_string(), src, d));
    assert_eq!(pick(&all), (s("DEMO-3773", RefSource::Assigned, false), s("#347", RefSource::Statusline, false)));
    let ev = RefEvidence { assigned: None, statusline_pr: None, ..all };
    assert_eq!(pick(&ev), (s("DEMO-4062", RefSource::Branch, false), s("#180", RefSource::Title, true)));
    let ev = RefEvidence { branches: &[], ..ev };
    assert_eq!(pick(&ev).0, s("DEMO-4116", RefSource::Folder, false));
    let ev = RefEvidence { dirs: &[], ..ev };
    assert_eq!(pick(&ev).0, s("DEMO-3192", RefSource::Title, true));
    let ev = RefEvidence { title: None, ..ev };
    assert_eq!(pick(&ev), (s("DEMO-1212", RefSource::Chat, true), None), "aus dem Chat nie ein PR");
    assert_eq!(pick(&RefEvidence { prefix: "DEMO", ..RefEvidence::default() }), (None, None));
}

#[test]
fn a_main_branch_falls_through_to_the_folder_and_long_tickets_are_cut() {
    let branches = vec!["main".to_string()];
    let dirs = vec!["C:/Repos/wt/4116-ui".to_string()];
    let ev = RefEvidence { branches: &branches, dirs: &dirs, prefix: "DEMO", ..RefEvidence::default() };
    assert_eq!(pick_ticket(&ev, DE).unwrap().source, RefSource::Folder);
    let long = RefEvidence { assigned: Some("SEHR-LANGES-TICKET-1"), prefix: "DEMO", ..RefEvidence::default() };
    let t = pick_ticket(&long, Lang::English).unwrap();
    assert_eq!((t.text.as_str(), t.origin.as_str()), ("SEHR-LANGES-T…", "assigned"));
    assert_eq!(t.text.chars().count(), TICKET_MAX_CHARS);
}
