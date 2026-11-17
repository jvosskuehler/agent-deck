//! Pull Requests fürs Info-Fenster: die eigenen offenen samt „wer ist am Zug“ und die
//! fremden, bei denen ein Review von mir erbeten ist.
//!
//! Der Dienst (`deck-services/src/dash/github.rs`) fragt GitHub in EINEM GraphQL-Aufruf
//! mit mehreren Aliassen ab; dieses Modul zerlegt die Antwort ([`parse`]) und baut die
//! Karte ([`card`]). Die Aliasse sind der Vertrag zwischen beiden Seiten ([`ALIAS_OPEN`]
//! usw.). Gelesen wird tolerant: ein fehlendes Feld heißt „keins“, nie ein Fehler.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use crate::isotime::parse_iso;

use super::pr_extra::PrExtra;

/// Alias der offenen PRs.
pub const ALIAS_OPEN: &str = "open";
/// Alias der offenen PRs, bei denen ein Review von mir erbeten ist.
pub const ALIAS_REVIEWS: &str = "reviews";
/// Alias: Anzahl heute (Ortszeit) erstellter PRs.
pub const ALIAS_CREATED_TODAY: &str = "createdToday";
/// Alias: Anzahl heute gemergter PRs.
pub const ALIAS_MERGED_TODAY: &str = "mergedToday";
/// Präfix der Aliasse mit gemergten PRs (`merged0`, `merged1` …). Mehrere Zeitfenster,
/// weil eine Suche höchstens 100 Treffer liefert und 30 Tage mehr hergeben können.
pub const ALIAS_MERGED_PREFIX: &str = "merged";

/// Ab diesem Alter gilt der Stand als alt (s): matt zeichnen, Alter im Hover.
pub const STALE_AFTER_S: f64 = 15.0 * 60.0;

/// Zustand der Checks am letzten Commit (`statusCheckRollup.state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CiState {
    /// Keine Checks (oder unbekannter Wert).
    #[default]
    None,
    Success,
    Failure,
    Running,
}

/// Review-Stand (`reviewDecision`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReviewState {
    /// Kein Review verlangt (oder unbekannter Wert).
    #[default]
    None,
    Approved,
    ChangesRequested,
    Required,
}

/// Ein offener PR.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PrInfo {
    /// `owner/name`.
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub head_ref: String,
    pub draft: bool,
    pub ci: CiState,
    pub review: ReviewState,
    /// `mergeable == MERGEABLE`; `UNKNOWN` (GitHub rechnet noch) zählt als nein.
    pub mergeable: bool,
    /// Nicht aufgelöste Review-Threads.
    pub open_threads: u32,
    /// Offene Review-Anfragen an Menschen (Login bzw. Team), Bots nicht mitgezählt.
    pub waiting_on: Vec<String>,
    /// Nach dem letzten „Änderungen gewünscht“ kamen Commits (umgesetzt).
    pub changed_since_request: bool,
    /// Kommentare, Threads, Checks, Dateien … für Feed und Automatik (`pr_extra.rs`).
    pub extra: PrExtra,
}

impl PrInfo {
    /// Schlüssel eines PRs in Zustandsdateien und Befehlen: `owner/repo#12`.
    pub fn key(&self) -> String {
        pr_key(&self.repo, self.number)
    }

    /// Kurz für die Anzeige: `client#393`.
    pub fn label(&self) -> String {
        format!("{}#{}", short_repo(&self.repo), self.number)
    }
}

/// `owner/repo#12`.
pub fn pr_key(repo: &str, number: u64) -> String {
    format!("{repo}#{number}")
}

/// Wer bei einem PR am Zug ist. „Änderungen gewünscht“ bleibt bei GitHub stehen, bis der
/// Reviewer neu prüft; ist er wieder angefragt, liegt der Ball bei ihm, nicht bei mir.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PrBall {
    /// CI grün, freigegeben, ohne Konflikt.
    Ready,
    CiRed,
    /// Änderungen gewünscht, noch nichts umgesetzt.
    ChangesOpen,
    /// Umgesetzt, aber den Reviewer nicht wieder angefragt.
    AskReviewer,
    /// Änderungen gewünscht, Reviewer wieder angefragt.
    ReReview,
    Review,
    Draft,
}

impl PrBall {
    /// Bin ich dran?
    pub fn is_mine(self) -> bool {
        matches!(self, PrBall::CiRed | PrBall::ChangesOpen | PrBall::AskReviewer)
    }
}

/// Reviewer, die Bots sind: ihre Anfragen zählen nicht als „wartet auf“.
pub fn is_bot(login: &str) -> bool {
    const BOTS: [&str; 5] = ["claude", "github-code-quality", "copilot-pull-request-reviewer", "coderabbitai", "sonarqubecloud"];
    login.ends_with("[bot]") || BOTS.contains(&login)
}

