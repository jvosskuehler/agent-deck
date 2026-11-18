//! Die Zeitleiste „Was passiert ist“ der letzten 24 Stunden: aus GitHub (gemergt, CI fertig
//! geworden, Review eines Menschen, Konflikt entstanden) und aus dem, was das Deck selbst
//! getan hat (`FeedState::activity`: gemergt, nachgetragen, Agent gestartet/fertig,
//! Entwurf bereit, Jira nachgezogen).
//!
//! Die CI kennt nur den Stand am letzten Commit: „CI rot geworden“ datiert auf den
//! Abschluss des letzten Checks. Ein Merge, den das Deck selbst gemacht hat, steht nur
//! einmal da (als Eintrag des Decks).

use super::ci_cause::{finished_at, CiCause};
use super::feed::FeedInput;
use super::feed_state::{Activity, Did};
use super::feed_view::{FeedEvent, FeedKind};
use super::prs::{pr_key, short_repo, short_title, CiState};
use crate::i18n::Lang;

/// Zeitraum der Zeitleiste (s).
pub const WINDOW_S: f64 = 24.0 * 3600.0;
/// Höchstens so viele Einträge.
const MAX: usize = 80;

fn ev(at: f64, kind: FeedKind, label: String, url: String, text: String, detail: String) -> FeedEvent {
    FeedEvent { at, kind, label, url, text, detail, report: String::new() }
}

fn activity(a: &Activity, i: &FeedInput) -> FeedEvent {
    let lang = i.lang;
    let l = |de: &str, en: &str| lang.l(de, en).to_string();
    let (kind, text, detail) = match &a.did {
        Did::Merged => (FeedKind::Merged, l("gemergt von der Automatik", "merged by the automation"), short_title(&a.title)),
        Did::MergeFailed(why) => (FeedKind::Problem, l("Merge gescheitert", "merge failed"), why.clone()),
        Did::BranchDeleted => (FeedKind::Minor, l("Branch gelöscht", "branch deleted"), String::new()),
        Did::ReviewerAdded(who) => (FeedKind::Minor, format!("{who} {}", lang.l("als Reviewer nachgetragen", "added as reviewer")), String::new()),
        Did::AssigneeAdded => (FeedKind::Minor, l("dich als Assignee nachgetragen", "assigned to you"), String::new()),
        Did::WorktreeRemoved(folder) => (FeedKind::Minor, format!("{} wt/{folder} {}", lang.l("Worktree", "worktree"), lang.l("entfernt", "removed")), String::new()),
        Did::AgentStarted(t) => (FeedKind::Agent, format!("{}: {}", lang.l("Claude gestartet", "Claude started"), t.text(lang)), String::new()),
        Did::AgentDone(t) => (FeedKind::Agent, format!("{}: {}", lang.l("Claude fertig", "Claude done"), t.text(lang)), String::new()),
        Did::AgentFailed(t, why) => (FeedKind::Problem, format!("{}: {}", lang.l("Claude gescheitert", "Claude failed"), t.text(lang)), why.clone()),
        Did::AgentAborted(t) => (FeedKind::Minor, format!("{}: {}", lang.l("Claude abgebrochen", "Claude aborted"), t.text(lang)), String::new()),
        Did::AgentTimedOut(t) => (FeedKind::Problem, format!("{}: {}", lang.l("Claude nach 30 Min. abgebrochen", "Claude stopped after 30 min"), t.text(lang)), String::new()),
        Did::CiGaveUp => (
            FeedKind::Problem,
            l("rote CI nach 2 Versuchen", "red CI after 2 attempts"),
            l("kein weiterer Agent, bis die CI grün war", "no further agent until CI was green"),
        ),
        Did::DraftReady => (FeedKind::Agent, l("Antwort-Entwurf bereit", "reply draft ready"), l("liegt oben unter „Braucht dich“", "see “Needs you” above")),
        Did::ReplySent => (FeedKind::Minor, l("Antwort gesendet", "reply sent"), String::new()),
        Did::ReplyFailed(why) => (FeedKind::Problem, l("Antwort nicht gesendet", "reply not sent"), why.clone()),
        Did::JiraMoved(ticket, status) => (FeedKind::Minor, format!("{ticket} {} {status}", lang.l("auf", "to")), String::new()),
        Did::JiraFailed(ticket, why) => (
            FeedKind::Problem,
            format!("{ticket} {}", lang.l("nicht nachgezogen", "not updated")),
            format!("{why}. {}", lang.l("acli angemeldet?", "is acli logged in?")),
        ),
    };
    let mut e = ev(a.at, kind, a.label.clone(), a.url.clone(), text, detail);
    if !a.run.is_empty() {
        e.report = i.st.run(&a.run).map(|r| r.report.clone()).unwrap_or_default();
    }
    e
}

