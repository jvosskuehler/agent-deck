//! Eine Runde des Feed-Dienstes (`feed_service.rs`): neuen Stand einarbeiten, Logs roter
//! Checks holen, die Mechanik ausführen (nachtragen, mergen, Branch löschen), Jira
//! nachziehen, dann die Hintergrund-Agenten (`feed_agents.rs`).
//!
//! Ins Protokoll kommen nur PR-Kürzel, Tickets, Ids, Anzahlen, Dauern und kurze
//! Fehlermeldungen, nie Kommentartexte.

use std::collections::BTreeMap;

use deck_core::dash::automation::{self, Act};
use deck_core::dash::ci_cause;
use deck_core::dash::feed_state::{Activity, Did};
use deck_core::dash::jira_sync;
use deck_core::dash::merge_order;
use deck_core::dash::prs::{pr_key, short_repo, PrSnapshot};
use serde_json::Value;

use super::feed_service::{lock, Shared};

/// Höchstens so viele Job-Logs je Runde.
const LOGS_PER_ROUND: usize = 6;

/// Ein Eintrag der Zeitleiste zu einem PR.
pub(crate) fn activity(snap: &PrSnapshot, key: &str, did: Did, run: &str, now: f64) -> Activity {
    let pr = snap.open.iter().find(|p| p.key() == key);
    let merged = snap.merged.iter().find(|m| pr_key(&m.repo, m.number) == key);
    let (label, url, title) = match (pr, merged) {
        (Some(p), _) => (p.label(), p.url.clone(), p.title.clone()),
        (None, Some(m)) => (format!("{}#{}", short_repo(&m.repo), m.number), m.url.clone(), m.title.clone()),
        (None, None) => (key.rsplit('/').next().unwrap_or(key).to_string(), String::new(), String::new()),
    };
    Activity { at: now, pr: key.to_string(), label, url, title, did, run: run.to_string() }
}

pub(crate) fn round(sh: &Shared) {
    let now = (sh.deps.now)();
    let snap = (sh.deps.prs)();
    let fresh = {
        let mut i = lock(&sh.inner);
        let fresh = snap.fetched_at > 0.0 && snap.fetched_at != i.seen_fetch;
        if fresh {
            i.seen_fetch = snap.fetched_at;
            if i.st.observe(&snap, now) {
                i.dirty = true;
            }
        }
        fresh
    };
    if fresh {
        fetch_logs(sh, &snap);
        fetch_deps(sh, &snap);
        update_causes(sh, &snap);
        let acts = {
            let i = lock(&sh.inner);
            let causes = i.causes.clone();
            automation::plan(&snap.open, &i.st, |p| causes.get(&p.key()).copied().flatten(), &i.cfg.reviewers, &snap.viewer)
        };
        run_acts(sh, &snap, acts, now);
        run_jira(sh, &snap, now);
    }
    super::feed_agents::watch(sh, &snap, now);
    super::feed_agents::start_due(sh, &snap, now);
    super::feed_agents::refresh_drafts(sh);
    sh.save();
    sh.notify();
}

/// Logs der roten Checks holen, die an Code Security hängen könnten (je Job einmal).
fn fetch_logs(sh: &Shared, snap: &PrSnapshot) {
    let todo: Vec<(String, u64)> = {
        let i = lock(&sh.inner);
        snap.open
            .iter()
            .flat_map(|p| p.extra.checks.iter().map(move |c| (p, c)))
            .filter(|(_, c)| ci_cause::log_needed(c) && !i.st.verdicts.contains_key(&c.job_id))
            .map(|(p, c)| (p.repo.clone(), c.job_id))
            .take(LOGS_PER_ROUND)
            .collect()
    };
    for (repo, job) in todo {
        let verdict = match sh.deps.gh.job_log(&repo, job) {
            Ok(text) => ci_cause::log_explains(&text),
            Err(e) => {
                sh.log.note(&format!("Job-Log {job} nicht geholt: {e}"));
                false
            }
        };
        let mut i = lock(&sh.inner);
        i.st.verdicts.insert(job, verdict);
        i.dirty = true;
    }
}

/// Zustand der PRs, nach denen eigene PRs kommen sollen (`Merge after:`), selbst bei
/// GitHub nachfragen: ein Merge verlässt sich nie nur auf den Status `merge-order`.
/// Scheitert die Abfrage, bleibt alles unbekannt, und die Automatik wartet.
fn fetch_deps(sh: &Shared, snap: &PrSnapshot) {
    let mut keys: Vec<String> = snap.open.iter().flat_map(|p| merge_order::dependencies(&p.extra.body, &p.repo)).collect();
    keys.sort();
    keys.dedup();
    let states = match merge_order::deps_query(&keys) {
        None => Default::default(),
        Some(q) => match sh.deps.gh.graphql(&q).and_then(|t| serde_json::from_str::<Value>(&t).map_err(|_| "Antwort kein JSON".to_string())) {
            Ok(v) => merge_order::parse_deps(&v, &keys),
            Err(e) => {
                sh.log.note(&format!("Abhängigkeiten ({}) nicht abgefragt: {e}", keys.len()));
                Default::default()
            }
        },
    };
    lock(&sh.inner).st.deps = states;
}