impl PrInfo {
    /// Wer am Zug ist.
    pub fn ball(&self) -> PrBall {
        if self.draft {
            PrBall::Draft
        } else if self.ci == CiState::Failure {
            PrBall::CiRed
        } else if self.review == ReviewState::ChangesRequested {
            if !self.waiting_on.is_empty() {
                PrBall::ReReview
            } else if self.changed_since_request {
                PrBall::AskReviewer
            } else {
                PrBall::ChangesOpen
            }
        } else if self.is_ready() {
            PrBall::Ready
        } else {
            PrBall::Review
        }
    }

    /// Kann sofort gemergt werden: CI grün, freigegeben, kein Draft, ohne Konflikt.
    pub fn is_ready(&self) -> bool {
        self.ci == CiState::Success && self.review == ReviewState::Approved && !self.draft && self.mergeable
    }
}

/// Ein gemergter PR (für das Aufräumen: ist der Kopf eines worktree schon gemergt? Und
/// für die Zeitleiste des Feeds).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MergedPr {
    /// `owner/name`.
    pub repo: String,
    pub head_ref: String,
    /// Commit, auf dem der Branch beim Merge stand.
    pub head_oid: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    /// Unix-Sekunden; 0 = unbekannt.
    pub merged_at: f64,
}

/// Ein PR, bei dem ein Review von mir erbeten ist.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReviewRequest {
    /// `owner/name`.
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    /// Unix-Sekunden.
    pub created_at: f64,
    pub additions: u64,
    pub deletions: u64,
    pub ci: CiState,
}

/// Letzter Stand des Dienstes.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PrSnapshot {
    pub open: Vec<PrInfo>,
    /// Offene PRs anderer, bei denen ein Review von mir erbeten ist.
    pub review_requests: Vec<ReviewRequest>,
    /// `None` = unbekannt (noch nie abgerufen oder nicht in der Antwort).
    pub created_today: Option<u32>,
    pub merged_today: Option<u32>,
    /// Gemergte PRs der letzten 30 Tage.
    pub merged: Vec<MergedPr>,
    /// Unix-Sekunden des letzten erfolgreichen Abrufs; 0 = noch keiner.
    pub fetched_at: f64,
    /// Der letzte Abruf scheiterte (kurz, ohne Token); die Daten sind dann die alten.
    pub error: Option<String>,
    /// Mein Login (`viewer.login`); leer = unbekannt.
    pub viewer: String,
}

