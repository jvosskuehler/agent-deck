//! Datenquelle `cleanup`: gemergte worktrees in `%USERPROFILE%\Repos\wt\` finden und auf
//! Knopfdruck entfernen (Karte `deck_core::dash::cleanup`).
//!
//! SICHERHEIT (das Modul LÖSCHT worktrees und Branches): entfernt wird nur, was
//! [`deck_core::dash::cleanup::classify`] als entfernbar einstuft, und zwar beim Scan UND
//! noch einmal direkt vor dem Löschen. Dazu muss der Ordner ein verlinkter worktree sein
//! (`.git` ist eine Datei unter `…/.git/worktrees/…`). `git worktree remove` läuft ohne
//! `--force`: git selbst verweigert dann noch einmal jeden worktree mit Änderungen. Der
//! Branch geht mit `branch -D`, weil sein Kopf per Regel (b) gemergt ist (ein
//! Squash-Merge ist für git nicht „gemergt“, `-d` würde scheitern).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use deck_core::dash::cleanup::{CleanupOutcome, CleanupSnapshot, WtCandidate, WtState, classify, is_in_use, repo_from_url};
use deck_core::dash::prs::MergedPr;
use deck_core::git_head::{find_branch, parse_gitdir};

use super::hidden_cmd;
use crate::fsutil::now_unix;
use crate::git_fs::StdGitFs;
use crate::logfile::{LogFile, Rotate};
use crate::worktree::{Git, is_linked_worktree, list_child_dirs};

pub use super::hidden_cmd::TimedGit;

/// Scan-Takt (dazu auf Anstoß und bei neuen gemergten PRs oder Agenten).
pub const SCAN_EVERY: Duration = Duration::from_secs(300);
pub const LOG_FILE: &str = "dash-cleanup.log";

pub use super::ticker::Wake;

/// Haupt-Checkout aus der `.git`-Datei eines worktree: `gitdir: <main>/.git/worktrees/x`.
fn main_repo_of(wt: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(wt.join(".git")).ok()?;
    let gitdir = parse_gitdir(&text, wt)?;
    let worktrees = gitdir.parent()?;
    let dot_git = worktrees.parent()?;
    let ok = worktrees.file_name()? == "worktrees" && dot_git.file_name()?.eq_ignore_ascii_case(".git");
    ok.then(|| dot_git.parent().map(Path::to_path_buf)).flatten()
}

/// Was ein Scan braucht.
struct Inputs<'a> {
    git: &'a dyn Git,
    merged: &'a [MergedPr],
    in_use: &'a [String],
}

impl Inputs<'_> {
    fn origin(&self, main: &Path, cache: &mut HashMap<PathBuf, Option<String>>) -> Option<String> {
        cache
            .entry(main.to_path_buf())
            .or_insert_with(|| match self.git.run(&["config", "--get", "remote.origin.url"], main) {
                (0, url) => repo_from_url(&url),
                _ => None,
            })
            .clone()
    }

    /// Ein worktree als Kandidat; `None`, wenn er kein verlinkter worktree ist, kein
    /// Branch ausgecheckt ist oder sein Branch nicht als PR gemergt wurde.
    fn inspect(&self, dir: &Path, cache: &mut HashMap<PathBuf, Option<String>>) -> Option<WtCandidate> {
        if !is_linked_worktree(dir) {
            return None;
        }
        let main = main_repo_of(dir)?;
        let branch = find_branch(dir, &StdGitFs)?;
        let repo = self.origin(&main, cache)?;
        let oids: Vec<&str> = self
            .merged
            .iter()
            .filter(|m| m.head_ref == branch && m.repo.eq_ignore_ascii_case(&repo))
            .map(|m| m.head_oid.as_str())
            .collect();
        if oids.is_empty() {
            return None;
        }
        let path = dir.to_string_lossy().into_owned();
        let state = if is_in_use(&path, self.in_use) {
            WtState::InUse
        } else {
            let clean = match self.git.run(&["status", "--porcelain", "--untracked-files=normal"], dir) {
                (0, out) => Some(out.trim().is_empty()),
                _ => None,
            };
            let head = match self.git.run(&["rev-parse", "HEAD"], dir) {
                (0, out) => Some(out.trim().to_string()),
                _ => None,
            };
            classify(clean, head.as_deref(), &oids, false)
        };
        let folder = dir.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        Some(WtCandidate { path, folder, repo, main_repo: main.to_string_lossy().into_owned(), branch, state })
    }
}

#[derive(Default)]
struct Control {
    generation: u64,
    force: bool,
}

#[derive(Default)]
struct Inbox {
    merged: Vec<MergedPr>,
    in_use: Vec<String>,
}

struct State {
    git: Arc<dyn Git>,
    log: LogFile,
    wt_root: PathBuf,
    inbox: Mutex<Inbox>,
    snap: Mutex<CleanupSnapshot>,
    /// Scan und Löschen nie gleichzeitig.
    op: Mutex<()>,
    ctl: Mutex<Control>,
    cv: Condvar,
    wake: Mutex<Option<Wake>>,
}

