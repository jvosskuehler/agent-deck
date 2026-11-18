//! Datenquelle und Automatik des PR-Feeds: ein Thread, der nach jedem Abruf der PRs
//! (`github.rs`) und alle [`ROUND`] nachsieht, was zu tun ist (`feed_round.rs`): Logs
//! roter Checks holen (`ci_cause`), Reviewer/Assignee nachtragen und mergen
//! (`automation`), Jira nachziehen (`jira_sync`), Hintergrund-Agenten starten,
//! überwachen und beenden (`bg_tasks`). Die Knöpfe des Feeds landen in `feed_actions.rs`.
//!
//! Das Gedächtnis steht in `state\pr-feed.json` (`FeedState`, atomar geschrieben),
//! Ausgaben der Agenten in `feed\runs\`, Entwürfe in `feed\drafts\`.
//!
//! Regel für die Sperre: nie gh, git, acli oder claude aufrufen, während `inner` gesperrt
//! ist; die Oberfläche fragt im 400-ms-Takt nach der Ansicht ([`FeedService::view`]).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use deck_core::dash::bg_tasks::{Causes, Due, Hold};
use deck_core::dash::feed::{build, FeedInput};
use deck_core::dash::feed_state::FeedState;
use deck_core::dash::feed_view::{CommentPopup, FeedView};
use deck_core::dash::prs::PrSnapshot;
use deck_core::i18n::Lang;

use super::agents::{AgentProc, AgentRunner, ClaudeCli};
use super::effects::{Acli, GhCliWrite, GhWrite, JiraApi};
use super::github::{GhApi, GhCli};
use super::hidden_cmd::TimedGit;
use super::ticker::{self, Ticker, Wake};
use crate::fsutil::now_unix;
use crate::logfile::{LogFile, Rotate};
use crate::worktree::Git;

/// Takt ohne neuen Abruf: Agenten überwachen, Entwürfe nachlesen.
pub const ROUND: Duration = Duration::from_secs(15);
pub const LOG_FILE: &str = "dash-feed.log";
pub const STATE_FILE: &str = "pr-feed.json";
/// So lange (s) wartet das Aufräumen nach einem eigenen Merge auf den Aufräum-Dienst.
pub const CLEAN_WAIT_S: f64 = 3600.0;
/// So lange (s) gilt eine gebaute Ansicht (relative Zeiten).
const VIEW_TTL_S: f64 = 20.0;

/// Einstellungen aus `deck_settings.json`, die der Feed braucht.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FeedConfig {
    /// `pr_automation`: die einzige Quelle, ob der Feed nach außen wirkt (mergen,
    /// Reviewer/Assignee, Jira, Hintergrund-Agenten). Aus = der Feed zeigt nur.
    pub automation: bool,
    /// `pr_reviewer`: wer als Reviewer nachgetragen wird (leer = niemand).
    pub reviewers: Vec<String>,
    /// `github_org`: Organisation für die Jira-Suche (leer = kein Jira).
    pub org: String,
    /// `jira_prefix`.
    pub prefix: String,
    /// Ordner der Haupt-Checkouts (`repos_root`); leer = nicht gesetzt, dann starten
    /// keine Hintergrund-Agenten.
    pub repos_root: PathBuf,
}

/// Was der Dienst nach außen braucht (Attrappen in Tests).
pub struct FeedDeps {
    pub gh: Arc<dyn GhApi>,
    pub write: Arc<dyn GhWrite>,
    pub jira: Arc<dyn JiraApi>,
    pub runner: Arc<dyn AgentRunner>,
    pub git: Arc<dyn Git>,
    /// Letzter Stand der PRs (vom GitHub-Dienst).
    pub prs: Arc<dyn Fn() -> PrSnapshot + Send + Sync>,
    pub now: Arc<dyn Fn() -> f64 + Send + Sync>,
}

