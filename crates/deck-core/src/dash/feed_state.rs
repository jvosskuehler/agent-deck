//! Das Gedächtnis des PR-Feeds, gespeichert in `state\pr-feed.json` (`docs/DATEIEN.md`):
//! Not-Aus der Automatik, Halteliste, „Später“, schon gemeldete Kommentare, seit wann ein
//! Konflikt besteht, was die Hintergrund-Agenten schon bearbeitet haben, ihre Läufe und
//! Entwürfe, und was das Deck selbst getan hat (Zeitleiste).
//!
//! Der Dienst (`deck-services/src/dash/feed_service.rs`) liest und schreibt die Datei
//! atomar; hier stehen nur Form und Regeln. Gelesen wird tolerant: eine kaputte Datei
//! ist ein frischer Stand (dann gilt `since` = jetzt, und nichts Altes löst etwas aus).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::bg_tasks::TaskKind;
use super::prs::PrSnapshot;

/// So lange (s) bleiben Einträge der Zeitleiste und beendete Läufe in der Datei.
pub const KEEP_S: f64 = 48.0 * 3600.0;
/// Höchstens so viele gemerkte Log-Urteile (Job-Id → erklärt).
const MAX_VERDICTS: usize = 400;

/// Was das Deck getan hat (Einträge der Zeitleiste).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail", rename_all = "camelCase")]
pub enum Did {
    Merged,
    MergeFailed(String),
    BranchDeleted,
    ReviewerAdded(String),
    /// Überholtes „Änderungen gewünscht“ eines Bots verworfen: (Bot, Kopf-Commit kurz).
    ReviewDismissed(String, String),
    /// Verwerfen gescheitert: Grund.
    DismissFailed(String),
    AssigneeAdded,
    WorktreeRemoved(String),
    AgentStarted(TaskKind),
    AgentDone(TaskKind),
    AgentFailed(TaskKind, String),
    AgentAborted(TaskKind),
    AgentTimedOut(TaskKind),
    /// Rote CI nach [`super::bg_tasks::MAX_CI_TRIES`] Versuchen: kein weiterer Agent,
    /// bis die CI einmal grün war.
    CiGaveUp,
    DraftReady,
    ReplySent,
    ReplyFailed(String),
    /// Jira-Ticket verschoben: (Ticket, neuer Status).
    JiraMoved(String, String),
    /// Jira nicht erreichbar oder Übergang abgelehnt: (Ticket, Grund).
    JiraFailed(String, String),
}

/// Ein Eintrag „das Deck hat …“.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub at: f64,
    /// `owner/repo#12`.
    pub pr: String,
    /// `client#393`.
    pub label: String,
    pub url: String,
    pub title: String,
    pub did: Did,
    /// Lauf-Id, wenn ein Hintergrund-Agent dahintersteht (für den Bericht).
    #[serde(default)]
    pub run: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RunStatus {
    Running,
    Done,
    Failed,
    Aborted,
    TimedOut,
}

/// Ein Lauf eines Hintergrund-Agenten.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: String,
    pub pr: String,
    pub label: String,
    pub url: String,
    pub title: String,
    pub task: TaskKind,
    pub started: f64,
    /// 0 = läuft noch.
    #[serde(default)]
    pub ended: f64,
    pub status: RunStatus,
    /// Letzte Antwort des Agenten (gekürzt).
    #[serde(default)]
    pub report: String,
    /// Arbeitsordner (worktree).
    pub dir: String,
    /// Nur bei `HumanReply`: der Kommentar, auf den der Entwurf antwortet.
    #[serde(default)]
    pub comment_id: String,
    #[serde(default)]
    pub thread_id: String,
    #[serde(default)]
    pub draft_path: String,
}

/// Ein Antwort-Entwurf, wartet auf „Absenden“.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Draft {
    pub comment_id: String,
    /// Leer = Antwort als PR-Kommentar, sonst in diesen Review-Thread.
    pub thread_id: String,
    pub path: String,
    /// Aktueller Text der Datei (der Dienst liest ihn jede Runde nach).
    pub text: String,
    pub at: f64,
    pub sending: bool,
    pub error: String,
}

