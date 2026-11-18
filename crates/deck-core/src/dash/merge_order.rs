//! Reihenfolge zwischen PRs für die Merge-Automatik (`automation.rs`).
//!
//! Konvention (17.11.2026): in der PR-Beschreibung eine oder mehrere Zeilen
//! `Merge after: <repo>#<n>` bzw. `Merge after: <owner>/<repo>#<n>` (auch mehrere,
//! durch Komma getrennt), ebenso `Depends on:`, `Blocked by:`, `Requires:`. Dazu eine Zeile,
//! die nur aus „after #N“ / „depends on #N“ besteht (selbes Repo). Mitten im Satz zählt
//! nichts: ein Satz wie „…added an index after #12 was written“ hielt einen PR einen Tag
//! lang fest, weil #12 ein geschlossener, nie gemergter PR war.
//! Ob eine Abhängigkeit gemergt ist, fragt der Dienst selbst bei GitHub nach
//! ([`deps_query`], [`parse_deps`]); unbekannt heißt warten.
//!
//! Zusätzlich setzt ein zentraler Workflow auf dem PR-Kopf den Commit-Status
//! [`MERGE_ORDER`]. Ist er vorhanden, muss er `success` sein.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use super::jira_sync::PrState;
use super::prs::{pr_key, short_repo};

/// Name des Commit-Status, den der zentrale Workflow setzt.
pub const MERGE_ORDER: &str = "merge-order";

/// Stand der Abhängigkeiten: PR-Schlüssel (`owner/repo#12`) → Zustand.
pub type DepStates = BTreeMap<String, PrState>;

static LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?im)^[\s>*-]*(?:merge\s+after|depends\s+on|blocked\s+by|requires)\s*:\s*(.+)$").expect("LINE"));
static REF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:([A-Za-z0-9_.-]+)/)?([A-Za-z0-9_.-]+)#(\d+)").expect("REF"));
/// `#N` ohne Repo in einer solchen Zeile: selbes Repo.
static BARE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|[\s,(])#(\d+)").expect("BARE"));
/// Eine Zeile nur aus „after #N“ (ohne Doppelpunkt), höchstens mit Punkt dahinter.
static AFTER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?im)^[\s>*-]*(?:merge\s+after|after|depends\s+on|blocked\s+by|requires)\s+(?:PR\s+)?#(\d+)[\s.]*$").expect("AFTER")
});

/// Alle Abhängigkeiten aus der Beschreibung eines PRs in `repo` (`owner/name`), als
/// PR-Schlüssel. Ein Repo ohne Besitzer gehört derselben Organisation.
pub fn dependencies(body: &str, repo: &str) -> Vec<String> {
    let owner = repo.split_once('/').map_or("", |(o, _)| o);
    let mut out: Vec<String> = Vec::new();
    for line in LINE.captures_iter(body) {
        for r in REF.captures_iter(&line[1]) {
            let o = r.get(1).map_or(owner, |m| m.as_str());
            let Ok(n) = r[3].parse::<u64>() else { continue };
            out.push(pr_key(&format!("{o}/{}", &r[2]), n));
        }
        for b in BARE.captures_iter(&line[1]) {
            if let Ok(n) = b[1].parse::<u64>() {
                out.push(pr_key(repo, n));
            }
        }
    }
    for c in AFTER.captures_iter(body) {
        if let Ok(n) = c[1].parse::<u64>() {
            out.push(pr_key(repo, n));
        }
    }
    out.dedup();
    out
}

/// `owner/repo#12` → `api#12` für die Anzeige.
pub fn dep_label(key: &str) -> String {
    match key.rsplit_once('#') {
        Some((repo, n)) => format!("{}#{n}", short_repo(repo)),
        None => key.to_string(),
    }
}

/// Die erste Abhängigkeit, die noch nicht gemergt ist (unbekannt zählt als nicht gemergt).
pub fn first_unmerged<'a>(deps: &'a [String], states: &DepStates) -> Option<&'a String> {
    deps.iter().find(|k| states.get(*k) != Some(&PrState::Merged))
}

