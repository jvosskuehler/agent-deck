//! Verwaiste git worktrees abräumen: beim Schließen eines Agenten und periodisch
//! (Python `ui/worktree_sweep.py`).
//!
//! Zwei Wege, weil ein Marker verloren gehen kann:
//! * **Marker-Sweep:** `state\<slot>.worktree` nennt den worktree; lebt der Agent dazu
//!   nicht mehr (nach einer Schonfrist gegen Aussetzer der Terminal-Liste), wird er über
//!   dieselbe sichere Maschinerie entfernt wie beim Schließen.
//! * **Platten-Sweep:** etwa minütlich die `<repo>.wt\`-Ordner der bekannten Repos
//!   absuchen, nach worktrees ohne lebenden Agenten, auch ohne Marker.
//!
//! Beide schützen fremde Slots: ein worktree, den ein anderer lebender Agent beansprucht
//! (per Marker oder per Ticket-Slug als Ordnername), wird nie angefasst. Ein gebundenes,
//! aber gerade getrenntes Fenster (Reload, kurzer Abriss) gilt nicht als tot. Und
//! solange der VS-Code-Neustart läuft (`Restarter::busy`), ruht alles: dann lebt zu
//! keinem Slot ein Agent, und jeder Ticket-worktree sähe verwaist aus.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use deck_core::binding::{ticket_branch, ticket_slug};
use deck_core::config::{WT_DISK_ORPHAN_GRACE_S, WT_DISK_SWEEP_INTERVAL_S, WT_ORPHAN_GRACE_S};
use deck_core::paths::{found_ticket_path, worktree_marker_path};
use serde_json::Value;

use crate::fsutil::{basename, dirname, normcase, normpath};
use crate::worktree::{WorktreeOps, is_linked_worktree, list_child_dirs, repo_root_from_wt_dir, wt_dir_for_repo};

/// Was das Deck in diesem Poll weiß (alles Slot-bezogene nach Slot-Namen, `A1` …).
#[derive(Debug, Clone, Default)]
pub struct SweepView {
    pub now: f64,
    /// `state\<slot>.worktree`: gemeldete worktree-Pfade.
    pub markers: BTreeMap<String, String>,
    /// `cwd` aus `state\<slot>.json` (die Repo-Wurzel des Agenten).
    pub cwds: BTreeMap<String, String>,
    /// Zugewiesenes oder vom Agenten gefundenes Ticket.
    pub tickets: BTreeMap<String, String>,
    /// Verbundene Fenster (Buchstaben).
    pub connected: BTreeSet<String>,
    /// Lebende Slots: die Terminals aller verbundenen Fenster.
    pub live: BTreeSet<String>,
    /// Fenster → gebundener Repo-Name, auch während es getrennt ist.
    pub bindings: BTreeMap<String, String>,
}

fn window_of(slot: &str) -> String {
    slot.chars().next().map(String::from).unwrap_or_default()
}

impl SweepView {
    fn bound_but_disconnected(&self, win: &str) -> bool {
        !self.connected.contains(win) && self.bindings.get(win).is_some_and(|b| !b.is_empty())
    }
}

// ── Beim Schließen eines Agenten ────────────────────────────────────────────

/// Was nach dem Schließen eines Agenten entfernt werden soll. Auf dem UI-Thread werden
/// nur die billigen Signale eingesammelt, solange Ticket und Marker noch da sind; das
/// Entfernen samt der blockierenden `git worktree list`-Suche läuft im Hintergrund.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupJob {
    /// Exakter Pfad aus dem Marker.
    pub exact: Option<String>,
    /// Ticket-Branch für den Rückfall (nur zusammen mit `repo`).
    pub branch: String,
    pub repo: Option<String>,
    /// Pfade, die ANDERE Slots per Marker beanspruchen (normalisiert): tabu. Zwei Agenten
    /// am selben Ticket teilen sich einen worktree (git erlaubt den Branch nur einmal).
    pub others: BTreeSet<String>,
}

/// Den Auftrag bauen; `None` = nichts bekannt, was zu entfernen wäre.
pub fn cleanup_job(slot: &str, markers: &BTreeMap<String, String>, ticket: Option<&str>, cwd: Option<&str>) -> Option<CleanupJob> {
    let ticket = ticket.filter(|t| !t.is_empty());
    let exact = markers.get(slot).filter(|p| !p.is_empty()).cloned();
    let branch = ticket.map(|t| ticket_branch(t, None)).unwrap_or_default();
    let repo = ticket.and(cwd).filter(|c| !c.is_empty()).map(String::from);
    let others = markers.iter().filter(|(s, p)| s.as_str() != slot && !p.is_empty()).map(|(_, p)| normpath(p)).collect();
    (exact.is_some() || (!branch.is_empty() && repo.is_some())).then_some(CleanupJob { exact, branch, repo, others })
}

