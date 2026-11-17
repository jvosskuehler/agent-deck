//! Das Anzeigemodell des Info-Fensters ([`DashView`]): oben die Tokenkosten je Ticket,
//! unten eine Leiste mit PRs, Reviews, Titan und Aufräumen. Ein Klick auf eine Kachel der
//! Leiste zeigt ihre Liste statt des Graphen; das entscheidet die Oberfläche.
//!
//! Hier stehen die Regeln, was gezeigt wird: Titan nur mit verwaisten Läufen, Aufräumen
//! nur mit entfernbaren Worktrees, PRs nach „wer ist am Zug“ ([`PrBall`]). Texte kommen
//! fertig in der Oberflächensprache an; Zahlen und Zeitachse des Graphen formatiert die
//! Oberfläche, denn nur sie kennt die Ortszeit des Browsers und den Platz.

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::cleanup::{CleanupSnapshot, WtState};
use super::cost_ledger::CostView;
use super::feed_view::{CommentPopup, FeedView};
use super::prs::{short_repo, short_title, CiState, PrBall, PrSnapshot, STALE_AFTER_S};
use super::titan::{load_percent, TitanSnapshot};
use super::DashTone;
use crate::i18n::Lang;

/// Ein eigener PR.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PrRow {
    /// Kurz: `api`, `client`.
    pub repo: String,
    #[ts(type = "number")]
    pub number: u64,
    /// `ORPH-4256` oder leer.
    pub ticket: String,
    pub title: String,
    pub url: String,
    /// „mergebereit“, „wartet auf Re-Review“ …
    pub status: String,
    pub tone: DashTone,
    /// „bei reviewer1 · CI grün“.
    pub meta: String,
}

/// Die eigenen offenen PRs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PrsView {
    /// Erst mergebereit, dann was bei mir liegt, dann was wartet, Drafts zuletzt.
    pub rows: Vec<PrRow>,
    pub ready: u32,
    /// Ich bin am Zug (CI rot, Änderungen offen, Reviewer nicht wieder angefragt).
    pub mine: u32,
    /// Ein Reviewer ist am Zug.
    pub waiting: u32,
    /// Daten alt oder letzter Abruf gescheitert: matt zeigen.
    pub stale: bool,
    /// Stand bzw. Fehler.
    pub note: String,
}

/// Ein fremder PR, bei dem ein Review von mir erbeten ist.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReviewRow {
    pub repo: String,
    #[ts(type = "number")]
    pub number: u64,
    pub title: String,
    pub url: String,
    /// „seit 12 Tagen · +84 −12 · CI rot“.
    pub meta: String,
}

/// Ein verwaister Titan-Lauf.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TitanRunRow {
    /// Voller Name, Schlüssel für `dash_action` (`titanKill:<name>`).
    pub name: String,
    /// Ordner ohne Hash (`4258-w206`).
    pub label: String,
    /// „seit 41 Std.“.
    pub age: String,
}

/// Titan, nur wenn Läufe verwaist sind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TitanView {
    pub runs: Vec<TitanRunRow>,
    /// „10 Container · 2 VMs an · Last 14 % · 290 GB frei“.
    pub facts: String,
}

/// Ein Worktree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WtRow {
    pub folder: String,
    pub repo: String,
    pub branch: String,
}

/// Aufräumen, nur wenn etwas entfernbar ist.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CleanupView {
    pub removable: Vec<WtRow>,
    /// Gemergt, aber mit Änderungen oder neuerem Stand: bleiben.
    pub changed: Vec<WtRow>,
    /// Entfernen läuft gerade.
    pub busy: bool,
    /// Ergebnis des letzten Laufs („24 entfernt, 2 stehen gelassen“); leer = keiner.
    pub result: String,
}

/// Das ganze Info-Fenster.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DashView {
    /// `None` vor dem ersten Lauf über die Transcripts.
    pub costs: Option<CostView>,
    pub prs: PrsView,
    /// Offene Review-Anfragen ohne die ausgeblendeten; nur sie zählt die Kachel.
    pub reviews: Vec<ReviewRow>,
    /// Von mir ausgeblendete Review-Anfragen, unten in der Liste wieder einblendbar.
    pub hidden_reviews: Vec<ReviewRow>,
    pub titan: Option<TitanView>,
    pub cleanup: Option<CleanupView>,
    /// Ergebnis der letzten Aktion („Approve gescheitert: …“), kurz sichtbar.
    pub notice: Option<String>,
    /// PR-Feed hinter der Kachel „PRs offen“ (`feed.rs`); `None` vor dem ersten Abruf.
    pub feed: Option<FeedView>,
    /// Kommentar-Fenster: der neueste noch nicht gemeldete Kommentar eines Menschen.
    pub popup: Option<CommentPopup>,
}

