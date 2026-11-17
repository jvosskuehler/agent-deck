//! Hintergrund-Agenten: wann das Deck `claude -p` ohne Fenster für einen eigenen PR
//! startet, in welchem worktree und mit welchem Auftrag.
//!
//! Automatisch nur für Arbeit ohne menschliches Gegenüber: neue Funde von Bots (offene
//! Review-Threads, die ein Bot eröffnet hat), rote CI mit unbekannter Ursache
//! (`ci_cause`), ein neuer Merge-Konflikt. Auf Kommentare von Menschen nie von selbst;
//! dort startet nur der Knopf „Claude soll umsetzen“, und der Agent schreibt seine Antwort
//! als Entwurf in eine Datei, statt sie abzusenden.
//!
//! Grenzen: höchstens [`MAX_RUNNING`] zugleich, einer je PR, keiner in einem worktree, in
//! dem gerade eine Agenten-Session des Decks arbeitet, Laufzeit höchstens
//! [`TIME_LIMIT_S`]. Was vor `FeedState::since` geschah, löst nichts aus.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use super::ci_cause::{self, CiCause};
use super::feed_state::FeedState;
use super::pr_extra::Comment;
use super::prs::{short_repo, PrInfo};
use crate::i18n::Lang;

/// Höchstens so viele Hintergrund-Agenten zugleich.
pub const MAX_RUNNING: usize = 2;
/// Laufzeitgrenze eines Agenten (s).
pub const TIME_LIMIT_S: f64 = 30.0 * 60.0;

/// Der Auftrag eines Laufs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskKind {
    /// Merge-Konflikt lösen.
    Conflict,
    /// Rote CI untersuchen und beheben.
    CiRed,
    /// Funde von Bots bewerten, umsetzen, Threads beantworten und auflösen.
    BotFindings,
    /// Kommentar eines Menschen umsetzen, Antwort nur als Entwurf (nur per Knopf).
    HumanReply,
}

impl TaskKind {
    pub fn text(self, lang: Lang) -> &'static str {
        match self {
            TaskKind::Conflict => lang.l("Konflikt lösen", "resolve conflict"),
            TaskKind::CiRed => lang.l("rote CI untersuchen", "investigate red CI"),
            TaskKind::BotFindings => lang.l("Bot-Funde umsetzen", "address bot findings"),
            TaskKind::HumanReply => lang.l("Kommentar umsetzen", "address comment"),
        }
    }
}

/// Ein fälliger Auftrag.
#[derive(Debug, Clone, PartialEq)]
pub struct Due {
    pub pr: String,
    pub task: TaskKind,
    /// Was beim Start als bearbeitet gilt: Bot-Threads bis zu diesem Zeitpunkt, der
    /// Konflikt seit diesem Zeitpunkt (bei roter CI zählt der Commit).
    pub mark: f64,
    pub oid: String,
}

/// Die automatischen Aufträge eines PRs, höchstens einer (Konflikt vor roter CI vor
/// Bot-Funden: auf einem Konflikt sagt die CI nichts).
pub fn due_for(pr: &PrInfo, st: &FeedState, cause: Option<CiCause>) -> Option<Due> {
    // Auf Drafts startet nie ein Agent: ein Draft ist geparkt.
    if pr.draft {
        return None;
    }
    let key = pr.key();
    let done = st.handled.get(&key).cloned().unwrap_or_default();
    let x = &pr.extra;
    if let Some(&since) = st.conflict_since.get(&key)
        && x.conflicting
        && since > st.since
        && done.conflict < since
    {
        return Some(Due { pr: key, task: TaskKind::Conflict, mark: since, oid: String::new() });
    }
    let finished = ci_cause::finished_at(&x.checks);
    if cause == Some(CiCause::Unknown) && finished > st.since && !x.head_oid.is_empty() && done.ci_oid != x.head_oid {
        return Some(Due { pr: key, task: TaskKind::CiRed, mark: finished, oid: x.head_oid.clone() });
    }
    let newest_bot = x
        .threads
        .iter()
        .filter(|t| !t.resolved && t.by_bot())
        .filter_map(|t| t.head.as_ref().map(|c| c.at))
        .filter(|&at| at > st.since && at > done.bot)
        .fold(0.0, f64::max);
    (newest_bot > 0.0).then(|| Due { pr: key, task: TaskKind::BotFindings, mark: newest_bot, oid: String::new() })
}

/// Warum ein fälliger Auftrag noch nicht startet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hold {
    /// Eine Agenten-Session des Decks arbeitet im worktree dieses PRs.
    SessionActive,
    /// Schon [`MAX_RUNNING`] Agenten unterwegs.
    Full,
}

/// Was von `due` jetzt startet und was wartet. `busy(pr)`: arbeitet eine Session des
/// Decks im worktree dieses PRs?
pub fn schedule(due: Vec<Due>, st: &FeedState, busy: impl Fn(&str) -> bool) -> (Vec<Due>, Vec<(Due, Hold)>) {
    let mut running: Vec<String> = st.running().map(|r| r.pr.clone()).collect();
    let (mut start, mut wait) = (Vec::new(), Vec::new());
    for d in due {
        if running.contains(&d.pr) {
            continue;
        }
        if busy(&d.pr) {
            wait.push((d, Hold::SessionActive));
        } else if running.len() >= MAX_RUNNING {
            wait.push((d, Hold::Full));
        } else {
            running.push(d.pr.clone());
            start.push(d);
        }
    }
    (start, wait)
}