/// Den Auftrag ausführen (blockierend). Der Branch-Rückfall läuft NUR, wenn der Marker
/// nichts lieferte: das spart die Abfrage und trifft nicht den worktree eines anderen
/// Agenten am selben Branch.
pub fn run_cleanup(job: &CleanupJob, ops: &WorktreeOps) {
    let mut paths: Vec<(String, String)> = Vec::new();
    let mut add = |p: Option<String>| {
        if let Some(p) = p.filter(|p| !p.is_empty()) {
            let n = normpath(&p);
            if !paths.iter().any(|(k, _)| *k == n) {
                paths.push((n, p));
            }
        }
    };
    add(job.exact.clone());
    if job.exact.is_none()
        && let Some(repo) = &job.repo
        && !job.branch.is_empty()
    {
        add(ops.worktree_for_branch(repo, &job.branch));
    }
    for (norm, p) in paths {
        if job.others.contains(&norm) {
            ops.note(&format!("uebersprungen (noch von anderem Slot beansprucht): {p}"));
            continue;
        }
        ops.remove_worktree(&p);
    }
}

// ── Marker-Sweep ────────────────────────────────────────────────────────────

/// Schonfrist-Uhren des Marker-Sweeps.
#[derive(Debug, Default)]
pub struct MarkerSweep {
    gone_since: HashMap<String, f64>,
}

impl MarkerSweep {
    /// Slots, deren worktree jetzt verwaist ist (Agent tot, Schonfrist um). Ein gebundenes,
    /// aber getrenntes Fenster ist nicht unseres: das räumt der Fenster-Weg ab, sobald es
    /// wirklich zu ist, sonst fiele ein worktree bei jedem Reload.
    pub fn orphans(&mut self, v: &SweepView) -> Vec<String> {
        if v.markers.is_empty() {
            self.gone_since.clear();
            return Vec::new();
        }
        let mut out = Vec::new();
        for slot in v.markers.keys() {
            if v.live.contains(slot) || v.bound_but_disconnected(&window_of(slot)) {
                self.gone_since.remove(slot);
                continue;
            }
            match self.gone_since.get(slot) {
                None => {
                    self.gone_since.insert(slot.clone(), v.now);
                }
                Some(t0) if v.now - t0 >= WT_ORPHAN_GRACE_S => out.push(slot.clone()),
                Some(_) => {}
            }
        }
        for slot in &out {
            self.gone_since.remove(slot);
        }
        self.gone_since.retain(|s, _| v.markers.contains_key(s));
        out
    }
}

// ── Platten-Sweep ───────────────────────────────────────────────────────────

/// Unveränderlicher Schnappschuss für einen Platten-Durchgang im Hintergrund.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DiskJob {
    pub now: f64,
    pub repos: Vec<String>,
    /// Repo-Namen (klein) gebundener, gerade getrennter Fenster: diesen Lauf auslassen.
    pub disc_names: BTreeSet<String>,
    /// Repo-Wurzeln (normcase) gebundener, getrennter Fenster: ebenso.
    pub skip_roots: BTreeSet<String>,
    /// Besitz lebender Slots: Marker-Pfade und Ticket-Slugs (beides normcase; Windows
    /// unterscheidet keine Groß-/Kleinschreibung, der Ordner kann anders heißen als der
    /// stets kleine Slug).
    pub owned_paths: BTreeSet<String>,
    pub owned_slugs: BTreeSet<String>,
}

/// Zustand des Platten-Sweeps über die Polls hinweg.
#[derive(Debug, Default)]
pub struct DiskSweep {
    /// Einmal gesehen, bleibt ein Repo für die Sitzung im Blick: auch nach dem Schließen
    /// seines letzten Agenten wird noch aufgeräumt.
    known_repos: BTreeSet<String>,
    last: Option<f64>,
    busy: Arc<AtomicBool>,
    gone_since: Arc<Mutex<HashMap<String, f64>>>,
}

impl DiskSweep {
    /// Läuft gerade ein Durchgang im Hintergrund?
    pub fn busy(&self) -> bool {
        self.busy.load(Ordering::SeqCst)
    }

