//! Was der Feed und die Automatik über einen eigenen offenen PR zusätzlich wissen müssen:
//! Kommentare und Review-Threads (wer hat zuletzt gesprochen, Mensch oder Bot), die
//! Reviews, die einzelnen Checks am letzten Commit, geänderte Dateien, Beschreibung,
//! Assignees und `mergeStateStatus`.
//!
//! Gelesen aus demselben GraphQL-Aufruf wie `prs.rs` (Felder in
//! `deck-services/src/dash/github.rs`), tolerant: was fehlt, ist leer.

use serde_json::Value;

use crate::isotime::parse_iso;

use super::prs::is_bot;

/// Alias der Detail-Abfrage (`deck-services/src/dash/github.rs`, `detail_query`).
pub const ALIAS_DETAIL: &str = "detail";

/// Die Antwort der Detail-Abfrage an die offenen PRs hängen (über Repo und Nummer).
/// Ohne `data` ist das ein Fehler (erste GraphQL-Meldung).
pub fn attach(open: &mut [super::prs::PrInfo], v: &Value) -> Result<(), String> {
    let Some(list) = v.pointer(&format!("/data/{ALIAS_DETAIL}/nodes")).and_then(Value::as_array) else {
        return Err(v.pointer("/errors/0/message").and_then(Value::as_str).unwrap_or("keine Details in der Antwort").to_string());
    };
    for n in list {
        let repo = n.pointer("/repository/nameWithOwner").and_then(Value::as_str).unwrap_or("");
        let number = n.get("number").and_then(Value::as_u64).unwrap_or(0);
        if let Some(pr) = open.iter_mut().find(|p| p.repo == repo && p.number == number) {
            pr.extra = parse(n);
        }
    }
    Ok(())
}

/// Ein Kommentar (PR-Kommentar, Thread-Kommentar oder Review mit Text).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Comment {
    /// GraphQL-Knoten-Id.
    pub id: String,
    pub author: String,
    pub bot: bool,
    /// Unix-Sekunden.
    pub at: f64,
    pub text: String,
    pub url: String,
}

/// Ein Review-Thread. `head` ist der erste Kommentar (wer den Fund gemacht hat), `tail`
/// die letzten (wer zuletzt gesprochen hat).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Thread {
    pub id: String,
    pub resolved: bool,
    pub head: Option<Comment>,
    pub tail: Vec<Comment>,
}

impl Thread {
    /// Der letzte Kommentar im Thread.
    pub fn last(&self) -> Option<&Comment> {
        self.tail.last().or(self.head.as_ref())
    }

    /// Ein Bot hat den Thread eröffnet (Fund von Claude, Copilot, CodeRabbit …).
    pub fn by_bot(&self) -> bool {
        self.head.as_ref().is_some_and(|c| c.bot)
    }
}

/// Ein abgegebenes Review.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Review {
    pub comment: Comment,
    /// `APPROVED`, `CHANGES_REQUESTED`, `COMMENTED`, `DISMISSED`.
    pub state: String,
    /// Commit, auf den sich das Review bezieht; leer = unbekannt.
    pub oid: String,
    /// REST-Id (`databaseId`), nötig zum Verwerfen (`stale_review`); 0 = unbekannt.
    pub db_id: u64,
    /// Klasse aus dem Marker `<!-- claude-classification class=… -->` im Review-Text
    /// (nur Claude setzt ihn); leer = keiner.
    pub classification: String,
}

/// Ist das Konto Claude (Review-Bot `claude[bot]`, in GraphQL `claude`)?
pub fn is_claude(login: &str) -> bool {
    let l = login.to_ascii_lowercase();
    l == "claude" || l.starts_with("claude[bot]")
}

/// `class=high` aus `<!-- claude-classification class=high risk=high -->`.
pub fn classification_of(body: &str) -> String {
    let Some(i) = body.find("claude-classification") else { return String::new() };
    let rest = &body[i..body.len().min(i + 120)];
    rest.split_whitespace()
        .find_map(|w| w.strip_prefix("class="))
        .map(|c| c.trim_end_matches(|ch: char| !ch.is_ascii_alphanumeric()).to_ascii_lowercase())
        .unwrap_or_default()
}

/// Ein Check am letzten Commit.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Check {
    /// `codeql / Analyze (actions)`, `claude / Review`.
    pub name: String,
    /// Name des Workflows (`CodeQL`, `CI`); leer bei Status-Kontexten.
    pub workflow: String,
    /// Job-Id von Actions (`databaseId`), 0 = keine (Status-Kontext).
    pub job_id: u64,
    pub failed: bool,
    pub running: bool,
    /// Unix-Sekunden des Abschlusses; 0 = unbekannt.
    pub done_at: f64,
}