fn ci_event(cause: Option<CiCause>, ci: CiState, lang: Lang) -> Option<(FeedKind, String, String)> {
    let l = |de: &str, en: &str| lang.l(de, en).to_string();
    Some(match cause {
        Some(CiCause::Unknown) => (FeedKind::Problem, l("CI rot geworden", "CI turned red"), String::new()),
        Some(CiCause::CodeSecurity) => (FeedKind::Minor, l("CI rot, erklärt", "CI red, explained"), l("Code Security ist aus", "Code Security is off")),
        Some(CiCause::ReviewGate) => (
            FeedKind::Minor,
            l("CI rot, erklärt", "CI red, explained"),
            l("ändert das Review-Gate, braucht eine menschliche Freigabe", "changes the review gate, needs a human approval"),
        ),
        None if ci == CiState::Success => (FeedKind::Minor, l("CI grün geworden", "CI turned green"), String::new()),
        None => return None,
    })
}

/// Die Einträge der letzten 24 Stunden, neueste zuerst.
pub fn events(i: &FeedInput) -> Vec<FeedEvent> {
    let (lang, now, st) = (i.lang, i.now, i.st);
    let recent = |at: f64| at > 0.0 && now - at < WINDOW_S;
    let mut out: Vec<FeedEvent> = st.activity.iter().filter(|a| recent(a.at)).map(|a| activity(a, i)).collect();
    let deck_merged = |key: &str| st.activity.iter().any(|a| a.pr == key && a.did == Did::Merged);
    for m in i.snap.merged.iter().filter(|m| recent(m.merged_at) && m.number > 0) {
        if deck_merged(&pr_key(&m.repo, m.number)) {
            continue;
        }
        let label = format!("{}#{}", short_repo(&m.repo), m.number);
        out.push(ev(m.merged_at, FeedKind::Merged, label, m.url.clone(), lang.l("gemergt", "merged").into(), short_title(&m.title)));
    }
    for pr in &i.snap.open {
        let (key, x) = (pr.key(), &pr.extra);
        let done = finished_at(&x.checks);
        if recent(done)
            && let Some((kind, text, detail)) = ci_event(i.causes.get(&key).copied().flatten(), if x.checks.is_empty() { pr.ci } else { CiState::Success }, lang)
        {
            out.push(ev(done, kind, pr.label(), pr.url.clone(), text, if detail.is_empty() { short_title(&pr.title) } else { detail }));
        }
        for r in x.reviews.iter().filter(|r| !r.comment.bot && recent(r.comment.at) && !r.comment.author.eq_ignore_ascii_case(&i.snap.viewer)) {
            let (kind, what) = match r.state.as_str() {
                "APPROVED" => (FeedKind::Review, lang.l("Approve von", "approved by")),
                "CHANGES_REQUESTED" => (FeedKind::Review, lang.l("Änderungen gewünscht von", "changes requested by")),
                "COMMENTED" => (FeedKind::Review, lang.l("Review von", "review by")),
                _ => continue,
            };
            out.push(ev(r.comment.at, kind, pr.label(), pr.url.clone(), format!("{what} {}", r.comment.author), short_title(&pr.title)));
        }
        if let Some(&since) = st.conflict_since.get(&key)
            && recent(since)
            && since > st.since
        {
            out.push(ev(since, FeedKind::Problem, pr.label(), pr.url.clone(), lang.l("Konflikt entstanden", "conflict appeared").into(), short_title(&pr.title)));
        }
    }
    out.sort_by(|a, b| b.at.total_cmp(&a.at));
    out.truncate(MAX);
    out
}