pub(crate) struct Inner {
    pub st: FeedState,
    pub dirty: bool,
    pub procs: HashMap<String, Box<dyn AgentProc>>,
    pub causes: Causes,
    pub waiting: Vec<(Due, Hold)>,
    pub seen_fetch: f64,
    pub cfg: FeedConfig,
    /// Ordner der offenen Agenten-Sessions des Decks.
    pub in_use: Vec<String>,
    /// Bekannte worktrees je PR (vom letzten Start).
    pub pr_dirs: HashMap<String, PathBuf>,
    /// Vom Deck gemergt, worktree noch aufzuräumen: (Repo, Branch, seit wann).
    pub to_clean: Vec<(String, String, f64)>,
    /// Zuletzt geloggter Grund je PR, warum kein Agent startete. Geloggt wird nur ein
    /// Wechsel, nicht jede Runde (sonst alle 15 s dieselbe Zeile).
    pub not_started: HashMap<String, String>,
    pub version: u64,
    view: Option<(u64, f64, f64, Lang, FeedView, Option<CommentPopup>)>,
}

pub(crate) struct Shared {
    pub deps: FeedDeps,
    pub inner: Mutex<Inner>,
    pub file: PathBuf,
    pub work_dir: PathBuf,
    pub log: LogFile,
    pub ticker: Arc<Ticker>,
    pub wake: Mutex<Option<Wake>>,
}

/// Der Dienst.
pub struct FeedService {
    pub(crate) sh: Arc<Shared>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    ticker::lock(m)
}

impl FeedService {
    /// `dir`: App-Ordner (`state_dir` für Gedächtnis und Protokoll, `work_dir` für
    /// Ausgaben und Entwürfe).
    pub fn new(deps: FeedDeps, state_dir: &Path, work_dir: &Path, log: LogFile) -> FeedService {
        let file = state_dir.join(STATE_FILE);
        let now = (deps.now)();
        let text = std::fs::read_to_string(&file).ok();
        let st = FeedState::from_json(text.as_deref(), now);
        let fresh = text.is_none();
        let inner = Inner {
            dirty: fresh,
            st,
            procs: HashMap::new(),
            causes: Causes::new(),
            waiting: Vec::new(),
            seen_fetch: 0.0,
            cfg: FeedConfig::default(),
            in_use: Vec::new(),
            pr_dirs: HashMap::new(),
            to_clean: Vec::new(),
            not_started: HashMap::new(),
            version: 0,
            view: None,
        };
        if fresh {
            log.note("Gedächtnis neu angelegt: ältere Kommentare und Funde lösen nichts aus");
        }
        let sh = Shared { deps, inner: Mutex::new(inner), file, work_dir: work_dir.to_path_buf(), log, ticker: Arc::new(Ticker::default()), wake: Mutex::new(None) };
        FeedService { sh: Arc::new(sh), thread: Mutex::new(None) }
    }

    /// Die echte Fassung: gh, acli, claude, git; `prs` liefert den Stand des GitHub-Dienstes.
    pub fn system(state_dir: &Path, app_dir: &Path, prs: Arc<dyn Fn() -> PrSnapshot + Send + Sync>) -> FeedService {
        let deps = FeedDeps {
            gh: Arc::new(GhCli::default()),
            write: Arc::new(GhCliWrite),
            jira: Arc::new(Acli),
            runner: Arc::new(ClaudeCli::default()),
            git: Arc::new(TimedGit::default()),
            prs,
            now: Arc::new(now_unix),
        };
        let log = LogFile::new(state_dir.join(LOG_FILE), 300_000, Rotate::Rename);
        FeedService::new(deps, state_dir, &app_dir.join("feed"), log)
    }

    pub fn start(&self, wake: Wake) {
        *lock(&self.sh.wake) = Some(wake);
        let mut t = lock(&self.thread);
        if t.is_some() {
            return;
        }
        let sh = self.sh.clone();
        *t = ticker::spawn("dash-feed", self.sh.ticker.clone(), ROUND, move || {
            super::feed_round::round(&sh);
            ROUND
        });
    }

    pub fn stop(&self) {
        self.sh.ticker.stop();
        lock(&self.thread).take();
    }

    /// Neuer Abruf da oder Knopf gedrückt: gleich eine Runde.
    pub fn kick(&self) {
        self.sh.ticker.kick();
    }

    pub fn set_config(&self, cfg: FeedConfig) {
        let mut i = lock(&self.sh.inner);
        // Die Einstellung ist die Quelle; das Gedächtnis zieht nach (Ansicht, Not-Aus).
        if i.st.automation != cfg.automation {
            i.st.automation = cfg.automation;
            i.dirty = true;
            self.sh.log.note(if cfg.automation { "Automatik an (Einstellung)" } else { "Automatik aus (Einstellung)" });
        }
        if i.cfg != cfg {
            i.cfg = cfg;
        }
    }

