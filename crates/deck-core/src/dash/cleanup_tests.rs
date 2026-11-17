//! Tests zu `cleanup.rs`: Sicherheitsregel, Pfadvergleich, Repo aus der URL, Karte.

use super::*;

#[test]
fn only_a_clean_worktree_whose_head_is_exactly_a_merged_head_is_removable() {
    assert_eq!(classify(Some(true), Some("abc123"), &["fff", "ABC123"], false), WtState::Removable);
    assert_eq!(classify(Some(false), Some("abc123"), &["abc123"], false), WtState::Changed, "schmutzig");
    assert_eq!(classify(None, Some("abc123"), &["abc123"], false), WtState::Changed, "git status ohne Antwort");
    assert_eq!(classify(Some(true), Some("abc124"), &["abc123"], false), WtState::Changed, "Kopf weicht ab");
    assert_eq!(classify(Some(true), None, &["abc123"], false), WtState::Changed, "Kopf unbekannt");
    assert_eq!(classify(Some(true), Some(""), &[""], false), WtState::Changed, "leerer Kopf passt nie");
    assert_eq!(classify(Some(true), Some("abc123"), &[], false), WtState::Changed);
    assert_eq!(classify(Some(true), Some("abc123"), &["abc123"], true), WtState::InUse, "Agent arbeitet darin");
}

#[test]
fn paths_in_use_match_ignoring_case_slashes_and_subfolders() {
    let used = vec!["c:/repos/WT/4062-api/src".to_string(), "C:\\Repos\\wt\\other\\".to_string()];
    assert!(is_in_use("C:\\Repos\\wt\\4062-api", &used));
    assert!(is_in_use("C:/Repos/wt/other", &used));
    assert!(!is_in_use("C:\\Repos\\wt\\4062", &used), "nur ganze Ordnernamen");
    assert!(!is_in_use("", &used));
    assert!(!is_in_use("C:\\Repos\\wt\\4062-api", &[]));
}

#[test]
fn origin_urls_of_every_form_give_owner_and_name() {
    let want = Some("Org/agent-deck".to_string());
    assert_eq!(repo_from_url("https://github.com/Org/agent-deck.git"), want);
    assert_eq!(repo_from_url("https://github.com/Org/agent-deck/\n"), want);
    assert_eq!(repo_from_url("git@github.com:Org/agent-deck.git"), want);
    assert_eq!(repo_from_url("ssh://git@github.com/Org/agent-deck"), want);
    assert_eq!(repo_from_url("https://user@github.com/Org/agent-deck"), want);
    assert_eq!(repo_from_url("C:\\lokal\\repo"), None);
    assert_eq!(repo_from_url(""), None);
}