fn s(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

fn count(v: Option<&Value>) -> Option<u32> {
    v?.get("issueCount")?.as_u64().map(|n| n.min(u64::from(u32::MAX)) as u32)
}

fn nodes(v: Option<&Value>) -> &[Value] {
    v.and_then(|x| x.get("nodes")).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

fn parse_pr(n: &Value) -> Option<PrInfo> {
    // Knoten ohne Nummer sind keine PRs (die Suche liefert die Union `SearchResultItem`).
    let number = n.get("number")?.as_u64()?;
    let ci = ci_of(n);
    let review = match n.get("reviewDecision").and_then(Value::as_str) {
        Some("APPROVED") => ReviewState::Approved,
        Some("CHANGES_REQUESTED") => ReviewState::ChangesRequested,
        Some("REVIEW_REQUIRED") => ReviewState::Required,
        _ => ReviewState::None,
    };
    let waiting_on: Vec<String> = nodes(n.get("reviewRequests"))
        .iter()
        .filter_map(|r| {
            let who = r.get("requestedReviewer")?;
            who.get("login").or_else(|| who.get("name")).and_then(Value::as_str).map(String::from)
        })
        .filter(|w| !is_bot(w))
        .collect();
    let events = nodes(n.get("timelineItems"));
    let last_request = events
        .iter()
        .filter(|e| e.get("state").and_then(Value::as_str) == Some("CHANGES_REQUESTED"))
        .filter_map(|e| e.get("submittedAt").and_then(Value::as_str).and_then(parse_iso))
        .fold(None, |m: Option<f64>, t| Some(m.map_or(t, |m| m.max(t))));
    let changed_since_request = last_request.is_some_and(|req| {
        events
            .iter()
            .filter_map(|e| e.pointer("/commit/committedDate").and_then(Value::as_str).and_then(parse_iso))
            .any(|t| t > req)
    });
    let open_threads = nodes(n.get("reviewThreads"))
        .iter()
        .filter(|t| t.get("isResolved").and_then(Value::as_bool) == Some(false))
        .count() as u32;
    Some(PrInfo {
        repo: n.pointer("/repository/nameWithOwner").and_then(Value::as_str).unwrap_or("").to_string(),
        number,
        title: s(n, "title"),
        url: s(n, "url"),
        head_ref: s(n, "headRefName"),
        draft: n.get("isDraft").and_then(Value::as_bool).unwrap_or(false),
        ci,
        review,
        mergeable: n.get("mergeable").and_then(Value::as_str) == Some("MERGEABLE"),
        open_threads,
        waiting_on,
        changed_since_request,
        extra: PrExtra::default(),
    })
}

fn ci_of(n: &Value) -> CiState {
    match nodes(n.get("commits")).last().and_then(|c| c.pointer("/commit/statusCheckRollup/state")).and_then(Value::as_str) {
        Some("SUCCESS") => CiState::Success,
        Some("FAILURE" | "ERROR") => CiState::Failure,
        Some("PENDING" | "EXPECTED") => CiState::Running,
        _ => CiState::None,
    }
}

fn parse_review(n: &Value) -> Option<ReviewRequest> {
    let number = n.get("number")?.as_u64()?;
    Some(ReviewRequest {
        repo: n.pointer("/repository/nameWithOwner").and_then(Value::as_str).unwrap_or("").to_string(),
        number,
        title: s(n, "title"),
        url: s(n, "url"),
        created_at: n.get("createdAt").and_then(Value::as_str).and_then(parse_iso).unwrap_or(0.0),
        additions: n.get("additions").and_then(Value::as_u64).unwrap_or(0),
        deletions: n.get("deletions").and_then(Value::as_u64).unwrap_or(0),
        ci: ci_of(n),
    })
}

/// Antwort von `gh api graphql` zerlegen. `fetched_at` und `error` bleiben leer, die
/// setzt der Dienst. Ohne `data` ist das ein Fehler (erste GraphQL-Meldung); Teilfehler
/// neben Daten (z. B. eine Organisation mit SAML-Sperre) werden hingenommen.
pub fn parse(v: &Value) -> Result<PrSnapshot, String> {
    let Some(data) = v.get("data").filter(|d| d.is_object()) else {
        let msg = v
            .pointer("/errors/0/message")
            .and_then(Value::as_str)
            .or_else(|| v.get("message").and_then(Value::as_str))
            .unwrap_or("keine Daten in der Antwort");
        return Err(msg.to_string());
    };
    let open = nodes(data.get(ALIAS_OPEN)).iter().filter_map(parse_pr).collect();
    let review_requests = nodes(data.get(ALIAS_REVIEWS)).iter().filter_map(parse_review).collect();
    let mut merged = Vec::new();
    if let Some(obj) = data.as_object() {
        for (key, val) in obj {
            if !key.starts_with(ALIAS_MERGED_PREFIX) || key == ALIAS_MERGED_TODAY {
                continue;
            }
            for n in nodes(Some(val)) {
                let m = MergedPr {
                    repo: n.pointer("/repository/nameWithOwner").and_then(Value::as_str).unwrap_or("").to_string(),
                    head_ref: s(n, "headRefName"),
                    head_oid: s(n, "headRefOid"),
                    number: n.get("number").and_then(Value::as_u64).unwrap_or(0),
                    title: s(n, "title"),
                    url: s(n, "url"),
                    merged_at: n.get("mergedAt").and_then(Value::as_str).and_then(parse_iso).unwrap_or(0.0),
                };
                if !m.repo.is_empty() && !m.head_ref.is_empty() && !m.head_oid.is_empty() {
                    merged.push(m);
                }
            }
        }
    }
    Ok(PrSnapshot {
        open,
        review_requests,
        created_today: count(data.get(ALIAS_CREATED_TODAY)),
        merged_today: count(data.get(ALIAS_MERGED_TODAY)),
        merged,
        fetched_at: 0.0,
        error: None,
        viewer: data.pointer("/viewer/login").and_then(Value::as_str).unwrap_or("").to_string(),
    })
}

static CC_PREFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z]+(\([^)]*\))?!?:\s*").expect("CC_PREFIX"));

/// Titel ohne Conventional-Commit-Präfix, Anfang groß: `fix(ORPH-4255): run …` → `Run …`.
pub fn short_title(title: &str) -> String {
    let rest = CC_PREFIX.replace(title.trim(), "");
    let rest = if rest.trim().is_empty() { title.trim() } else { rest.trim() };
    let mut chars = rest.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Die Kürzel der Worktree-Ordner (`wt/4062-api`, Repos/CLAUDE.md): auf einer breiten
/// Karte ist für `management-api` kein Platz, „api“ kennt man von den Ordnern.
const REPO_SHORT: [(&str, &str); 6] = [
    ("management-api", "api"),
    ("management-ui", "ui"),
    ("client-electron", "client"),
    ("services-dotnet", "dotnet"),
    ("services-python", "python"),
    ("idm-orpheus-docs", "docs"),
];

/// Reponame kurz: ohne Organisation und ohne `orpheus-`, bekannte Repos mit ihrem
/// Worktree-Kürzel (`…/orpheus-management-api` → `api`).
pub fn short_repo(repo: &str) -> String {
    let name = repo.rsplit('/').next().unwrap_or(repo);
    let name = name.strip_prefix("orpheus-").unwrap_or(name);
    REPO_SHORT.iter().find(|(long, _)| *long == name).map_or(name, |(_, short)| *short).to_string()
}

#[cfg(test)]
#[path = "prs_tests.rs"]
mod tests;
