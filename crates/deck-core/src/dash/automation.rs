//! Die Mechanik ohne Claude: was das Deck an eigenen offenen PRs selbst erledigt, im Takt
//! der PR-Abfrage. Reviewer (aus der Einstellung `pr_reviewer`) und mich als Assignee
//! nachtragen, je einmal; mergen, wenn nichts mehr fehlt. Ausgeführt wird in
//! `deck-services/src/dash/feed_service.rs` hinter dem Trait `GhWrite`.
//!
//! Mergen nur, wenn ALLES stimmt (sonst sagt [`merge_wait`], worauf es wartet):
//! kein Draft, nicht auf der Halteliste, `mergeStateStatus` CLEAN (UNSTABLE nur, wenn die
//! rote CI nach `ci_cause` bloß an Code Security liegt), CI grün, keine offenen
//! Review-Threads, kein offenes „Änderungen gewünscht“ eines Menschen, eine Freigabe
//! eines Menschen, kein Status `merge-order` außer `success`, keine offene Abhängigkeit
//! (`Merge after: <repo>#<n>` bzw. „after #N“ in der Beschreibung, laut GitHub gemergt,
//! `merge_order.rs`; oder ein offener Backend-PR zum selben Ticket bei einem UI-PR). Gemergt wird mit
//! `--merge`, nie `--auto`; danach wird der Remote-Branch gelöscht.

use super::bg_tasks::ticket_of;
use super::ci_cause::CiCause;
use super::merge_order;
use super::feed_state::FeedState;
use super::prs::{CiState, PrInfo};
use crate::i18n::Lang;

/// Eine Wirkung nach außen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    AddReviewer { pr: String, url: String, login: String },
    AddAssignee { pr: String, url: String },
    Merge { pr: String, url: String, repo: String, head_ref: String },
}

/// Worauf ein Merge wartet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeWait {
    Off,
    Draft,
    Held,
    Conflict,
    CiRunning,
    CiRed,
    /// Rot, aber erklärt (Review-Gate): braucht einen Menschen.
    NeedsHuman,
    OpenThreads(u32),
    ChangesRequested,
    NoApproval,
    /// Claude stuft den PR als `high` ein: eine Freigabe von Claude reicht nicht.
    HighNeedsHuman,
    /// Wartet auf einen anderen offenen PR (Nummer im selben Repo bzw. Backend-PR).
    DependsOn(String),
    /// `mergeStateStatus` noch nicht CLEAN (BEHIND, BLOCKED, UNKNOWN …).
    State(String),
    /// Der letzte Versuch an diesem Commit scheiterte.
    Failed,
    /// Der Commit-Status `merge-order` ist da, aber nicht `success`.
    MergeOrder(String),
}

impl MergeWait {
    pub fn text(&self, lang: Lang) -> String {
        match self {
            MergeWait::Off => lang.l("Automatik aus", "automation off").into(),
            MergeWait::Draft => "Draft".into(),
            MergeWait::Held => lang.l("gehalten, wird nicht gemergt", "on hold, not merged").into(),
            MergeWait::Conflict => lang.l("Konflikt", "conflict").into(),
            MergeWait::CiRunning => lang.l("CI läuft", "CI running").into(),
            MergeWait::CiRed => lang.l("CI rot", "CI red").into(),
            MergeWait::NeedsHuman => lang.l("braucht menschliche Freigabe", "needs a human approval").into(),
            MergeWait::OpenThreads(n) => format!("{n} {}", lang.l("Threads offen", "open threads")),
            MergeWait::ChangesRequested => lang.l("Änderungen gewünscht", "changes requested").into(),
            MergeWait::NoApproval => lang.l("wartet auf Freigabe am aktuellen Stand", "awaiting approval of the current head").into(),
            MergeWait::HighNeedsHuman => lang.l("high: wartet auf Freigabe eines Menschen", "high: awaiting a human approval").into(),
            MergeWait::DependsOn(what) => format!("{} {what}", lang.l("wartet auf", "waits for")),
            MergeWait::State(s) => format!("{} {}", lang.l("noch nicht mergebar:", "not mergeable yet:"), s.to_lowercase()),
            MergeWait::MergeOrder(s) => format!("{} merge-order ({s})", lang.l("wartet auf", "waits for")),
            MergeWait::Failed => lang.l("Merge gescheitert, wartet auf neuen Commit", "merge failed, waits for a new commit").into(),
        }
    }
}

fn repo_name(repo: &str) -> &str {
    repo.rsplit('/').next().unwrap_or(repo)
}

fn is_frontend(repo: &str) -> bool {
    repo_name(repo).ends_with("-ui")
}

fn is_backend(repo: &str) -> bool {
    let n = repo_name(repo);
    n.ends_with("-api") || n.contains("services")
}

/// Wartet `pr` auf einen anderen PR? Jede Abhängigkeit aus der Beschreibung
/// (`merge_order::dependencies`) muss laut GitHub gemergt sein (`FeedState::deps`, vom
/// Dienst abgefragt; unbekannt = warten). `open`: alle eigenen offenen PRs.
fn dependency(pr: &PrInfo, open: &[PrInfo], st: &FeedState) -> Option<String> {
    let deps = merge_order::dependencies(&pr.extra.body, &pr.repo);
    if let Some(k) = merge_order::first_unmerged(&deps, &st.deps) {
        return Some(merge_order::dep_label(k));
    }
    let ticket = ticket_of(pr);
    if is_frontend(&pr.repo) && !ticket.is_empty() {
        return open.iter().find(|o| is_backend(&o.repo) && ticket_of(o) == ticket).map(PrInfo::label);
    }
    None
}

