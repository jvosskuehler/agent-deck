//! Tests zu `account.rs` (aus `tests/test_claude_usage.py`, „Zwei Konten“). Kein Netz:
//! das Profil kommt aus einer Attrappe hinter `UsageHttp`.

use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::json;

use super::super::fetch::{HttpReply, Transport, USAGE_URL};
use super::*;

/// Profil-Endpoint als Attrappe: Token → Konto, unbekanntes Token → 401. Zählt Abrufe.
pub(crate) struct ProfileHttp {
    pub accounts: Vec<(&'static str, &'static str)>,
    pub calls: AtomicUsize,
}

impl ProfileHttp {
    pub fn new(accounts: Vec<(&'static str, &'static str)>) -> ProfileHttp {
        ProfileHttp { accounts, calls: AtomicUsize::new(0) }
    }
}

impl UsageHttp for ProfileHttp {
    fn get(&self, url: &str, token: &str) -> Result<HttpReply, Transport> {
        assert_eq!(url, PROFILE_URL, "nur das Profil wird gefragt");
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(match self.accounts.iter().find(|(t, _)| *t == token) {
            Some((_, acc)) => HttpReply { status: 200, retry_after: None, body: json!({"account": {"email_address": acc}}).to_string() },
            None => HttpReply { status: 401, retry_after: None, body: String::new() },
        })
    }
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|t| t.to_string()).collect()
}

#[test]
fn a_desktop_token_of_another_account_is_not_asked() {
    // Bei 429 auf dem CLI-Token sprang der Abruf aufs Desktop-Token. Gehört das einem
    // anderen Konto, wären das still dessen Zahlen.
    let of = |t: &str| match t {
        "cli" => Some("b@x.de".to_string()),
        "desk" => Some("a@x.de".to_string()),
        _ => None,
    };
    assert_eq!(select_tokens(&s(&["cli"]), Some("b@x.de"), &s(&["desk"]), &of), (s(&["cli"]), Some("b@x.de".into())));
}

#[test]
fn a_desktop_token_of_the_same_account_comes_after_the_cli() {
    let of = |_: &str| Some("a@x.de".to_string());
    assert_eq!(select_tokens(&s(&["cli"]), Some("a@x.de"), &s(&["desk"]), &of), (s(&["cli", "desk"]), Some("a@x.de".into())));
}

#[test]
fn without_a_known_cli_account_desktop_stays_out() {
    let of = |_: &str| Some("a@x.de".to_string());
    assert_eq!(select_tokens(&s(&["cli"]), None, &s(&["desk"]), &of), (s(&["cli"]), None));
}

#[test]
fn without_a_cli_login_desktop_carries_alone() {
    let of = |_: &str| Some("a@x.de".to_string());
    assert_eq!(select_tokens(&[], None, &s(&["desk", "desk2"]), &of), (s(&["desk", "desk2"]), Some("a@x.de".into())));
    assert_eq!(select_tokens(&[], None, &[], &of), (Vec::new(), None));
}

#[test]
fn the_account_comes_from_the_profile_normalised() {
    assert_eq!(account_from_profile(&json!({"account": {"email_address": "  Jorrit@X.de "}})).as_deref(), Some("jorrit@x.de"));
    assert_eq!(account_from_profile(&json!({"account": {"email_address": "", "email": "b@x.de"}})).as_deref(), Some("b@x.de"));
    for junk in [json!({}), json!({"account": null}), json!({"account": {"email_address": "   "}}), json!({"account": {"email": 5}})] {
        assert_eq!(account_from_profile(&junk), None, "{junk}");
    }
}

#[test]
fn the_cli_account_is_read_from_oauth_account() {
    assert_eq!(cli_account_from(&json!({"oauthAccount": {"emailAddress": "A@x.de"}})).as_deref(), Some("a@x.de"));
    assert_eq!(cli_account_from(&json!({"oauthAccount": null})), None);
    assert_eq!(cli_account_from(&json!([])), None);
}

#[test]
fn the_cli_account_file_follows_the_environment_variable() {
    let home = std::path::Path::new(r"C:\Users\x");
    let moved = std::ffi::OsStr::new(r"X:\woanders");
    assert_eq!(cli_account_path_in(Some(moved), home), PathBuf::from(r"X:\woanders\.claude.json"), "darin, nicht daneben");
    assert_eq!(cli_account_path_in(None, home), PathBuf::from(r"C:\Users\x\.claude.json"), "im Home, nicht in .claude");
    assert_eq!(cli_account_path_in(Some(std::ffi::OsStr::new("")), home), home.join(".claude.json"), "leer = nicht gesetzt");
}

#[test]
fn the_account_of_a_token_is_asked_once_and_failures_are_asked_again() {
    let http = Arc::new(ProfileHttp::new(vec![("tok-a", "a@x.de")]));
    let accounts = TokenAccounts::new(http.clone());
    assert_eq!(accounts.account_of("tok-a").as_deref(), Some("a@x.de"));
    assert_eq!(accounts.account_of("tok-a").as_deref(), Some("a@x.de"));
    assert_eq!(http.calls.load(Ordering::SeqCst), 1, "ein Erfolg wird gemerkt");
    assert_eq!(accounts.account_of("tok-x"), None);
    assert_eq!(accounts.account_of("tok-x"), None);
    assert_eq!(http.calls.load(Ordering::SeqCst), 3, "ein Fehler wird neu gefragt");
}

#[test]
fn a_network_error_leaves_the_account_unknown() {
    struct Down;
    impl UsageHttp for Down {
        fn get(&self, url: &str, _: &str) -> Result<HttpReply, Transport> {
            assert_ne!(url, USAGE_URL);
            Err(Transport::Network)
        }
    }
    assert_eq!(TokenAccounts::new(Arc::new(Down)).account_of("t"), None);
}