/// Die Zusatzdaten eines offenen PRs.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PrExtra {
    pub body: String,
    pub files: Vec<String>,
    /// `CLEAN`, `UNSTABLE`, `BLOCKED`, `BEHIND`, `DIRTY`, `UNKNOWN` …
    pub merge_state: String,
    pub conflicting: bool,
    pub assignees: Vec<String>,
    pub head_oid: String,
    /// Unix-Sekunden des letzten Commits.
    pub last_commit_at: f64,
    /// Kommentare unter dem PR (nicht in Threads).
    pub comments: Vec<Comment>,
    pub threads: Vec<Thread>,
    pub reviews: Vec<Review>,
    pub checks: Vec<Check>,
    /// Wer den PR wann auf Draft bzw. bereit fürs Review gestellt hat.
    pub events: Vec<(String, f64)>,
}

fn s(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

fn nodes<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key).and_then(|x| x.get("nodes")).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

fn time(v: &Value, key: &str) -> f64 {
    v.get(key).and_then(Value::as_str).and_then(parse_iso).unwrap_or(0.0)
}

/// Ein Kommentar-Knoten (`author{login __typename}`). Ein Konto vom Typ `Bot` ist immer
/// ein Bot, dazu die bekannten Bot-Konten ohne diesen Typ ([`is_bot`]).
pub fn comment(v: &Value, time_key: &str) -> Option<Comment> {
    let author = v.pointer("/author/login").and_then(Value::as_str)?.to_string();
    let bot = v.pointer("/author/__typename").and_then(Value::as_str) == Some("Bot") || is_bot(&author);
    Some(Comment { id: s(v, "id"), bot, at: time(v, time_key), text: s(v, "bodyText"), url: s(v, "url"), author })
}

fn check(v: &Value) -> Option<Check> {
    match v.get("__typename").and_then(Value::as_str) {
        Some("CheckRun") => {
            let conclusion = v.get("conclusion").and_then(Value::as_str);
            Some(Check {
                name: s(v, "name"),
                workflow: v.pointer("/checkSuite/workflowRun/workflow/name").and_then(Value::as_str).unwrap_or("").to_string(),
                job_id: v.get("databaseId").and_then(Value::as_u64).unwrap_or(0),
                failed: matches!(conclusion, Some("FAILURE" | "TIMED_OUT" | "STARTUP_FAILURE" | "ACTION_REQUIRED")),
                running: conclusion.is_none(),
                done_at: time(v, "completedAt"),
            })
        }
        Some("StatusContext") => {
            let state = v.get("state").and_then(Value::as_str);
            Some(Check {
                name: s(v, "context"),
                failed: matches!(state, Some("FAILURE" | "ERROR")),
                running: matches!(state, Some("PENDING" | "EXPECTED")),
                done_at: time(v, "createdAt"),
                ..Check::default()
            })
        }
        _ => None,
    }
}

/// Die Zusatzdaten aus einem PR-Knoten der Suche.
pub fn parse(n: &Value) -> PrExtra {
    let last = nodes(n, "commits").last().and_then(|c| c.get("commit"));
    let threads = nodes(n, "reviewThreads")
        .iter()
        .map(|t| Thread {
            id: s(t, "id"),
            resolved: t.get("isResolved").and_then(Value::as_bool).unwrap_or(false),
            head: nodes(t, "head").first().and_then(|c| comment(c, "createdAt")),
            tail: nodes(t, "tail").iter().filter_map(|c| comment(c, "createdAt")).collect(),
        })
        .collect();
    let reviews = nodes(n, "timelineItems")
        .iter()
        .filter(|e| e.get("submittedAt").is_some())
        .filter_map(|e| {
            Some(Review {
                comment: comment(e, "submittedAt")?,
                state: s(e, "state"),
                oid: e.pointer("/commit/oid").and_then(Value::as_str).unwrap_or("").to_string(),
                db_id: e.get("databaseId").and_then(Value::as_u64).unwrap_or(0),
                classification: classification_of(e.get("raw").and_then(Value::as_str).unwrap_or("")),
            })
        })
        .collect();
    // „Auf Draft gestellt“ und „bereit fürs Review“: wer das tut, hat reagiert.
    let events = nodes(n, "timelineItems")
        .iter()
        .filter(|e| e.get("submittedAt").is_none())
        .filter_map(|e| Some((e.pointer("/actor/login")?.as_str()?.to_string(), time(e, "createdAt"))))
        .collect();
    PrExtra {
        body: s(n, "body"),
        files: nodes(n, "files").iter().map(|f| s(f, "path")).filter(|p| !p.is_empty()).collect(),
        merge_state: s(n, "mergeStateStatus"),
        conflicting: n.get("mergeable").and_then(Value::as_str) == Some("CONFLICTING"),
        assignees: nodes(n, "assignees").iter().map(|a| s(a, "login")).filter(|l| !l.is_empty()).collect(),
        head_oid: last.map(|c| s(c, "oid")).unwrap_or_default(),
        last_commit_at: last.map_or(0.0, |c| time(c, "committedDate")),
        comments: nodes(n, "comments").iter().filter_map(|c| comment(c, "createdAt")).collect(),
        threads,
        reviews,
        checks: latest_checks(
            last.and_then(|c| c.pointer("/statusCheckRollup/contexts/nodes"))
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(check).collect())
                .unwrap_or_default(),
        ),
        events,
    }
}