fn update_causes(sh: &Shared, snap: &PrSnapshot) {
    let mut i = lock(&sh.inner);
    let causes: BTreeMap<_, _> = snap.open.iter().map(|p| (p.key(), ci_cause::cause(&p.extra.checks, &p.extra.files, &i.st.verdicts))).collect();
    i.causes = causes;
}

fn run_acts(sh: &Shared, snap: &PrSnapshot, acts: Vec<Act>, now: f64) {
    for act in acts {
        let (key, did, result) = match &act {
            Act::AddReviewer { pr, url, login } => (pr.clone(), Did::ReviewerAdded(login.clone()), sh.deps.write.add_reviewer(url, login)),
            Act::AddAssignee { pr, url } => (pr.clone(), Did::AssigneeAdded, sh.deps.write.add_assignee_me(url)),
            Act::Merge { pr, url, .. } => (pr.clone(), Did::Merged, sh.deps.write.merge(url)),
        };
        let label = activity(snap, &key, Did::Merged, "", now).label;
        let mut i = lock(&sh.inner);
        i.dirty = true;
        match (&act, result) {
            (Act::AddReviewer { .. }, r) => {
                i.st.handled_mut(&key).reviewer = true;
                log_result(sh, &label, "Reviewer nachgetragen", &r);
                if r.is_ok() {
                    i.st.record(activity(snap, &key, did, "", now));
                }
            }
            (Act::AddAssignee { .. }, r) => {
                i.st.handled_mut(&key).assignee = true;
                log_result(sh, &label, "Assignee nachgetragen", &r);
                if r.is_ok() {
                    i.st.record(activity(snap, &key, did, "", now));
                }
            }
            (Act::Merge { repo, head_ref, .. }, Ok(())) => {
                sh.log.note(&format!("{label}: gemergt"));
                i.st.record(activity(snap, &key, Did::Merged, "", now));
                drop(i);
                let deleted = sh.deps.write.delete_branch(repo, head_ref);
                log_result(sh, &label, "Remote-Branch gelöscht", &deleted);
                let mut i = lock(&sh.inner);
                if deleted.is_ok() {
                    i.st.record(activity(snap, &key, Did::BranchDeleted, "", now));
                }
                i.to_clean.push((repo.clone(), head_ref.clone(), now));
            }
            (Act::Merge { .. }, Err(e)) => {
                sh.log.note(&format!("{label}: Merge gescheitert: {e}"));
                let oid = snap.open.iter().find(|p| p.key() == key).map(|p| p.extra.head_oid.clone()).unwrap_or_default();
                i.st.handled_mut(&key).merge_failed_oid = oid;
                i.st.record(activity(snap, &key, Did::MergeFailed(e), "", now));
            }
        }
    }
}

fn log_result(sh: &Shared, label: &str, what: &str, r: &Result<(), String>) {
    match r {
        Ok(()) => sh.log.note(&format!("{label}: {what}")),
        Err(e) => sh.log.note(&format!("{label}: {what} gescheitert: {e}")),
    }
}

/// Jira nachziehen für Tickets frisch gemergter PRs.
fn run_jira(sh: &Shared, snap: &PrSnapshot, now: f64) {
    let (due, org, prefix) = {
        let i = lock(&sh.inner);
        if !i.st.automation {
            return;
        }
        (jira_sync::due(snap, &i.st, &i.cfg.prefix, now), i.cfg.org.clone(), i.cfg.prefix.clone())
    };
    for (key, ticket) in due {
        let outcome = jira_one(sh, &org, &prefix, &ticket);
        let mut i = lock(&sh.inner);
        i.st.jira_checked.insert(key.clone(), now);
        i.dirty = true;
        match outcome {
            Ok(Some(status)) => {
                sh.log.note(&format!("Jira {ticket} auf {status}"));
                i.st.record(activity(snap, &key, Did::JiraMoved(ticket, status.to_string()), "", now));
            }
            Ok(None) => {}
            Err(e) => {
                sh.log.note(&format!("Jira {ticket} nicht nachgezogen: {e}"));
                i.st.record(activity(snap, &key, Did::JiraFailed(ticket, e), "", now));
            }
        }
    }
}

fn jira_one(sh: &Shared, org: &str, prefix: &str, ticket: &str) -> Result<Option<&'static str>, String> {
    if org.is_empty() {
        return Err("Einstellung github_org fehlt".into());
    }
    let text = sh.deps.gh.graphql(&jira_sync::search_query(org, ticket))?;
    let v: Value = serde_json::from_str(&text).map_err(|_| "Antwort kein JSON".to_string())?;
    let prs = jira_sync::parse_search(&v, ticket, prefix)?;
    let current = sh.deps.jira.status(ticket)?;
    let Some(target) = jira_sync::target(&prs, &current) else { return Ok(None) };
    sh.deps.jira.transition(ticket, target)?;
    Ok(Some(target))
}