    pub fn set_in_use(&self, dirs: Vec<String>) {
        lock(&self.sh.inner).in_use = dirs;
    }

    /// Eine Runde synchron (Tests und Probelauf).
    pub fn round_now(&self) {
        super::feed_round::round(&self.sh);
    }

    /// Worktrees, die das Deck nach einem eigenen Merge aufräumen soll: (Repo, Branch).
    /// Die App reicht sie an den Aufräum-Dienst, sobald der sie als entfernbar kennt (er
    /// prüft seine Regeln selbst); nach [`CLEAN_WAIT_S`] gibt das Deck auf.
    pub fn clean_wanted(&self, now: f64) -> Vec<(String, String)> {
        let mut i = lock(&self.sh.inner);
        i.to_clean.retain(|(_, _, at)| now - at < CLEAN_WAIT_S);
        i.to_clean.iter().map(|(r, b, _)| (r.clone(), b.clone())).collect()
    }

    /// Der worktree von (`repo`, `branch`) ist erledigt; `folder` = entfernt (Eintrag in
    /// der Zeitleiste), `None` = stehen gelassen.
    pub fn cleaned(&self, repo: &str, branch: &str, folder: Option<&str>) {
        let snap = (self.sh.deps.prs)();
        let now = (self.sh.deps.now)();
        let mut i = lock(&self.sh.inner);
        i.to_clean.retain(|(r, b, _)| !(r == repo && b == branch));
        if let Some(f) = folder {
            let key = snap.merged.iter().find(|m| m.repo == repo && m.head_ref == branch).map(|m| deck_core::dash::prs::pr_key(&m.repo, m.number));
            if let Some(key) = key {
                let did = deck_core::dash::feed_state::Did::WorktreeRemoved(f.to_string());
                i.st.record(super::feed_round::activity(&snap, &key, did, "", now));
                i.dirty = true;
            }
            self.sh.log.note(&format!("worktree {f} nach eigenem Merge entfernt"));
        }
        drop(i);
        self.sh.save();
    }

    /// Die Ansicht; gebaut nur bei neuem Stand oder nach [`VIEW_TTL_S`].
    pub fn view(&self, now: f64, lang: Lang) -> (FeedView, Option<CommentPopup>) {
        let snap = (self.sh.deps.prs)();
        let mut i = lock(&self.sh.inner);
        if let Some((v, fetched, at, l, view, popup)) = &i.view
            && *v == i.version
            && *fetched == snap.fetched_at
            && *l == lang
            && now - at < VIEW_TTL_S
        {
            return (view.clone(), popup.clone());
        }
        let (view, popup) = build(&FeedInput { snap: &snap, st: &i.st, causes: &i.causes, waiting: &i.waiting, now, lang });
        let version = i.version;
        i.view = Some((version, snap.fetched_at, now, lang, view.clone(), popup.clone()));
        (view, popup)
    }

    /// Das Gedächtnis (Tests).
    pub fn state(&self) -> FeedState {
        lock(&self.sh.inner).st.clone()
    }
}

impl Drop for FeedService {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Shared {
    /// Gedächtnis speichern, wenn es sich geändert hat; Ansicht neu bauen lassen.
    pub fn save(&self) {
        let json = {
            let mut i = lock(&self.inner);
            i.version += 1;
            if !i.dirty {
                return;
            }
            i.dirty = false;
            i.st.to_json()
        };
        if let Err(e) = deck_core::paths::atomic_write(&self.file, json.as_bytes()) {
            self.log.note(&format!("Gedächtnis nicht gespeichert: {}", e.kind()));
        }
    }

    pub fn notify(&self) {
        if let Some(cb) = lock(&self.wake).clone() {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cb()));
        }
    }

    pub fn runs_dir(&self) -> PathBuf {
        self.work_dir.join("runs")
    }

    pub fn drafts_dir(&self) -> PathBuf {
        self.work_dir.join("drafts")
    }
}

#[cfg(test)]
#[path = "feed_service_tests.rs"]
mod tests;
