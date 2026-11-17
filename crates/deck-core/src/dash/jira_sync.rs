//! Jira-Status nach dem Merge nachziehen (Konvention: offener PR = „waiting for review“,
//! alle PRs gemergt = „Done“).
//!
//! Nach jedem Merge eines eigenen PRs (durch das Deck oder durch jemand anderen) prüft der
//! Dienst das Ticket aus dem PR-Titel: alle PRs mit diesem Ticket im Titel, in allen Repos
//! der Organisation (Einstellung `github_org`). Sind alle gemergt oder geschlossen und
//! mindestens einer gemergt: Done. Ist noch einer offen: waiting for review, auch aus
//! Done heraus. Sonst nie zurück aus Done. Abgefragt wird per GraphQL-Suche
//! ([`search_query`]), gesetzt per `acli` (`deck-services`, Trait `JiraApi`).

use std::collections::BTreeSet;

use serde_json::Value;

use super::feed_state::FeedState;
use super::prs::{pr_key, PrSnapshot};

/// Zielstatus „alles gemergt“.
pub const DONE: &str = "Done";
/// Zielstatus „noch ein PR offen“.
pub const REVIEW: &str = "waiting for review";
/// Nur Merges der letzten Stunden lösen etwas aus (ältere sind längst nachgezogen).
pub const WINDOW_S: f64 = 24.0 * 3600.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrState {
    Open,
    Merged,
    Closed,
}

/// Ein PR zum Ticket, aus der Suche.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketPr {
    pub title: String,
    pub state: PrState,
}

/// Ticket-Schlüssel im Titel (`<PREFIX>-123`, Präfix aus `jira_prefix`); leer = keins.
pub fn ticket_in(title: &str, prefix: &str) -> String {
    let pat = format!("{prefix}-");
    let mut from = 0;
    while let Some(i) = title[from..].find(&pat) {
        let start = from + i;
        let before_ok = start == 0 || !title[..start].chars().next_back().is_some_and(char::is_alphanumeric);
        let digits: String = title[start + pat.len()..].chars().take_while(char::is_ascii_digit).collect();
        if before_ok && !digits.is_empty() {
            return format!("{pat}{digits}");
        }
        from = start + pat.len();
    }
    String::new()
}

/// Gemergte eigene PRs der letzten Stunden, deren Ticket noch nicht geprüft wurde:
/// (PR-Schlüssel, Ticket).
pub fn due(snap: &PrSnapshot, st: &FeedState, prefix: &str, now: f64) -> Vec<(String, String)> {
    let mut seen = BTreeSet::new();
    snap.merged
        .iter()
        .filter(|m| m.merged_at > st.since && now - m.merged_at < WINDOW_S && m.number > 0)
        .filter_map(|m| {
            let key = pr_key(&m.repo, m.number);
            let ticket = ticket_in(&m.title, prefix);
            (!ticket.is_empty() && !st.jira_checked.contains_key(&key)).then_some((key, ticket))
        })
        .filter(|(_, t)| seen.insert(t.clone()))
        .collect()
}

/// GraphQL-Suche nach allen PRs mit dem Ticket im Titel in der Organisation.
pub fn search_query(org: &str, ticket: &str) -> String {
    format!(
        "query{{ s: search(query:\"is:pr org:{org} in:title {ticket}\", type:ISSUE, first:50){{ nodes{{ ... on PullRequest{{ title state }} }} }} }}"
    )
}

/// Antwort der Suche; nur PRs, deren Titel das Ticket wirklich nennt (die Suche ist
/// unscharf: `ORPH-12` fände sonst auch `ORPH-123`).
pub fn parse_search(v: &Value, ticket: &str, prefix: &str) -> Result<Vec<TicketPr>, String> {
    let nodes = v.pointer("/data/s/nodes").and_then(Value::as_array).ok_or_else(|| {
        v.pointer("/errors/0/message").and_then(Value::as_str).unwrap_or("keine Daten in der Antwort").to_string()
    })?;
    Ok(nodes
        .iter()
        .filter_map(|n| {
            let title = n.get("title")?.as_str()?.to_string();
            let state = match n.get("state")?.as_str()? {
                "OPEN" => PrState::Open,
                "MERGED" => PrState::Merged,
                _ => PrState::Closed,
            };
            (ticket_in(&title, prefix) == ticket).then_some(TicketPr { title, state })
        })
        .collect())
}

