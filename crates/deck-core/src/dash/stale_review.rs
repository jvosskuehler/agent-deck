//! Ein überholtes „Änderungen gewünscht“ von Claude verwerfen, wenn es als Einziges noch
//! den Merge aufhält.
//!
//! Warum: GitHub zählt je Konto das letzte Review mit Urteil (APPROVED oder
//! CHANGES_REQUESTED); ein späteres COMMENTED hebt ein „Änderungen gewünscht“ nicht auf.
//! Claude gibt `high`-PRs nie frei, sondern kommentiert am neuen Stand nur. Ist der Fund
//! behoben und der Thread aufgelöst, bleibt der PR trotzdem auf `reviewDecision`
//! CHANGES_REQUESTED und `mergeStateStatus` BLOCKED stehen, bis jemand das alte Review von
//! Hand verwirft (am 19.11.2026 so geschehen).
//!
//! Verworfen wird nur, wenn ALLES stimmt (sonst `None`):
//! - das Review ist von Claude ([`is_claude`]), nie von einem Menschen,
//! - es lautet CHANGES_REQUESTED und gilt einem älteren Commit als dem Kopf,
//! - Claudes neuestes Review gilt dem Kopf und wünscht keine Änderungen,
//! - kein Review-Thread ist offen,
//! - GitHub sagt CHANGES_REQUESTED und BLOCKED,
//! - ohne dieses Review wäre der PR nach der Regel in [`merge_wait`] mergebar (kein Draft,
//!   nicht gehalten, CI grün, Freigabe am Kopf, bei `high` ein Mensch …),
//! - an diesem Kopf ist das Verwerfen nicht schon einmal gescheitert.
//!
//! Gemergt wird danach nicht sofort: erst der nächste Abruf zeigt, ob GitHub den PR
//! freigibt, und dann gilt wieder [`merge_wait`] mit allen Bedingungen.

use super::automation::{merge_wait, MergeWait};
use super::ci_cause::CiCause;
use super::feed_state::FeedState;
use super::pr_extra::{is_claude, Review};
use super::prs::{PrInfo, ReviewState};

/// Was verworfen wird.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dismissal {
    /// REST-Ids der überholten Reviews (meist eines).
    pub ids: Vec<u64>,
    /// Konto des Bots, wie GitHub es nennt.
    pub author: String,
    /// Kopf-Commit, gekürzt auf 7 Zeichen.
    pub head: String,
    /// Begründung, die GitHub am Review zeigt.
    pub message: String,
}

/// Die Begründung am verworfenen Review (englisch, steht auf GitHub).
pub fn message(head: &str) -> String {
    format!("All findings addressed in {head}; the latest review on the head requests no changes.")
}

fn short(oid: &str) -> String {
    oid.chars().take(7).collect()
}

/// Claudes überholte „Änderungen gewünscht“: an einem bekannten Commit, der nicht der
/// Kopf ist, mit REST-Id.
fn stale(pr: &PrInfo) -> Vec<&Review> {
    let x = &pr.extra;
    x.reviews
        .iter()
        .filter(|r| is_claude(&r.comment.author) && r.state == "CHANGES_REQUESTED")
        .filter(|r| !r.oid.is_empty() && r.oid != x.head_oid && r.db_id > 0)
        .collect()
}

/// Hat Claude den Kopf zuletzt begutachtet, ohne Änderungen zu wünschen?
fn latest_on_head_is_fine(pr: &PrInfo) -> bool {
    let x = &pr.extra;
    x.reviews
        .iter()
        .filter(|r| is_claude(&r.comment.author))
        .max_by(|a, b| a.comment.at.total_cmp(&b.comment.at))
        .is_some_and(|r| r.oid == x.head_oid && r.state != "CHANGES_REQUESTED")
}

/// Soll an `pr` ein überholtes Review von Claude verworfen werden? Argumente wie
/// [`merge_wait`].
pub fn dismissal(pr: &PrInfo, open: &[PrInfo], cause: Option<CiCause>, st: &FeedState) -> Option<Dismissal> {
    let x = &pr.extra;
    if x.head_oid.is_empty() || pr.review != ReviewState::ChangesRequested || x.merge_state != "BLOCKED" {
        return None;
    }
    if pr.open_threads > 0 || x.threads.iter().any(|t| !t.resolved) || !latest_on_head_is_fine(pr) {
        return None;
    }
    let old = stale(pr);
    let first = old.first()?;
    if st.handled.get(&pr.key()).is_some_and(|h| h.dismiss_failed_oid == x.head_oid) {
        return None;
    }
    // Heute blockiert nur GitHub; ohne die alten Reviews müsste die eigene Regel mergen.
    if merge_wait(pr, open, cause, st) != Some(MergeWait::State("BLOCKED".into())) {
        return None;
    }
    let mut without = pr.clone();
    without.review = ReviewState::None;
    without.extra.merge_state = "CLEAN".into();
    for r in without.extra.reviews.iter_mut().filter(|r| old.iter().any(|o| o.db_id == r.db_id)) {
        r.state = "DISMISSED".into();
    }
    if merge_wait(&without, open, cause, st).is_some() {
        return None;
    }
    let head = short(&x.head_oid);
    Some(Dismissal { ids: old.iter().map(|r| r.db_id).collect(), author: first.comment.author.clone(), message: message(&head), head })
}

#[cfg(test)]
#[path = "stale_review_tests.rs"]
mod tests;
