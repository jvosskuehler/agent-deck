//! Reviewer-Namen gegen die Mitglieder der GitHub-Organisation auflösen
//! (`deck_core::dash::reviewers`). Die Mitglieder kommen über `gh` ([`GhDirectory`]),
//! hinter dem Trait [`OrgDirectory`]; Tests bekommen eine Attrappe.
//!
//! Gebraucht von `deck-setup resolve-reviewers` (Installer) und vom Einstellungsfenster.
//! Nur lesend. `GITHUB_TOKEN`/`GH_TOKEN` werden dem Kindprozess entzogen wie überall im
//! Feed (`dash::github`): der Token in der Umgebung ist oft ein anderer oder abgelaufen,
//! die Keyring-Anmeldung von gh gilt.

use std::collections::HashMap;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use deck_core::dash::reviewers::{self, Member, OrgPick, ReviewerNote, ReviewerReport, members_query, parse_members_page};
use serde_json::Value;

use crate::dash::hidden_cmd::{self, short_error};

/// Zeitlimit je gh-Aufruf.
pub const TIMEOUT: Duration = Duration::from_secs(30);
/// So lange gilt eine gelesene Mitgliederliste (das Fenster fragt bei jeder Änderung).
pub const CACHE_FOR: Duration = Duration::from_secs(600);
/// Höchstens so viele Seiten zu 100 Mitgliedern.
const MAX_PAGES: usize = 50;

/// Wer in welcher Organisation ist.
pub trait OrgDirectory: Send + Sync {
    /// Logins der Organisationen des angemeldeten Kontos.
    fn my_orgs(&self) -> Result<Vec<String>, String>;
    /// Alle Mitglieder einer Organisation.
    fn members(&self, org: &str) -> Result<Vec<Member>, String>;
}

/// Die echte Fassung über `gh`.
#[derive(Debug, Clone, Copy)]
pub struct GhDirectory {
    pub timeout: Duration,
}

impl Default for GhDirectory {
    fn default() -> GhDirectory {
        GhDirectory { timeout: TIMEOUT }
    }
}

impl GhDirectory {
    fn gh(&self, args: &[String]) -> Result<hidden_cmd::Output, String> {
        let mut cmd = Command::new("gh");
        cmd.args(args).env_remove("GITHUB_TOKEN").env_remove("GH_TOKEN");
        hidden_cmd::run(cmd, self.timeout)
    }

    fn page(&self, full: bool, org: &str, cursor: Option<&str>) -> Result<Value, String> {
        let mut args: Vec<String> = vec!["api".into(), "graphql".into(), "-f".into(), format!("query={}", members_query(full))];
        args.extend(["-f".into(), format!("org={org}")]);
        if let Some(c) = cursor {
            args.extend(["-f".into(), format!("cursor={c}")]);
        }
        let out = self.gh(&args)?;
        // Bei GraphQL-Fehlern endet gh mit 1 und druckt die Antwort trotzdem.
        if out.stdout.trim_start().starts_with('{') {
            return serde_json::from_str(&out.stdout).map_err(|_| "Antwort kein JSON".to_string());
        }
        let why = short_error(&out.stderr);
        Err(if why.is_empty() { format!("gh: Exit {}", out.code) } else { format!("gh: {}", why.trim_start_matches("gh: ")) })
    }

    fn all_pages(&self, full: bool, org: &str) -> Result<Vec<Member>, String> {
        let mut all = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let v = self.page(full, org, cursor.as_deref())?;
            let (members, next) = parse_members_page(&v).map_err(|e| short_error(&e))?;
            all.extend(members);
            match next {
                Some(c) => cursor = Some(c),
                None => break,
            }
        }
        Ok(all)
    }
}

impl OrgDirectory for GhDirectory {
    fn my_orgs(&self) -> Result<Vec<String>, String> {
        let out = self.gh(&["api".into(), "user/orgs".into(), "--jq".into(), ".[].login".into()])?;
        if out.code != 0 {
            let why = short_error(&out.stderr);
            return Err(if why.is_empty() { format!("gh: Exit {}", out.code) } else { format!("gh: {}", why.trim_start_matches("gh: ")) });
        }
        Ok(out.stdout.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect())
    }

    fn members(&self, org: &str) -> Result<Vec<Member>, String> {
        // Die Firmenadressen zeigt GitHub nicht jedem: ohne sie noch einmal.
        self.all_pages(true, org).or_else(|_| self.all_pages(false, org))
    }
}

/// Löst Eingaben auf und merkt sich Mitgliederlisten eine Weile.
pub struct ReviewerResolver {
    dir: Arc<dyn OrgDirectory>,
    cache: Mutex<HashMap<String, (Instant, Vec<Member>)>>,
    ttl: Duration,
}

impl ReviewerResolver {
    pub fn new(dir: Arc<dyn OrgDirectory>, ttl: Duration) -> ReviewerResolver {
        ReviewerResolver { dir, cache: Mutex::new(HashMap::new()), ttl }
    }

    /// Die echte Fassung über gh.
    pub fn system() -> ReviewerResolver {
        ReviewerResolver::new(Arc::new(GhDirectory::default()), CACHE_FOR)
    }

    fn members(&self, org: &str) -> Result<Vec<Member>, String> {
        let key = org.to_lowercase();
        {
            let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((at, list)) = cache.get(&key)
                && at.elapsed() < self.ttl
            {
                return Ok(list.clone());
            }
        }
        let list = self.dir.members(org)?;
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).insert(key, (Instant::now(), list.clone()));
        Ok(list)
    }

    /// `org_setting`: `github_org` (leer = aus dem Konto bestimmen). `raw`: die Eingabe.
    /// Scheitert nie: ohne gh, ohne Organisation oder bei mehreren zur Wahl bleibt jeder
    /// Eintrag unaufgelöst, und `note` sagt, warum.
    pub fn resolve(&self, org_setting: &str, raw: &str) -> ReviewerReport {
        let mut report = ReviewerReport {
            org: String::new(),
            orgs: Vec::new(),
            note: ReviewerNote::None,
            detail: String::new(),
            results: Vec::new(),
        };
        if reviewers::split_input(raw).is_empty() {
            return report;
        }
        match reviewers::pick_org(org_setting, || self.dir.my_orgs()) {
            OrgPick::Use(org) => {
                match self.members(&org) {
                    Ok(list) => report.results = reviewers::resolve_all(Some(&list), raw),
                    Err(e) => {
                        report.note = ReviewerNote::MembersFailed;
                        report.detail = short_error(&e);
                    }
                }
                report.org = org;
            }
            OrgPick::Choose(orgs) => {
                report.note = ReviewerNote::ChooseOrg;
                report.orgs = orgs;
            }
            OrgPick::Without(note, detail) => {
                report.note = note;
                report.detail = short_error(&detail);
            }
        }
        if report.results.is_empty() {
            report.results = reviewers::resolve_all(None, raw);
        }
        report
    }
}

#[cfg(test)]
#[path = "reviewers_tests.rs"]
mod tests;