/// Der Hintergrund-Dienst.
pub struct CleanupService {
    state: Arc<State>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn sorted_keys(paths: &[String]) -> Vec<String> {
    let mut v: Vec<String> = paths.iter().map(|p| deck_core::dash::cleanup::path_key(p)).collect();
    v.sort();
    v.dedup();
    v
}

impl CleanupService {
    /// `wt_root`: der Sammelordner der worktrees.
    pub fn new(git: Arc<dyn Git>, log: LogFile, wt_root: PathBuf) -> CleanupService {
        CleanupService {
            state: Arc::new(State {
                git,
                log,
                wt_root,
                inbox: Mutex::new(Inbox::default()),
                snap: Mutex::new(CleanupSnapshot::default()),
                op: Mutex::new(()),
                ctl: Mutex::new(Control::default()),
                cv: Condvar::new(),
                wake: Mutex::new(None),
            }),
            thread: Mutex::new(None),
        }
    }

    /// Die echte Fassung: `%USERPROFILE%\Repos\wt`, Protokoll `state\dash-cleanup.log`.
    pub fn system(state_dir: &Path) -> CleanupService {
        let home = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default();
        let log = LogFile::new(state_dir.join(LOG_FILE), 200_000, Rotate::Rename);
        CleanupService::new(Arc::new(TimedGit::default()), log, home.join("Repos").join("wt"))
    }

    /// Thread starten (erster Scan sofort); ein zweiter Aufruf ersetzt nur den Rückruf.
    pub fn start(&self, wake: Wake) {
        *lock(&self.state.wake) = Some(wake);
        let mut t = lock(&self.thread);
        if t.is_some() {
            return;
        }
        let generation = lock(&self.state.ctl).generation;
        let st = self.state.clone();
        *t = std::thread::Builder::new().name("dash-cleanup".into()).spawn(move || run(&st, generation)).ok();
    }

    pub fn stop(&self) {
        lock(&self.state.ctl).generation += 1;
        self.state.cv.notify_all();
        lock(&self.thread).take();
    }

    /// Im Hintergrund neu scannen.
    pub fn rescan(&self) {
        lock(&self.state.ctl).force = true;
        self.state.cv.notify_all();
    }

    /// Gemergte PRs aus dem GitHub-Stand; ändern sie sich, wird neu gescannt.
    pub fn set_merged(&self, merged: Vec<MergedPr>) {
        let changed = {
            let mut inbox = lock(&self.state.inbox);
            let changed = inbox.merged != merged;
            inbox.merged = merged;
            changed
        };
        if changed {
            self.rescan();
        }
    }

    /// Pfade, in denen offene Agenten arbeiten (cwd, worktree); bei Änderung neu scannen.
    pub fn set_in_use(&self, in_use: Vec<String>) {
        let changed = {
            let mut inbox = lock(&self.state.inbox);
            let changed = sorted_keys(&inbox.in_use) != sorted_keys(&in_use);
            inbox.in_use = in_use;
            changed
        };
        if changed {
            self.rescan();
        }
    }

    pub fn snapshot(&self) -> CleanupSnapshot {
        lock(&self.state.snap).clone()
    }

    /// Ein Scan, synchron.
    pub fn scan_now(&self) {
        scan(&self.state);
    }

    /// Alle entfernbaren worktrees im Hintergrund entfernen. `false`, wenn schon ein
    /// Lauf im Gang ist.
    pub fn remove_all(&self) -> bool {
        if !mark_busy(&self.state) {
            return false;
        }
        let st = self.state.clone();
        let spawned = std::thread::Builder::new().name("dash-cleanup-remove".into()).spawn(move || {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| remove(&st)));
            lock(&st.snap).busy = false;
            notify(&st);
        });
        if spawned.is_err() {
            lock(&self.state.snap).busy = false;
        }
        true
    }

    /// Dasselbe synchron (Tests): Ergebnis des Laufs, `None` wenn schon einer läuft.
    pub fn remove_all_now(&self) -> Option<CleanupOutcome> {
        if !mark_busy(&self.state) {
            return None;
        }
        let out = remove(&self.state);
        lock(&self.state.snap).busy = false;
        Some(out)
    }
}

impl Drop for CleanupService {
    fn drop(&mut self) {
        self.stop();
    }
}

fn mark_busy(st: &State) -> bool {
    let mut snap = lock(&st.snap);
    if snap.busy {
        return false;
    }
    snap.busy = true;
    drop(snap);
    notify(st);
    true
}

fn notify(st: &State) {
    if let Some(cb) = lock(&st.wake).clone() {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cb()));
    }
}

fn count(c: &[WtCandidate], s: WtState) -> usize {
    c.iter().filter(|x| x.state == s).count()
}