static TICKET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b([A-Z][A-Z0-9]+-\d+)\b").expect("TICKET"));

fn ci_text(ci: CiState, lang: Lang) -> &'static str {
    match ci {
        CiState::Success => lang.l("CI grün", "CI green"),
        CiState::Failure => lang.l("CI rot", "CI red"),
        CiState::Running => lang.l("CI läuft", "CI running"),
        CiState::None => "",
    }
}

fn ball_text(b: PrBall, lang: Lang) -> (&'static str, DashTone) {
    match b {
        PrBall::Ready => (lang.l("mergebereit", "ready to merge"), DashTone::Good),
        PrBall::CiRed => (lang.l("CI rot", "CI red"), DashTone::Bad),
        PrBall::ChangesOpen => (lang.l("Änderungen offen", "changes open"), DashTone::Warn),
        PrBall::AskReviewer => (lang.l("Reviewer anfragen", "re-request review"), DashTone::Warn),
        PrBall::ReReview => (lang.l("wartet auf Re-Review", "awaiting re-review"), DashTone::Plain),
        PrBall::Review => (lang.l("wartet auf Review", "awaiting review"), DashTone::Plain),
        PrBall::Draft => ("Draft", DashTone::Muted),
    }
}

/// Die eigenen PRs.
pub fn prs(snap: &PrSnapshot, now: f64, lang: Lang) -> PrsView {
    let mut open: Vec<_> = snap.open.iter().map(|p| (p.ball(), p)).collect();
    open.sort_by_key(|(b, p)| (*b, short_repo(&p.repo), p.number));
    let rows = open
        .iter()
        .map(|(b, p)| {
            let (status, tone) = ball_text(*b, lang);
            let waiting = matches!(b, PrBall::ReReview | PrBall::Review) && !p.waiting_on.is_empty();
            let threads = (p.open_threads > 0).then(|| format!("{} {}", p.open_threads, lang.l("Threads offen", "open threads")));
            let meta = [
                waiting.then(|| format!("{} {}", lang.l("bei", "with"), p.waiting_on.join(", "))),
                threads,
                Some(ci_text(p.ci, lang).to_string()).filter(|s| !s.is_empty()),
            ];
            PrRow {
                repo: short_repo(&p.repo),
                number: p.number,
                ticket: TICKET.find(&p.title).or_else(|| TICKET.find(&p.head_ref)).map(|m| m.as_str().to_string()).unwrap_or_default(),
                title: short_title(&p.title),
                url: p.url.clone(),
                status: status.into(),
                tone,
                meta: meta.into_iter().flatten().collect::<Vec<_>>().join(" · "),
            }
        })
        .collect();
    let count = |f: &dyn Fn(PrBall) -> bool| open.iter().filter(|(b, _)| f(*b)).count() as u32;
    let old = snap.fetched_at <= 0.0 || now - snap.fetched_at > STALE_AFTER_S;
    let note = match &snap.error {
        Some(e) => format!("{} {e}", lang.l("Abruf gescheitert:", "Fetch failed:")),
        None if snap.fetched_at <= 0.0 => lang.l("noch nicht abgerufen", "not fetched yet").into(),
        None => String::new(),
    };
    PrsView {
        rows,
        ready: count(&|b| b == PrBall::Ready),
        mine: count(&PrBall::is_mine),
        waiting: count(&|b| matches!(b, PrBall::ReReview | PrBall::Review)),
        stale: snap.error.is_some() || old,
        note,
    }
}

fn days_text(days: i64, lang: Lang) -> String {
    match days {
        ..=0 => lang.l("seit heute", "today").into(),
        1 => lang.l("seit gestern", "since yesterday").into(),
        n => format!("{} {n} {}", lang.l("seit", "for"), lang.l("Tagen", "days")),
    }
}