    /// Fällig? Dann den Schnappschuss bauen und `busy` setzen (nie zwei parallel).
    pub fn plan(&mut self, v: &SweepView) -> Option<DiskJob> {
        if self.busy() || self.last.is_some_and(|t| v.now - t < WT_DISK_SWEEP_INTERVAL_S) {
            return None;
        }
        self.known_repos.extend(v.cwds.values().filter(|c| !c.is_empty()).map(|c| normpath(c)));
        self.known_repos.extend(v.markers.values().filter_map(|p| repo_root_from_wt_dir(dirname(&normpath(p)))));
        if self.known_repos.is_empty() {
            return None;
        }
        let owned_paths = v.markers.iter().filter(|(s, p)| v.live.contains(*s) && !p.is_empty()).map(|(_, p)| normcase(p)).collect();
        let owned_slugs = v
            .live
            .iter()
            .filter_map(|s| v.tickets.get(s))
            .map(|t| ticket_slug(t))
            .filter(|s| !s.is_empty())
            .map(|s| s.to_lowercase())
            .collect();
        let bound_disc: BTreeSet<String> = v.bindings.keys().filter(|w| v.bound_but_disconnected(w)).cloned().collect();
        let disc_names = bound_disc.iter().filter_map(|w| v.bindings.get(w)).map(|n| n.to_lowercase()).collect();
        let mut skip_roots = BTreeSet::new();
        let roots = v.cwds.iter().map(|(s, c)| (s, Some(c.clone())));
        let marker_roots = v.markers.iter().map(|(s, p)| (s, repo_root_from_wt_dir(dirname(&normpath(p)))));
        for (slot, root) in roots.chain(marker_roots) {
            if let Some(root) = root.filter(|r| !r.is_empty())
                && bound_disc.contains(&window_of(slot))
            {
                skip_roots.insert(normcase(&root));
            }
        }
        self.last = Some(v.now);
        self.busy.store(true, Ordering::SeqCst);
        Some(DiskJob {
            now: v.now,
            repos: self.known_repos.iter().cloned().collect(),
            disc_names,
            skip_roots,
            owned_paths,
            owned_slugs,
        })
    }

    /// Einen geplanten Durchgang ausführen (blockierend) und `busy` wieder freigeben.
    pub fn run(&self, job: &DiskJob, ops: &WorktreeOps) {
        let mut gone = self.gone_since.lock().unwrap_or_else(|e| e.into_inner());
        run_disk_job(job, &mut gone, ops);
        self.busy.store(false, Ordering::SeqCst);
    }

    fn shared(&self) -> (Arc<AtomicBool>, Arc<Mutex<HashMap<String, f64>>>) {
        (self.busy.clone(), self.gone_since.clone())
    }
}

/// Ein Platten-Durchgang. Übersprungene Repos (Reload) kommen NICHT in `examined`, ihre
/// Uhren bleiben also stehen (pausiert, nicht zurückgesetzt): sonst könnte ein dauernd
/// flackerndes Fenster die Schonfrist ewig neu starten.
pub fn run_disk_job(job: &DiskJob, gone: &mut HashMap<String, f64>, ops: &WorktreeOps) {
    let mut seen = BTreeSet::new();
    let mut examined = BTreeSet::new();
    for root in &job.repos {
        if job.skip_roots.contains(&normcase(root)) || job.disc_names.contains(&basename(root).to_lowercase()) {
            continue;
        }
        let Some(wt_dir) = wt_dir_for_repo(root) else { continue };
        examined.insert(normcase(&wt_dir));
        for d in list_child_dirs(Path::new(&wt_dir)) {
            let ds = d.to_string_lossy().into_owned();
            let nd = normcase(&ds);
            let slug = basename(&normpath(&ds)).to_lowercase();
            if job.owned_paths.contains(&nd) || job.owned_slugs.contains(&slug) {
                gone.remove(&nd);
                continue;
            }
            seen.insert(nd.clone());
            match gone.get(&nd) {
                None => {
                    gone.insert(nd, job.now);
                }
                Some(t0) if job.now - t0 >= WT_DISK_ORPHAN_GRACE_S => {
                    if remove_orphan_worktree(&d, root, ops) {
                        gone.remove(&nd);
                    }
                }
                Some(_) => {}
            }
        }
    }
    gone.retain(|nd, _| !(examined.contains(dirname(nd)) && !seen.contains(nd)));
}

