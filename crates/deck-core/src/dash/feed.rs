//! Der PR-Feed ([`FeedView`]) und das Kommentar-Fenster ([`CommentPopup`]): „Braucht
//! dich“ (Kommentare von Menschen, auf die ich noch nicht reagiert habe), die
//! Hintergrund-Agenten, die Zeitleiste (`feed_events.rs`) und was die Automatik je PR
//! vorhat.
//!
//! „Noch nicht reagiert“ heißt: neuer als meine letzte Antwort, mein letztes Review oder
//! mein letzter Commit (`PrExtra::my_last_activity`). Bots zählen nie, Threads, die
//! aufgelöst sind, auch nicht. „Später“ blendet aus, bis ein neuerer Kommentar kommt.

use super::automation::merge_wait;
use super::bg_tasks::{Causes, Due, Hold};
use super::feed_events::events;
use super::feed_state::FeedState;
use super::feed_view::{AgentRow, AutoRow, CommentPopup, DraftView, FeedView, NeedCard};
use super::pr_extra::Comment;
use super::prs::{short_title, PrInfo, PrSnapshot};
use crate::i18n::Lang;

/// Wie ein offener Kommentar gemeint ist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    Commented,
    InThread,
    ChangesRequested,
}

/// Ein Kommentar eines Menschen, auf den ich noch nicht reagiert habe.
#[derive(Debug, Clone, PartialEq)]
pub struct Pending {
    pub comment: Comment,
    pub verb: Verb,
    /// Review-Thread des Kommentars; leer = Kommentar unter dem PR bzw. Review.
    pub thread: String,
}

/// Offene Kommentare von Menschen auf `pr`, älteste zuerst.
pub fn pending(pr: &PrInfo, me: &str) -> Vec<Pending> {
    let x = &pr.extra;
    let after = x.my_last_activity(me);
    let human = |c: &Comment| !c.bot && !c.author.eq_ignore_ascii_case(me) && c.at > after;
    let mut out: Vec<Pending> = Vec::new();
    for c in x.comments.iter().filter(|c| human(c)) {
        out.push(Pending { comment: c.clone(), verb: Verb::Commented, thread: String::new() });
    }
    for r in x.reviews.iter().filter(|r| human(&r.comment)) {
        let verb = match r.state.as_str() {
            "CHANGES_REQUESTED" => Verb::ChangesRequested,
            "COMMENTED" if !r.comment.text.trim().is_empty() => Verb::Commented,
            _ => continue,
        };
        out.push(Pending { comment: r.comment.clone(), verb, thread: String::new() });
    }
    for t in x.threads.iter().filter(|t| !t.resolved) {
        for c in t.head.iter().chain(t.tail.iter()).filter(|c| human(c)) {
            if !out.iter().any(|p| p.comment.id == c.id && !c.id.is_empty()) {
                out.push(Pending { comment: c.clone(), verb: Verb::InThread, thread: t.id.clone() });
            }
        }
    }
    out.sort_by(|a, b| a.comment.at.total_cmp(&b.comment.at));
    out
}

/// Kürzel im Kreis: `reviewer-one` → `RO`, `lh` → `LH`.
pub fn initials(login: &str) -> String {
    let parts: Vec<&str> = login.split(['-', '_', '.', ' ']).filter(|p| !p.is_empty()).collect();
    let s: String = match parts.as_slice() {
        [a, b, ..] => a.chars().take(1).chain(b.chars().take(1)).collect(),
        [a] => a.chars().take(2).collect(),
        [] => "?".into(),
    };
    s.to_uppercase()
}

/// Ein Text als eine Zeile, höchstens `max` Zeichen (mit „…“).
pub fn excerpt(text: &str, max: usize) -> String {
    let one = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= max {
        return one;
    }
    let cut: String = one.chars().take(max).collect();
    format!("{}…", cut.trim_end())
}

/// „seit 4 Min.“.
pub fn since_text(secs: f64, lang: Lang) -> String {
    let m = (secs.max(0.0) / 60.0).floor() as i64;
    match m {
        0 => lang.l("seit eben", "just now").into(),
        1..=59 => format!("{} {m} {}", lang.l("seit", "for"), lang.l("Min.", "min")),
        _ => format!("{} {} {}", lang.l("seit", "for"), m / 60, lang.l("Std.", "h")),
    }
}

fn verb_text(v: Verb, lang: Lang) -> &'static str {
    match v {
        Verb::Commented => lang.l("kommentiert", "commented"),
        Verb::InThread => lang.l("antwortet im Code", "replied in code"),
        Verb::ChangesRequested => lang.l("wünscht Änderungen", "requested changes"),
    }
}

