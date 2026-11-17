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

/// Schalter in `deck_settings.json`: Info-Karten zeigen (Standard an).
pub const SETTING: &str = "dashboard";

/// Sind die Info-Karten eingeschaltet? Nur ein ausdrückliches `false` schaltet ab.
pub fn enabled(settings: &serde_json::Map<String, serde_json::Value>) -> bool {
    settings.get(SETTING).and_then(serde_json::Value::as_bool).unwrap_or(true)
}

/// Hosts, deren Adressen ein Klick auf eine Info-Karte öffnen darf. Die Adressen kommen
/// aus fremden Antworten (GitHub, Jira); was dort steht, soll keinen beliebigen Aufruf
/// auslösen können.
const URL_HOSTS: [&str; 3] = ["github.com", "idmedizin.atlassian.net", "claude.ai"];

/// Darf `open_dash_url` diese Adresse öffnen? Nur `https://` auf [`URL_HOSTS`].
pub fn allowed_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    URL_HOSTS.contains(&host) && !url.chars().any(char::is_whitespace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_links_to_known_hosts_open() {
        assert!(allowed_url("https://github.com/pulls"));
        assert!(allowed_url("https://idmedizin.atlassian.net/browse/ORPH-4256"));
        assert!(!allowed_url("http://github.com/pulls"));
        assert!(!allowed_url("https://github.com.evil.example/x"));
        assert!(!allowed_url("file:///C:/Windows/System32/calc.exe"));
        assert!(!allowed_url("https://github.com/a b"));
    }
}
