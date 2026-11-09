//! Ticket-Prompts: EINZEILIG und alle Platzhalter gefüllt (Python `test_ui_ticket.py`).
//! Ein mehrzeiliger Prompt wird als mehrere Eingaben abgeschickt.

use std::path::Path;

use deck_core::prompts::*;

fn assert_clean(p: &str, must: &[&str]) {
    assert!(!p.contains('\n') && !p.contains('\r'), "Prompt ist mehrzeilig");
    assert!(!p.contains('{') && !p.contains('}'), "unaufgelöster Platzhalter: {p}");
    for s in must {
        assert!(p.contains(s), "fehlt im Prompt: {s}");
    }
}

#[test]
fn the_ticket_prompt_is_one_line_and_filled() {
    let state = Path::new(r"C:\Users\x\AppData\Local\claude-agent-deck\state");
    let p = ticket_prompt(state, "A1", "ABC-123", "mach\nwas", "PROJ", None);
    assert_clean(&p, &["ABC-123", "ticket/abc-123", "C:/Users/x/AppData/Local/claude-agent-deck/state/A1.worktree", "mach was", "Jira"]);
}

#[test]
fn a_bare_number_becomes_a_key_of_the_configured_project() {
    let p = ticket_prompt(Path::new("C:/s"), "A1", "2701", "", "ORPH", None);
    assert_clean(&p, &["ORPH-2701", "ticket/2701", TICKET_TASK_FALLBACK]);
}

#[test]
fn the_home_block_gets_the_all_repos_prompt_without_marker_file() {
    let p = hub_prompt("ORPH-4116", r"C:\Users\x\Repos", "mach was");
    assert_clean(&p, &["ORPH-4116", "C:/Users/x/Repos/wt/4116-", "mach was", "Jira"]);
    assert!(!p.contains(".worktree") && !p.contains("git rev-parse"));
    let via_ticket = ticket_prompt(Path::new("C:/s"), "B1", "ORPH-4116", "mach was", "ORPH", Some(r"C:\Users\x\Repos"));
    assert_eq!(via_ticket, p);
}

#[test]
fn filling_does_not_replace_placeholders_inside_inserted_values() {
    assert_eq!(fill("{a} {b} {c}", &[("a", "{b}"), ("b", "x")]), "{b} x {c}");
}
