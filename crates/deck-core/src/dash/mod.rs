//! Das Info-Fenster in der freien Fensterkachel neben den Agenten.
//!
//! Liegen die Agentenfenster eines Blocks so, dass im Raster eine Kachel frei bleibt (5
//! Fenster als 3 + 2), stellt das Deck dort ein eigenes Fenster hin; ohne Lücke holt es
//! die Info-Taste (Strg+Alt+D) nach vorn. Oben die Tokenkosten je Ticket über die Zeit
//! (`costs`, `cost_ledger`), unten eine Leiste mit PRs, Reviews, Titan und Aufräumen
//! (`board`). Die Dienste (`deck-services/src/dash/`) liefern nur Daten; das
//! Anzeigemodell baut `board.rs`.

pub mod automation;
pub mod bg_tasks;
pub mod board;
pub mod ci_cause;
pub mod cleanup;
pub mod cost_ledger;
pub mod costs;
pub mod feed;
pub mod feed_events;
pub mod feed_state;
pub mod feed_view;
pub mod jira_sync;
pub mod merge_order;
pub mod pr_extra;
pub mod prs;
pub mod reviewer_match;
pub mod reviewers;
pub mod titan;

use serde::{Deserialize, Serialize};
use ts_rs::TS;


/// Ton einer Angabe: bestimmt nur ihre Schriftfarbe (`app/src/styles/dash.css`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum DashTone {
    /// Normale Textfarbe.
    Plain,
    /// Gedämpft (Nebensache).
    Muted,
    /// Gut (grün: mergebereit, reicht).
    Good,
    /// Achtung (amber: offene Threads, knapp).
    Warn,
    /// Schlecht (rot: CI rot, Kontingent leer vor dem Reset).
    Bad,
}

use crate::deck_settings::{Switch, on};

type Settings = serde_json::Map<String, serde_json::Value>;

/// Schalter in `deck_settings.json`: Info-Fenster zeigen (Standard an, es liest nur).
pub const SETTING: &str = "dashboard";

/// Ist das Info-Fenster eingeschaltet?
pub fn enabled(settings: &Settings) -> bool {
    on(settings, Switch::Dashboard)
}

/// Titan-Kachel samt ssh-Abfrage (`titan`, neue Installationen: aus). Aus heißt: kein
/// einziger ssh-Aufruf.
pub fn titan_enabled(settings: &Settings) -> bool {
    on(settings, Switch::Titan)
}

/// PR-Automatik (`pr_automation`, neue Installationen: aus): mergt, setzt Reviewer und
/// Assignee, startet Hintergrund-Agenten. Die eine Stelle, an der sie fragen muss.
pub fn automation_enabled(settings: &Settings) -> bool {
    on(settings, Switch::PrAutomation)
}

/// Hosts, deren Adressen ein Klick auf eine Info-Karte öffnen darf. Die Adressen kommen
/// aus fremden Antworten (GitHub, Jira); was dort steht, soll keinen beliebigen Aufruf
/// auslösen können.
const URL_HOSTS: [&str; 2] = ["github.com", "claude.ai"];

/// Jira Cloud: jede Seite `<firma>.atlassian.net`, denn jede Firma hat ihre eigene.
const URL_HOST_SUFFIX: &str = ".atlassian.net";

/// Darf `open_dash_url` diese Adresse öffnen? Nur `https://` auf [`URL_HOSTS`] oder einen
/// Host `<name>.atlassian.net` (genau eine Ebene, ohne Port, ohne Anmeldedaten).
pub fn allowed_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let jira = host.strip_suffix(URL_HOST_SUFFIX).is_some_and(|name| {
        !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    });
    (URL_HOSTS.contains(&host) || jira) && !url.chars().any(char::is_whitespace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_links_to_known_hosts_open() {
        assert!(allowed_url("https://github.com/pulls"));
        assert!(allowed_url("https://idmedizin.atlassian.net/browse/ORPH-4256"));
        assert!(allowed_url("https://acme-dev.atlassian.net/browse/ABC-1"));
        assert!(!allowed_url("https://atlassian.net/browse/ABC-1"), "kein Firmenname");
        assert!(!allowed_url("https://.atlassian.net/x"));
        assert!(!allowed_url("https://evil.example/x.atlassian.net"), "Pfad zählt nicht als Host");
        assert!(!allowed_url("https://a.b.atlassian.net/x"), "nur eine Ebene");
        assert!(!allowed_url("https://user@acme.atlassian.net/x"), "keine Anmeldedaten");
        assert!(!allowed_url("https://acme.atlassian.net.evil.example/x"));
        assert!(!allowed_url("https://acme.atlassian.net:8443/x"), "kein Port");
        assert!(!allowed_url("http://github.com/pulls"));
        assert!(!allowed_url("https://github.com.evil.example/x"));
        assert!(!allowed_url("file:///C:/Windows/System32/calc.exe"));
        assert!(!allowed_url("https://github.com/a b"));
    }
}