/// Fremde PRs mit Review-Anfrage an mich, neueste zuerst.
pub fn reviews(snap: &PrSnapshot, now: f64, lang: Lang) -> Vec<ReviewRow> {
    let mut rows: Vec<_> = snap.review_requests.iter().collect();
    rows.sort_by(|a, b| b.created_at.total_cmp(&a.created_at));
    rows.into_iter()
        .map(|r| {
            let days = ((now - r.created_at) / 86_400.0).floor() as i64;
            let mut meta = vec![days_text(days, lang), format!("+{} −{}", r.additions, r.deletions)];
            if r.ci == CiState::Failure {
                meta.push(ci_text(r.ci, lang).into());
            }
            ReviewRow { repo: short_repo(&r.repo), number: r.number, title: short_title(&r.title), url: r.url.clone(), meta: meta.join(" · ") }
        })
        .collect()
}

/// Review-Zeilen in sichtbar und ausgeblendet trennen; `hidden` sind PR-Adressen.
pub fn split_reviews(rows: Vec<ReviewRow>, hidden: &[String]) -> (Vec<ReviewRow>, Vec<ReviewRow>) {
    rows.into_iter().partition(|r| !hidden.contains(&r.url))
}

/// Ausgeblendete Adressen vergessen, deren PR nicht mehr auf mich wartet (gemergt,
/// geschlossen, Anfrage zurückgezogen). Kommt dieselbe Anfrage später neu, ist sie wieder
/// sichtbar. Nur nach einem gelungenen Abruf: ein gescheiterter oder noch keiner hätte
/// eine leere Liste und würde alles vergessen. `true` = etwas vergessen.
pub fn prune_hidden(hidden: &mut Vec<String>, snap: &PrSnapshot) -> bool {
    if snap.fetched_at <= 0.0 || snap.error.is_some() {
        return false;
    }
    let before = hidden.len();
    hidden.retain(|url| snap.review_requests.iter().any(|r| &r.url == url));
    hidden.len() != before
}

/// Titan, nur mit verwaisten Läufen (nur die sind zu tun).
pub fn titan(snap: &TitanSnapshot, lang: Lang) -> Option<TitanView> {
    if !snap.reachable {
        return None;
    }
    let runs: Vec<TitanRunRow> = snap
        .runs
        .iter()
        .filter(|r| r.orphan)
        .map(|r| TitanRunRow {
            name: r.name.clone(),
            label: super::titan::run_label(&r.name),
            age: r.age_s.map_or_else(String::new, |s| format!("{} {} {}", lang.l("seit", "for"), (s as f64 / 3600.0).round(), lang.l("Std.", "h"))),
        })
        .collect();
    if runs.is_empty() {
        return None;
    }
    let facts = [
        format!("{} Container", snap.containers),
        format!("{} {}", snap.vms.len(), lang.l("VMs an", "VMs on")),
        format!("{} {:.0} %", lang.l("Last", "load"), load_percent(snap)),
        format!("{:.0} {}", snap.mem_avail_gb, lang.l("GB frei", "GB free")),
    ];
    Some(TitanView { runs, facts: facts.join(" · ") })
}

/// Aufräumen, nur mit entfernbaren Worktrees (oder während ein Lauf noch arbeitet).
pub fn cleanup(snap: &CleanupSnapshot, lang: Lang) -> Option<CleanupView> {
    let busy = snap.busy;
    let row = |c: &super::cleanup::WtCandidate| WtRow { folder: c.folder.clone(), repo: short_repo(&c.repo), branch: c.branch.clone() };
    let removable: Vec<WtRow> = snap.removable().map(row).collect();
    if removable.is_empty() && !busy {
        return None;
    }
    let changed = snap.candidates.iter().filter(|c| c.state == WtState::Changed).map(row).collect();
    let result = snap.last_result.map_or_else(String::new, |r| {
        let mut parts = vec![format!("{} {}", r.removed, lang.l("entfernt", "removed"))];
        if r.skipped > 0 {
            parts.push(format!("{} {}", r.skipped, lang.l("stehen gelassen", "left in place")));
        }
        if r.failed > 0 {
            parts.push(format!("{} {}", r.failed, lang.l("gescheitert", "failed")));
        }
        parts.join(", ")
    });
    Some(CleanupView { removable, changed, busy, result })
}

#[cfg(test)]
#[path = "board_tests.rs"]
mod tests;