/// Was die Hintergrund-Agenten zu einem PR schon bekommen haben.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Handled {
    /// Bot-Threads bis zu diesem Zeitpunkt sind bearbeitet.
    pub bot: f64,
    /// Rote CI an diesem Commit ist bearbeitet.
    pub ci_oid: String,
    /// So viele Agenten haben die rote CI schon versucht, seit sie zuletzt grün war.
    /// Jeder Versuch pusht einen neuen Commit; ohne Zähler startete jeder rote Lauf am
    /// neuen Commit den nächsten Agenten (einmal vier in 40 Minuten auf demselben PR).
    pub ci_tries: u32,
    /// Konflikt seit diesem Zeitpunkt ist bearbeitet.
    pub conflict: f64,
    /// Reviewer bzw. Assignee schon einmal nachgetragen (nie zweimal: wer sich selbst
    /// austrägt, wird nicht wieder eingetragen).
    pub reviewer: bool,
    pub assignee: bool,
    /// Merge an diesem Commit scheiterte (neuer Versuch erst mit neuem Commit).
    pub merge_failed_oid: String,
    /// Verwerfen von Claudes überholtem Review scheiterte an diesem Commit
    /// (`stale_review`; neuer Versuch erst mit neuem Commit).
    pub dismiss_failed_oid: String,
}

/// Das ganze Gedächtnis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FeedState {
    /// Ab hier zählt alles als neu (erster Start): ältere Kommentare melden nichts,
    /// ältere Funde starten keinen Agenten.
    pub since: f64,
    /// Not-Aus: aus = nur anzeigen.
    pub automation: bool,
    /// Halteliste: diese PRs mergt die Automatik nie.
    pub hold: BTreeSet<String>,
    /// „Später“: ausgeblendet bis zu einem Kommentar nach diesem Zeitpunkt.
    pub snoozed: BTreeMap<String, f64>,
    /// Kommentare bis zu diesem Zeitpunkt hat das Kommentar-Fenster schon gezeigt.
    pub notified: BTreeMap<String, f64>,
    pub conflict_since: BTreeMap<String, f64>,
    pub handled: BTreeMap<String, Handled>,
    /// Job-Id → das Log erklärt den Fehlschlag mit Code Security (`ci_cause`).
    pub verdicts: BTreeMap<u64, bool>,
    pub activity: Vec<Activity>,
    pub runs: Vec<Run>,
    pub drafts: BTreeMap<String, Draft>,
    /// Gemergte PRs, deren Ticket schon nachgezogen wurde (Schlüssel → wann).
    pub jira_checked: BTreeMap<String, f64>,
    /// Schon einmal einen gelungenen Abruf gesehen. Was der erste Abruf zeigt, bestand
    /// schon vorher: ein Konflikt bekommt dann `since`, nicht „jetzt“, und löst nichts aus.
    pub seen_any: bool,
    /// Regelstand der gemerkten Log-Urteile (`ci_cause::VERDICT_RULE`). Fehlt er in der
    /// Datei, ist sie älter als jede Regel: 0, die Urteile werden neu geholt.
    #[serde(default)]
    pub verdict_rule: u32,
    /// Zustand der PRs, nach denen eigene PRs kommen sollen (`merge_order`): bei jedem
    /// Abruf frisch von GitHub, darum nicht gespeichert.
    #[serde(skip)]
    pub deps: super::merge_order::DepStates,
}