/// Worauf der Merge von `pr` wartet; `None` = jetzt mergen.
pub fn merge_wait(pr: &PrInfo, open: &[PrInfo], cause: Option<CiCause>, st: &FeedState) -> Option<MergeWait> {
    let x = &pr.extra;
    let key = pr.key();
    let wait = if !st.automation {
        MergeWait::Off
    } else if pr.draft {
        MergeWait::Draft
    } else if st.hold.contains(&key) {
        MergeWait::Held
    } else if x.conflicting {
        MergeWait::Conflict
    } else if let Some(c) = x.checks.iter().find(|c| c.name == merge_order::MERGE_ORDER).filter(|c| c.failed || c.running) {
        MergeWait::MergeOrder(if c.failed { "failure" } else { "pending" }.into())
    } else if x.checks.iter().any(|c| c.running && c.name != merge_order::MERGE_ORDER) || (pr.ci == CiState::Running && x.checks.is_empty()) {
        MergeWait::CiRunning
    } else if cause == Some(CiCause::ReviewGate) {
        MergeWait::NeedsHuman
    } else if cause.is_some_and(|c| !c.counts_as_green()) {
        MergeWait::CiRed
    } else if cause.is_none() && x.checks.is_empty() && pr.ci != CiState::Success {
        // Ohne einzelne Checks zählt der Gesamtstand; mit Checks zählt nur ihr neuester Lauf.
        MergeWait::CiRunning
    } else if pr.open_threads > 0 {
        MergeWait::OpenThreads(pr.open_threads)
    } else if !x.changes_requested_by().is_empty() {
        MergeWait::ChangesRequested
    } else if !x.approved_current() {
        MergeWait::NoApproval
    } else if x.claude_high() && !x.approved_by_human() {
        MergeWait::HighNeedsHuman
    } else if let Some(d) = dependency(pr, open, st) {
        MergeWait::DependsOn(d)
    } else if !(x.merge_state == "CLEAN" || (x.merge_state == "UNSTABLE" && (cause == Some(CiCause::CodeSecurity) || (cause.is_none() && !x.checks.is_empty())))) {
        MergeWait::State(if x.merge_state.is_empty() { "UNKNOWN".into() } else { x.merge_state.clone() })
    } else if st.handled.get(&key).is_some_and(|h| !h.merge_failed_oid.is_empty() && h.merge_failed_oid == x.head_oid) {
        MergeWait::Failed
    } else {
        return None;
    };
    Some(wait)
}

/// `pr_reviewer` als Liste: Komma oder Leerraum trennt, leere fallen weg, doppelte
/// (ohne Groß/klein) zählen einmal, ein führendes `@` fällt weg.
pub fn reviewer_list(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for login in raw.split(|c: char| c == ',' || c == ';' || c.is_whitespace()) {
        let login = login.trim().trim_start_matches('@');
        if !login.is_empty() && !out.iter().any(|o| o.eq_ignore_ascii_case(login)) {
            out.push(login.to_string());
        }
    }
    out
}

/// Was in dieser Runde zu tun ist. `reviewers`: Logins aus der Einstellung (leer = keiner;
/// mehrere kommen in einem Aufruf, `login` ist dann `a,b`), `me`: mein Login. Bei
/// abgeschalteter Automatik nichts.
pub fn plan(open: &[PrInfo], st: &FeedState, cause: impl Fn(&PrInfo) -> Option<CiCause>, reviewers: &[String], me: &str) -> Vec<Act> {
    if !st.automation {
        return Vec::new();
    }
    let mut acts = Vec::new();
    // Drafts fasst die Automatik nie an: kein Reviewer, kein Assignee, kein Merge.
    for pr in open.iter().filter(|p| !p.draft) {
        let key = pr.key();
        let done = st.handled.get(&key).cloned().unwrap_or_default();
        let x = &pr.extra;
        let has = |l: &str| pr.waiting_on.iter().any(|w| w.eq_ignore_ascii_case(l)) || x.reviewed_by().iter().any(|r| r.eq_ignore_ascii_case(l));
        let missing: Vec<&str> = reviewers.iter().map(String::as_str).filter(|r| !r.eq_ignore_ascii_case(me) && !has(r)).collect();
        if !missing.is_empty() && !done.reviewer {
            acts.push(Act::AddReviewer { pr: key.clone(), url: pr.url.clone(), login: missing.join(",") });
        }
        if !me.is_empty() && !done.assignee && !x.assignees.iter().any(|a| a.eq_ignore_ascii_case(me)) {
            acts.push(Act::AddAssignee { pr: key.clone(), url: pr.url.clone() });
        }
        if merge_wait(pr, open, cause(pr), st).is_none() {
            acts.push(Act::Merge { pr: key, url: pr.url.clone(), repo: pr.repo.clone(), head_ref: pr.head_ref.clone() });
        }
    }
    acts
}

#[cfg(test)]
#[path = "automation_tests.rs"]
mod tests;