/// Einen als verwaist erkannten Ordner über den passenden sicheren Weg entfernen:
/// verlinkter worktree → `remove_worktree`; ein Rest GANZ OHNE `.git` →
/// `remove_orphan_dir`; alles andere (echter Checkout, Submodul) bleibt tabu.
pub fn remove_orphan_worktree(path: &Path, repo: &str, ops: &WorktreeOps) -> bool {
    let p = path.to_string_lossy();
    if is_linked_worktree(path) {
        return ops.remove_worktree(&p);
    }
    if !path.join(".git").exists() {
        return ops.remove_orphan_dir(&p, Some(repo));
    }
    ops.note(&format!("uebersprungen (fremdes .git, kein verwaister worktree): {p}"));
    false
}

// ── Verdrahtung ─────────────────────────────────────────────────────────────

/// Beide Wege samt Hintergrund-Threads, so wie die App sie braucht.
pub struct WorktreeSweeper {
    ops: Arc<WorktreeOps>,
    state_dir: PathBuf,
    marker: MarkerSweep,
    disk: DiskSweep,
}

impl WorktreeSweeper {
    pub fn new(ops: Arc<WorktreeOps>, state_dir: PathBuf) -> WorktreeSweeper {
        WorktreeSweeper { ops, state_dir, marker: MarkerSweep::default(), disk: DiskSweep::default() }
    }

    pub fn system(state_dir: &Path) -> WorktreeSweeper {
        WorktreeSweeper::new(Arc::new(WorktreeOps::system(state_dir)), state_dir.to_path_buf())
    }

    /// Ein Agent wird geschlossen (Python `_cleanup_worktrees`): Marker und cwd frisch
    /// lesen, Auftrag bauen, den Marker löschen, entfernen im Hintergrund. `ticket` ist
    /// das zugewiesene oder gefundene Ticket des Slots. `true` = es lief ein Auftrag an.
    pub fn cleanup_slot(&self, slot: &str, ticket: Option<&str>) -> bool {
        let markers: BTreeMap<String, String> = deck_core::slot_state::read_found_worktrees(&self.state_dir).into_iter().collect();
        let cwd = deck_core::slot_state::read_all(&self.state_dir)
            .get(slot)
            .and_then(|st| st.get("cwd").and_then(Value::as_str).map(String::from));
        let job = cleanup_job(slot, &markers, ticket, cwd.as_deref());
        let _ = std::fs::remove_file(worktree_marker_path(&self.state_dir, slot));
        let Some(job) = job else { return false };
        let ops = self.ops.clone();
        std::thread::Builder::new().name("worktree-cleanup".into()).spawn(move || run_cleanup(&job, &ops)).is_ok()
    }

    /// Im Poll: Marker-Sweep, dann Platten-Sweep. Rückgabe: die verwaisten Slots, deren
    /// Ticket-Zuweisung die App jetzt vergisst (die Tickets gehören ihr). Marker und
    /// gefundene Ticket-ID dieser Slots sind dann schon gelöscht. `paused` = der
    /// VS-Code-Neustart läuft: dann geschieht nichts.
    pub fn tick(&mut self, view: &SweepView, paused: bool) -> Vec<String> {
        if paused {
            return Vec::new();
        }
        let orphans = self.marker.orphans(view);
        for slot in &orphans {
            self.cleanup_slot(slot, view.tickets.get(slot).map(String::as_str));
            let _ = std::fs::remove_file(found_ticket_path(&self.state_dir, slot));
        }
        // Den Schnappschuss dieses Polls angleichen: die Marker der Waisen sind weg.
        let mut v = view.clone();
        for slot in &orphans {
            v.markers.remove(slot);
            v.tickets.remove(slot);
        }
        if let Some(job) = self.disk.plan(&v) {
            let (busy, gone) = self.disk.shared();
            let ops = self.ops.clone();
            let spawned = std::thread::Builder::new().name("worktree-disk-sweep".into()).spawn(move || {
                let mut g = gone.lock().unwrap_or_else(|e| e.into_inner());
                run_disk_job(&job, &mut g, &ops);
                busy.store(false, Ordering::SeqCst);
            });
            if spawned.is_err() {
                self.disk.busy.store(false, Ordering::SeqCst);
            }
        }
        orphans
    }

    /// Läuft gerade ein Platten-Durchgang?
    pub fn disk_busy(&self) -> bool {
        self.disk.busy()
    }
}

#[cfg(test)]
#[path = "worktree_sweep_tests.rs"]
mod tests;