/// Was der Feed zum Bauen braucht.
pub struct FeedInput<'a> {
    pub snap: &'a PrSnapshot,
    pub st: &'a FeedState,
    pub causes: &'a Causes,
    /// Fällige Aufträge, die noch warten (Session aktiv, alle Plätze belegt).
    pub waiting: &'a [(Due, Hold)],
    pub now: f64,
    pub lang: Lang,
}

fn card(pr: &PrInfo, list: &[Pending], i: &FeedInput) -> NeedCard {
    let lang = i.lang;
    let newest = list.last().expect("nicht leer");
    let key = pr.key();
    let run = i.st.running().find(|r| r.pr == key);
    let draft = i.st.drafts.get(&key).filter(|d| !d.text.trim().is_empty()).map(|d| DraftView {
        text: d.text.trim().to_string(),
        thread: !d.thread_id.is_empty(),
        sending: d.sending,
        error: d.error.clone(),
    });
    let agent = run.map_or_else(String::new, |r| format!("{}, {}", lang.l("Claude setzt um", "Claude is on it"), since_text(i.now - r.started, lang)));
    NeedCard {
        key: key.clone(),
        initials: initials(&newest.comment.author),
        login: newest.comment.author.clone(),
        label: pr.label(),
        title: short_title(&pr.title),
        url: if newest.comment.url.is_empty() { pr.url.clone() } else { newest.comment.url.clone() },
        at: newest.comment.at,
        verb: verb_text(newest.verb, lang).into(),
        excerpt: excerpt(&newest.comment.text, 160),
        more: (list.len() - 1) as u32,
        can_delegate: run.is_none() && draft.is_none(),
        agent,
        draft,
    }
}

/// Feed und Kommentar-Fenster.
pub fn build(i: &FeedInput) -> (FeedView, Option<CommentPopup>) {
    let (st, lang, me) = (i.st, i.lang, i.snap.viewer.as_str());
    let mut needs: Vec<(NeedCard, String)> = Vec::new();
    // Drafts sind geparkt: sie brauchen niemanden, auch kein Kommentar-Fenster.
    for pr in i.snap.open.iter().filter(|p| !p.draft) {
        let list = pending(pr, me);
        let Some(newest) = list.last() else { continue };
        let key = pr.key();
        let has_draft = st.drafts.get(&key).is_some_and(|d| !d.text.trim().is_empty());
        if st.snoozed.get(&key).is_some_and(|&t| newest.comment.at <= t) && !has_draft {
            continue;
        }
        needs.push((card(pr, &list, i), newest.comment.text.clone()));
    }
    needs.sort_by(|a, b| b.0.at.total_cmp(&a.0.at));
    let popup = needs
        .iter()
        .find(|(c, _)| c.at > st.since && st.notified.get(&c.key).is_none_or(|&t| c.at > t))
        .map(|(c, text)| CommentPopup { card: c.clone(), text: excerpt(text, 300) });
    let mut agents: Vec<AgentRow> = st
        .running()
        .map(|r| AgentRow {
            id: r.id.clone(),
            label: r.label.clone(),
            url: r.url.clone(),
            task: r.task.text(lang).into(),
            state: since_text(i.now - r.started, lang),
            waiting: false,
        })
        .collect();
    for (d, hold) in i.waiting {
        let Some(pr) = i.snap.open.iter().find(|p| p.key() == d.pr) else { continue };
        let state = match hold {
            Hold::SessionActive => lang.l("wartet, Session aktiv", "waiting, session active"),
            Hold::Full => lang.l("wartet auf freien Platz", "waiting for a free slot"),
        };
        agents.push(AgentRow { id: String::new(), label: pr.label(), url: pr.url.clone(), task: d.task.text(lang).into(), state: state.into(), waiting: true });
    }
    let auto = i
        .snap
        .open
        .iter()
        .map(|pr| {
            let wait = merge_wait(pr, &i.snap.open, i.causes.get(&pr.key()).copied().flatten(), st);
            AutoRow {
                key: pr.key(),
                label: pr.label(),
                url: pr.url.clone(),
                title: short_title(&pr.title),
                state: wait.map_or_else(|| lang.l("wird gemergt", "merging now").to_string(), |w| w.text(lang)),
                held: st.hold.contains(&pr.key()),
            }
        })
        .collect();
    let note = match &i.snap.error {
        Some(e) => format!("{} {e}", lang.l("Abruf gescheitert:", "Fetch failed:")),
        None if i.snap.fetched_at <= 0.0 => lang.l("noch nicht abgerufen", "not fetched yet").into(),
        None => String::new(),
    };
    let view = FeedView { needs: needs.into_iter().map(|(c, _)| c).collect(), agents, events: events(i), auto, automation: st.automation, note };
    (view, popup)
}

#[cfg(test)]
#[path = "feed_tests.rs"]
mod tests;