impl Default for FeedState {
    fn default() -> FeedState {
        FeedState {
            since: 0.0,
            // Aus, bis die Einstellung `pr_automation` es anders sagt (`FeedService::set_config`):
            // eine neue Installation wirkt nie nach außen, bevor jemand einschaltet.
            automation: false,
            hold: BTreeSet::new(),
            snoozed: BTreeMap::new(),
            notified: BTreeMap::new(),
            conflict_since: BTreeMap::new(),
            handled: BTreeMap::new(),
            verdicts: BTreeMap::new(),
            activity: Vec::new(),
            runs: Vec::new(),
            drafts: BTreeMap::new(),
            jira_checked: BTreeMap::new(),
            seen_any: false,
            verdict_rule: super::ci_cause::VERDICT_RULE,
            deps: Default::default(),
        }
    }
}

impl FeedState {
    /// Frisch beim ersten Start: ab jetzt zählt alles als neu.
    pub fn fresh(now: f64) -> FeedState {
        FeedState { since: now, ..FeedState::default() }
    }

    /// Aus der Datei; kaputt oder fehlend = frisch. Ein Lauf, der beim Beenden des Decks
    /// noch lief, gilt als gescheitert (sein Prozess gehört nicht mehr zu uns).
    pub fn from_json(text: Option<&str>, now: f64) -> FeedState {
        let mut st = text.and_then(|t| serde_json::from_str::<FeedState>(t).ok()).filter(|s| s.since > 0.0).unwrap_or_else(|| FeedState::fresh(now));
        if st.verdict_rule != super::ci_cause::VERDICT_RULE {
            st.verdicts.clear();
            st.verdict_rule = super::ci_cause::VERDICT_RULE;
        }
        for r in st.runs.iter_mut().filter(|r| r.status == RunStatus::Running) {
            r.status = RunStatus::Failed;
            r.ended = now;
            r.report = "Deck neu gestartet, Lauf nicht mehr verfolgt".into();
        }
        st
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Neuen Stand von GitHub einarbeiten: Konflikte datieren, Altes wegräumen. `true`,
    /// wenn sich etwas geändert hat (dann speichern).
    pub fn observe(&mut self, snap: &PrSnapshot, now: f64) -> bool {
        let before = self.clone();
        let open: BTreeSet<String> = snap.open.iter().map(|p| p.key()).collect();
        let ok = snap.error.is_none() && snap.fetched_at > 0.0;
        let first_seen = if self.seen_any { now } else { self.since };
        for p in &snap.open {
            if super::ci_cause::all_green(&p.extra.checks)
                && let Some(h) = self.handled.get_mut(&p.key())
            {
                h.ci_tries = 0;
            }
            if p.extra.conflicting {
                self.conflict_since.entry(p.key()).or_insert(first_seen);
            } else {
                self.conflict_since.remove(&p.key());
            }
        }
        // Nur aufräumen, wenn der Abruf geklappt hat: eine leere Liste nach einem Fehler
        // hieße sonst „alle PRs zu“.
        if ok {
            self.seen_any = true;
            self.conflict_since.retain(|k, _| open.contains(k));
            self.hold.retain(|k| open.contains(k));
            self.snoozed.retain(|k, _| open.contains(k));
            self.notified.retain(|k, _| open.contains(k));
            self.handled.retain(|k, _| open.contains(k));
            self.drafts.retain(|k, d| open.contains(k) || d.sending);
        }
        self.activity.retain(|a| now - a.at < KEEP_S);
        self.jira_checked.retain(|_, at| now - *at < KEEP_S);
        self.runs.retain(|r| r.status == RunStatus::Running || now - r.ended < KEEP_S);
        while self.verdicts.len() > MAX_VERDICTS {
            self.verdicts.pop_first();
        }
        *self != before
    }

    pub fn record(&mut self, a: Activity) {
        self.activity.push(a);
    }

    pub fn handled_mut(&mut self, key: &str) -> &mut Handled {
        self.handled.entry(key.to_string()).or_default()
    }

    pub fn running(&self) -> impl Iterator<Item = &Run> {
        self.runs.iter().filter(|r| r.status == RunStatus::Running)
    }

    pub fn run(&self, id: &str) -> Option<&Run> {
        self.runs.iter().find(|r| r.id == id)
    }
}

#[cfg(test)]
#[path = "feed_state_tests.rs"]
mod tests;