/// Wohin das Ticket soll; `None` = bleibt. `current`: Status laut Jira.
pub fn target(prs: &[TicketPr], current: &str) -> Option<&'static str> {
    let is = |s: &str| current.trim().eq_ignore_ascii_case(s);
    if prs.iter().any(|p| p.state == PrState::Open) {
        return (!is(REVIEW)).then_some(REVIEW);
    }
    if prs.iter().any(|p| p.state == PrState::Merged) && !is(DONE) {
        return Some(DONE);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dash::prs::MergedPr;
    use serde_json::json;

    fn p(state: PrState) -> TicketPr {
        TicketPr { title: "x".into(), state }
    }

    #[test]
    fn all_merged_or_closed_with_one_merged_goes_to_done_once() {
        assert_eq!(target(&[p(PrState::Merged), p(PrState::Closed)], "waiting for review"), Some(DONE));
        assert_eq!(target(&[p(PrState::Merged)], "Done"), None);
        assert_eq!(target(&[p(PrState::Closed)], "ASSESSMENT"), None, "nur geschlossen ist nicht erledigt");
    }

    #[test]
    fn an_open_pr_means_waiting_for_review_even_out_of_done() {
        assert_eq!(target(&[p(PrState::Merged), p(PrState::Open)], "Done"), Some(REVIEW));
        assert_eq!(target(&[p(PrState::Open)], "Waiting for Review"), None);
    }

    #[test]
    fn the_ticket_comes_from_the_title_as_a_whole_word() {
        assert_eq!(ticket_in("feat(ORPH-4258): x", "ORPH"), "ORPH-4258");
        assert_eq!(ticket_in("XORPH-1 and ORPH-77", "ORPH"), "ORPH-77");
        assert_eq!(ticket_in("no ticket", "ORPH"), "");
    }

    #[test]
    fn search_keeps_only_titles_with_exactly_this_ticket() {
        let v = json!({ "data": { "s": { "nodes": [
            { "title": "fix(ORPH-12): a", "state": "MERGED" },
            { "title": "fix(ORPH-123): b", "state": "OPEN" },
            { "title": "ORPH-12 follow-up", "state": "CLOSED" }, {} ] } } });
        let prs = parse_search(&v, "ORPH-12", "ORPH").unwrap();
        assert_eq!(prs.iter().map(|p| p.state).collect::<Vec<_>>(), [PrState::Merged, PrState::Closed]);
        assert!(parse_search(&json!({ "errors": [ { "message": "nope" } ] }), "ORPH-12", "ORPH").is_err());
        assert!(search_query("Org", "ORPH-12").contains("org:Org in:title ORPH-12"));
    }

    #[test]
    fn only_fresh_merges_after_the_start_and_each_ticket_once_are_due() {
        let m = |n: u64, title: &str, at: f64| MergedPr { repo: "Org/r".into(), number: n, title: title.into(), merged_at: at, ..MergedPr::default() };
        let snap = PrSnapshot { merged: vec![m(1, "a ORPH-1", 990.0), m(2, "b ORPH-1", 995.0), m(3, "c ORPH-2", 500.0), m(4, "none", 999.0)], ..PrSnapshot::default() };
        let mut st = FeedState::fresh(900.0);
        assert_eq!(due(&snap, &st, "ORPH", 1000.0), [("Org/r#1".to_string(), "ORPH-1".to_string())]);
        st.jira_checked.insert("Org/r#1".into(), 1000.0);
        assert_eq!(due(&snap, &st, "ORPH", 1000.0), [("Org/r#2".to_string(), "ORPH-1".to_string())]);
        assert!(due(&snap, &st, "ORPH", 996.0 + WINDOW_S).is_empty(), "zu alt");
    }
}