fn safe(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Eine GraphQL-Abfrage nach dem Zustand aller `keys` (Aliasse `d0`, `d1` …). Schlüssel
/// mit fremden Zeichen fallen weg (sie stammen aus Beschreibungen).
pub fn deps_query(keys: &[String]) -> Option<String> {
    let mut parts = Vec::new();
    for (i, k) in keys.iter().enumerate() {
        let Some((repo, n)) = k.rsplit_once('#') else { continue };
        let Some((owner, name)) = repo.split_once('/') else { continue };
        if !safe(owner) || !safe(name) || n.parse::<u64>().is_err() {
            continue;
        }
        parts.push(format!("d{i}: repository(owner:\"{owner}\", name:\"{name}\"){{ pullRequest(number:{n}){{ state }} }}"));
    }
    (!parts.is_empty()).then(|| format!("query{{ {} }}", parts.join(" ")))
}

/// Antwort auf [`deps_query`]: Zustand je Schlüssel; was fehlt, bleibt unbekannt.
pub fn parse_deps(v: &Value, keys: &[String]) -> DepStates {
    keys.iter()
        .enumerate()
        .filter_map(|(i, k)| {
            let s = v.pointer(&format!("/data/d{i}/pullRequest/state"))?.as_str()?;
            let st = match s {
                "MERGED" => PrState::Merged,
                "OPEN" => PrState::Open,
                _ => PrState::Closed,
            };
            Some((k.clone(), st))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merge_after_lines_name_repos_with_or_without_owner() {
        let body = "## Why\nstuff\n\nMerge after: orpheus-management-api#212, Other/helm-charts#7\n- merge after: deck#3\nafter #9";
        assert_eq!(
            dependencies(body, "Org/orpheus-management-ui"),
            ["Org/orpheus-management-api#212", "Other/helm-charts#7", "Org/deck#3", "Org/orpheus-management-ui#9"]
        );
        assert!(dependencies("Merges after lunch #4 is fine", "Org/x").is_empty());
        let other = "Depends on: #5, api#6\n- Blocked by: Other/x#1";
        assert_eq!(dependencies(other, "Org/orpheus-management-ui"), ["Org/api#6", "Org/orpheus-management-ui#5", "Other/x#1"]);
    }

    #[test]
    fn after_a_number_in_running_text_is_no_dependency() {
        let body = "- **ABC-1 (#30)** added an index after #12 was written.\nThis requires #9 to be understood.";
        assert!(dependencies(body, "Org/x").is_empty());
        assert_eq!(dependencies("Depends on PR #3.", "Org/x"), ["Org/x#3"]);
        assert_eq!(dep_label("Org/orpheus-management-api#212"), "api#212");
    }

    #[test]
    fn only_a_merged_dependency_lets_the_pr_go() {
        let deps = vec!["Org/a#1".to_string(), "Org/b#2".to_string()];
        let mut st = DepStates::new();
        assert_eq!(first_unmerged(&deps, &st), Some(&deps[0]), "unbekannt heißt warten");
        st.insert("Org/a#1".into(), PrState::Merged);
        st.insert("Org/b#2".into(), PrState::Closed);
        assert_eq!(first_unmerged(&deps, &st), Some(&deps[1]), "geschlossen ohne Merge reicht nicht");
        st.insert("Org/b#2".into(), PrState::Merged);
        assert_eq!(first_unmerged(&deps, &st), None);
    }

    #[test]
    fn the_query_asks_each_dependency_and_skips_odd_names() {
        let keys = vec!["Org/a#1".to_string(), "Org/b\"x#2".to_string()];
        let q = deps_query(&keys).unwrap();
        assert!(q.contains("d0: repository(owner:\"Org\", name:\"a\"){ pullRequest(number:1){ state } }"));
        assert!(!q.contains("d1"));
        let v = json!({ "data": { "d0": { "pullRequest": { "state": "MERGED" } }, "d1": null } });
        assert_eq!(parse_deps(&v, &keys), DepStates::from([("Org/a#1".to_string(), PrState::Merged)]));
        assert_eq!(deps_query(&[]), None);
    }
}