fn scan(st: &State) {
    let _op = lock(&st.op);
    let (merged, in_use) = {
        let inbox = lock(&st.inbox);
        (inbox.merged.clone(), inbox.in_use.clone())
    };
    let mut error = None;
    let mut candidates = Vec::new();
    if !merged.is_empty() && st.wt_root.is_dir() {
        if std::fs::read_dir(&st.wt_root).is_err() {
            error = Some("Ordner wt nicht lesbar".to_string());
        }
        let inputs = Inputs { git: st.git.as_ref(), merged: &merged, in_use: &in_use };
        let mut cache = HashMap::new();
        candidates = list_child_dirs(&st.wt_root).iter().filter_map(|d| inputs.inspect(d, &mut cache)).collect();
    }
    let mut snap = lock(&st.snap);
    let before = (count(&snap.candidates, WtState::Removable), count(&snap.candidates, WtState::Changed));
    let after = (count(&candidates, WtState::Removable), count(&candidates, WtState::Changed));
    if before != after || snap.error != error {
        st.log.note(&format!("Scan: {} entfernbar, {} gemergt mit Aenderungen{}", after.0, after.1, error.as_deref().map(|e| format!(", Fehler: {e}")).unwrap_or_default()));
    }
    snap.candidates = candidates;
    snap.scanned_at = now_unix();
    snap.error = error;
}

/// Ein Kandidat direkt vor dem Löschen: noch derselbe Branch und weiter entfernbar?
fn recheck(st: &State, c: &WtCandidate) -> Result<(), &'static str> {
    let (merged, in_use) = {
        let inbox = lock(&st.inbox);
        (inbox.merged.clone(), inbox.in_use.clone())
    };
    let inputs = Inputs { git: st.git.as_ref(), merged: &merged, in_use: &in_use };
    let now = inputs.inspect(Path::new(&c.path), &mut HashMap::new()).ok_or("nicht mehr gemergt oder kein worktree")?;
    if now.branch != c.branch || now.main_repo != c.main_repo {
        return Err("Branch gewechselt");
    }
    match now.state {
        WtState::Removable => Ok(()),
        WtState::Changed => Err("Aenderungen oder Kopf nicht gemergt"),
        WtState::InUse => Err("Agent arbeitet darin"),
    }
}

fn remove(st: &State) -> CleanupOutcome {
    remove_some(st, None)
}

/// Wie [`remove`], aber nur die worktrees dieser (Repo, Branch)-Paare (`None` = alle).
fn remove_some(st: &State, only: Option<&[(String, String)]>) -> CleanupOutcome {
    let wanted = |c: &WtCandidate| only.is_none_or(|o| o.iter().any(|(r, b)| r.eq_ignore_ascii_case(&c.repo) && *b == c.branch));
    let todo: Vec<WtCandidate> = lock(&st.snap).removable().filter(|c| wanted(c)).cloned().collect();
    let mut out = CleanupOutcome::default();
    {
        let _op = lock(&st.op);
        for c in &todo {
            if let Err(why) = recheck(st, c) {
                st.log.note(&format!("stehen gelassen ({why}): {}", c.folder));
                out.skipped += 1;
                continue;
            }
            let main = Path::new(&c.main_repo);
            let (rc, msg) = st.git.run(&["worktree", "remove", &c.path], main);
            if rc != 0 || Path::new(&c.path).exists() {
                st.log.note(&format!("worktree remove gescheitert (Exit {rc}): {} ({})", c.folder, hidden_cmd::short_error(&msg)));
                out.failed += 1;
                continue;
            }
            out.removed += 1;
            let (rc, msg) = st.git.run(&["branch", "-D", &c.branch], main);
            if rc == 0 {
                st.log.note(&format!("entfernt: {} samt Branch {}", c.folder, c.branch));
            } else {
                st.log.note(&format!("entfernt: {}, Branch {} blieb (Exit {rc}: {})", c.folder, c.branch, hidden_cmd::short_error(&msg)));
                out.failed += 1;
            }
        }
    }
    st.log.note(&format!("Lauf fertig: {} entfernt, {} stehen gelassen, {} fehlgeschlagen", out.removed, out.skipped, out.failed));
    lock(&st.snap).last_result = Some(out);
    scan(st);
    out
}

fn run(st: &State, generation: u64) {
    loop {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scan(st)));
        notify(st);
        let mut ctl = lock(&st.ctl);
        if ctl.generation == generation && !ctl.force {
            ctl = st.cv.wait_timeout(ctl, SCAN_EVERY).map(|(g, _)| g).unwrap_or_else(|e| e.into_inner().0);
        }
        if ctl.generation != generation {
            return;
        }
        ctl.force = false;
    }
}

#[path = "cleanup_targeted.rs"]
mod targeted;

#[cfg(test)]
#[path = "cleanup_tests.rs"]
mod tests;