/// Je Check (Workflow und Name) nur der neueste Lauf: ein alter roter Lauf am selben
/// Commit (etwa vor einem Branch-Umzug) ist überholt, wenn derselbe Check danach lief.
fn latest_checks(all: Vec<Check>) -> Vec<Check> {
    let mut out: Vec<Check> = Vec::new();
    for c in all {
        match out.iter_mut().find(|o| o.name == c.name && o.workflow == c.workflow) {
            Some(o) if c.running || (!o.running && c.done_at > o.done_at) => *o = c,
            Some(_) => {}
            None => out.push(c),
        }
    }
    out
}

impl PrExtra {
    /// Wann ich zuletzt etwas getan habe: eigener Kommentar, eigenes Review, eigene
    /// Antwort im Thread oder der letzte Commit (Push).
    pub fn my_last_activity(&self, me: &str) -> f64 {
        let mine = |c: &Comment| (!me.is_empty() && c.author.eq_ignore_ascii_case(me)).then_some(c.at);
        let threads = self.threads.iter().flat_map(|t| t.head.iter().chain(t.tail.iter()));
        self.comments
            .iter()
            .chain(self.reviews.iter().map(|r| &r.comment))
            .chain(threads)
            .filter_map(mine)
            .chain(self.events.iter().filter(|(who, _)| !me.is_empty() && who.eq_ignore_ascii_case(me)).map(|(_, at)| *at))
            .fold(self.last_commit_at, f64::max)
    }

    /// Freigabe von Claude oder einem Menschen auf dem aktuellen Kopf (Review am
    /// Kopf-Commit; ohne Commit-Angabe: nicht älter als der letzte Commit).
    pub fn approved_current(&self) -> bool {
        self.reviews.iter().any(|r| {
            r.state == "APPROVED"
                && (!r.comment.bot || is_claude(&r.comment.author))
                && if r.oid.is_empty() { r.comment.at >= self.last_commit_at } else { r.oid == self.head_oid }
        })
    }

    /// Hat Claude den PR zuletzt als `high` eingestuft? Dann braucht er einen Menschen.
    pub fn claude_high(&self) -> bool {
        self.reviews
            .iter()
            .filter(|r| is_claude(&r.comment.author) && !r.classification.is_empty())
            .max_by(|a, b| a.comment.at.total_cmp(&b.comment.at))
            .is_some_and(|r| r.classification == "high")
    }

    /// Menschen, deren letztes Review „Änderungen gewünscht“ lautet.
    pub fn changes_requested_by(&self) -> Vec<String> {
        let mut last: Vec<(&str, &Review)> = Vec::new();
        for r in self.reviews.iter().filter(|r| !r.comment.bot && r.state != "COMMENTED") {
            match last.iter_mut().find(|(a, _)| *a == r.comment.author) {
                Some(slot) if slot.1.comment.at <= r.comment.at => slot.1 = r,
                Some(_) => {}
                None => last.push((&r.comment.author, r)),
            }
        }
        last.into_iter().filter(|(_, r)| r.state == "CHANGES_REQUESTED").map(|(a, _)| a.to_string()).collect()
    }

    /// Hat ein Mensch freigegeben (sein letztes Urteil)?
    pub fn approved_by_human(&self) -> bool {
        self.reviews.iter().any(|r| !r.comment.bot && r.state == "APPROVED") && self.changes_requested_by().is_empty()
    }

    /// Wer schon ein Review abgegeben hat (Menschen).
    pub fn reviewed_by(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.reviews.iter().filter(|r| !r.comment.bot).map(|r| r.comment.author.as_str()).collect();
        v.dedup();
        v
    }
}

#[cfg(test)]
#[path = "pr_extra_tests.rs"]
mod tests;