static TICKET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b([A-Z][A-Z0-9]+)-(\d+)\b").expect("TICKET"));

/// Ticket eines PRs (`ORPH-4258`) aus Titel oder Branch; leer = keins.
pub fn ticket_of(pr: &PrInfo) -> String {
    TICKET.find(&pr.title).or_else(|| TICKET.find(&pr.head_ref)).map(|m| m.as_str().to_string()).unwrap_or_default()
}

/// Ordnername des worktree nach der Konvention `wt/<nr>-<kürzel>` (Repos/CLAUDE.md);
/// ohne Ticket `pr<nummer>-<kürzel>`.
pub fn worktree_folder(pr: &PrInfo) -> String {
    let short = short_repo(&pr.repo);
    match TICKET.captures(&pr.title).or_else(|| TICKET.captures(&pr.head_ref)) {
        Some(c) => format!("{}-{short}", &c[2]),
        None => format!("pr{}-{short}", pr.number),
    }
}

const RULES: &str = "Rules: evaluate every finding yourself; implement what is valid, give a short reason for what is not. \
Commit messages in English, Conventional Commits style `type(TICKET): subject` (TICKET is given below), subject under 100 characters, \
never add a Co-Authored-By trailer. Push to the PR branch. Never merge, never close the PR, never force-push \
(only `git push --force-with-lease` to shorten a commit subject that is too long). Do not touch other branches or PRs. \
End with a short plain-text report (what you changed, which commits, what you left and why).";

/// Der Auftrag an `claude -p`. Enthält nur Adressen und Ids, keinen Kommentartext: den
/// liest der Agent selbst mit `gh`.
pub fn prompt(task: TaskKind, pr: &PrInfo, target: Option<&Comment>, draft_path: &str) -> String {
    let ticket = match ticket_of(pr) {
        t if t.is_empty() => "the ticket from the branch name".to_string(),
        t => t,
    };
    let head = format!(
        "You work headless for the PR {url} (repo {repo}, branch `{branch}`, ticket {ticket}) in this worktree. \
         Use `gh` for everything on GitHub.",
        url = pr.url,
        repo = pr.repo,
        branch = pr.head_ref,
    );
    let job = match task {
        TaskKind::Conflict => "The PR has a merge conflict with its base branch. Merge the base branch into the PR branch, \
             resolve the conflicts carefully (keep both sides' intent), run the relevant tests, commit and push."
            .to_string(),
        TaskKind::CiRed => "The CI of the latest commit is red for an unknown reason. Find the failing jobs (`gh pr checks`, \
             `gh run view --log-failed`), find the cause, fix it if it is caused by this PR, commit and push. \
             If the cause is outside this PR (flaky runner, infrastructure), do not change code; say so in the report."
            .to_string(),
        TaskKind::BotFindings => "Bots (Claude, Copilot, CodeRabbit, Sonar, github-code-quality) opened review threads that are \
             not resolved yet. For each unresolved bot thread: evaluate it, implement it if valid, commit and push. Then answer \
             every such thread in English in one or two sentences (name the commit, or the reason why not) and resolve it \
             (GraphQL `addPullRequestReviewThreadReply`, `resolveReviewThread`). If a human had requested changes that are \
             now implemented, re-request their review (`gh pr edit --add-reviewer`)."
            .to_string(),
        TaskKind::HumanReply => format!(
            "A human commented on this PR: {comment}. Read the comment and its context with `gh`. Evaluate it; implement what \
             is valid, commit and push. Do NOT post anything on GitHub and do NOT resolve any thread. Instead write the reply \
             you propose (English, one or two sentences, name the commit or the reason why not) as plain text into the file \
             `{draft_path}`, nothing else in that file.",
            comment = target.map_or(pr.url.as_str(), |c| c.url.as_str()),
        ),
    };
    format!("{head}\n\n{job}\n\n{RULES}")
}

/// Neue Bot-Threads seit `since`, je PR (für Tests und die Anzeige).
pub fn open_bot_threads(pr: &PrInfo, since: f64) -> usize {
    pr.extra.threads.iter().filter(|t| !t.resolved && t.by_bot() && t.head.as_ref().is_some_and(|c| c.at > since)).count()
}

/// Höchstlänge eines Berichts im Feed (Zeichen).
pub const REPORT_MAX: usize = 4000;

/// Ergebnis eines Laufs aus der Ausgabe von `claude -p --output-format stream-json`:
/// die letzte Zeile vom Typ `result` (`is_error`, `result`). `None`, wenn keine kam
/// (Prozess vorher beendet oder abgestürzt).
pub fn report_from_stream(text: &str) -> Option<(bool, String)> {
    text.lines().rev().find_map(|line| {
        let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
        (v.get("type")?.as_str()? == "result").then(|| {
            let err = v.get("is_error").and_then(serde_json::Value::as_bool).unwrap_or(false);
            let text = v.get("result").and_then(serde_json::Value::as_str).unwrap_or("").trim();
            (err, text.chars().take(REPORT_MAX).collect())
        })
    })
}

/// Ursachen je PR-Schlüssel (der Dienst rechnet sie mit den Log-Urteilen).
pub type Causes = BTreeMap<String, Option<CiCause>>;

#[cfg(test)]
#[path = "bg_tasks_tests.rs"]
mod tests;
