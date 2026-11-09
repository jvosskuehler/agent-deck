//! Tests zu `worktree_sweep.rs`. Die Python-Fassung hatte für die Sweep-Regeln keine
//! eigenen Tests; hier sind sie Regel für Regel festgenagelt. Gelöscht wird nur in
//! Wegwerf-Ordnern und Wegwerf-Repos.

use std::time::Duration;

use super::*;
use crate::logfile::{LogFile, Rotate};
use crate::worktree::tests::{real, repo_with_worktree};
use crate::worktree::{GitCli, LOG_FILE};

fn ops(dir: &Path) -> WorktreeOps {
    WorktreeOps::new(Arc::new(GitCli), LogFile::new(dir.join(LOG_FILE), 200_000, Rotate::Truncate), vec![Duration::ZERO])
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn map(items: &[(&str, &str)]) -> BTreeMap<String, String> {
    items.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

// ── Marker-Sweep ────────────────────────────────────────────────────────────

#[test]
fn a_dead_agents_worktree_is_orphaned_only_after_the_grace_period() {
    let mut s = MarkerSweep::default();
    let mut v = SweepView { now: 100.0, markers: map(&[("A1", r"C:\r.wt\x")]), connected: set(&["A"]), ..Default::default() };
    assert!(s.orphans(&v).is_empty(), "erstmals gesehen: Uhr läuft an");
    v.now += WT_ORPHAN_GRACE_S - 1.0;
    assert!(s.orphans(&v).is_empty());
    v.now += 1.0;
    assert_eq!(s.orphans(&v), ["A1"]);
}

#[test]
fn a_living_agent_stops_the_clock() {
    let mut s = MarkerSweep::default();
    let mut v = SweepView { now: 0.0, markers: map(&[("A1", r"C:\r.wt\x")]), connected: set(&["A"]), ..Default::default() };
    s.orphans(&v);
    v.live = set(&["A1"]);
    v.now = 15.0;
    assert!(s.orphans(&v).is_empty());
    v.live.clear();
    v.now = 30.0;
    assert!(s.orphans(&v).is_empty(), "die Uhr beginnt neu, nicht bei 0");
}

#[test]
fn a_bound_but_disconnected_window_is_not_ours_to_sweep() {
    // Reload oder kurzer Abriss: das native Fenster ist noch offen.
    let mut s = MarkerSweep::default();
    let mut v = SweepView { now: 0.0, markers: map(&[("A1", r"C:\r.wt\x")]), bindings: map(&[("A", "repo")]), ..Default::default() };
    for t in [0.0, 100.0, 1000.0] {
        v.now = t;
        assert!(s.orphans(&v).is_empty());
    }
}

// ── Schließen ───────────────────────────────────────────────────────────────

#[test]
fn the_cleanup_job_takes_the_marker_and_protects_other_slots_claims() {
    let markers = map(&[("A1", "C:/r.wt/abc-1"), ("A2", "C:/r.wt/abc-1/"), ("B1", "")]);
    let job = cleanup_job("A1", &markers, None, None).unwrap();
    assert_eq!(job.exact.as_deref(), Some("C:/r.wt/abc-1"));
    assert_eq!(job.others, set(&[r"C:\r.wt\abc-1"]));
    assert_eq!((job.branch.as_str(), job.repo), ("", None), "ohne Ticket kein Branch-Rückfall");
}

#[test]
fn the_branch_fallback_needs_ticket_and_cwd() {
    let none = BTreeMap::new();
    assert_eq!(cleanup_job("A1", &none, None, Some(r"C:\r")), None);
    assert_eq!(cleanup_job("A1", &none, Some("ABC-1"), None), None);
    let job = cleanup_job("A1", &none, Some("ABC-1"), Some(r"C:\r")).unwrap();
    assert_eq!((job.branch.as_str(), job.repo.as_deref()), ("ticket/abc-1", Some(r"C:\r")));
}

#[test]
fn a_worktree_claimed_by_another_slot_survives_the_cleanup() {
    let base = tempfile::tempdir().unwrap();
    let (_repo, wt) = repo_with_worktree(base.path());
    let p = wt.to_string_lossy().into_owned();
    let job = cleanup_job("A1", &map(&[("A1", &p), ("A2", &p)]), None, None).unwrap();
    run_cleanup(&job, &ops(base.path()));
    assert!(wt.is_dir());
}

#[test]
fn without_a_marker_the_ticket_branch_finds_the_worktree() {
    let base = tempfile::tempdir().unwrap();
    let (repo, wt) = repo_with_worktree(base.path());
    let job = cleanup_job("A1", &BTreeMap::new(), Some("ABC-123"), Some(&repo.to_string_lossy())).unwrap();
    run_cleanup(&job, &ops(base.path()));
    assert!(!wt.exists());
}

// ── Platten-Sweep ───────────────────────────────────────────────────────────

#[test]
fn the_disk_sweep_runs_at_most_once_a_minute_and_never_twice_at_once() {
    let mut d = DiskSweep::default();
    let v = SweepView { now: 1000.0, cwds: map(&[("A1", r"C:\r")]), ..Default::default() };
    let job = d.plan(&v).expect("erster Lauf fällig");
    assert_eq!(job.repos, [r"C:\r"]);
    assert!(d.plan(&SweepView { now: 2000.0, ..v.clone() }).is_none(), "läuft noch");
    d.busy.store(false, Ordering::SeqCst);
    assert!(d.plan(&SweepView { now: 1000.0 + WT_DISK_SWEEP_INTERVAL_S - 1.0, ..v.clone() }).is_none());
    assert!(d.plan(&SweepView { now: 1000.0 + WT_DISK_SWEEP_INTERVAL_S, ..v }).is_some());
}

#[test]
fn known_repos_come_from_cwds_and_markers_and_stay_for_the_session() {
    let mut d = DiskSweep::default();
    let v = SweepView { now: 0.0, markers: map(&[("B1", r"C:\x\beta.wt\slug")]), cwds: map(&[("A1", "C:/x/alpha/")]), ..Default::default() };
    assert_eq!(d.plan(&v).unwrap().repos, [r"C:\x\alpha", r"C:\x\beta"]);
    d.busy.store(false, Ordering::SeqCst);
    let later = SweepView { now: 100.0, ..Default::default() };
    assert_eq!(d.plan(&later).unwrap().repos.len(), 2, "auch ohne Agenten weiter im Blick");
}

#[test]
fn ownership_and_reload_protection_land_in_the_snapshot() {
    let mut d = DiskSweep::default();
    let v = SweepView {
        now: 0.0,
        markers: map(&[("A1", r"C:\x\alpha.wt\Mine"), ("B1", r"C:\x\beta.wt\theirs")]),
        cwds: map(&[("A1", r"C:\x\alpha"), ("B1", r"C:\x\beta")]),
        tickets: map(&[("A2", "ABC-7: Login")]),
        connected: set(&["A"]),
        live: set(&["A1", "A2"]),
        bindings: map(&[("A", "alpha"), ("B", "Beta")]),
    };
    let job = d.plan(&v).unwrap();
    assert_eq!(job.owned_paths, set(&[r"c:\x\alpha.wt\mine"]));
    assert_eq!(job.owned_slugs, set(&["abc-7-login"]));
    assert_eq!(job.disc_names, set(&["beta"]));
    assert_eq!(job.skip_roots, set(&[r"c:\x\beta"]));
}

fn disk_job(root: &Path, now: f64) -> DiskJob {
    DiskJob { now, repos: vec![root.to_string_lossy().into_owned()], ..Default::default() }
}

#[test]
fn an_orphan_folder_falls_after_the_grace_and_a_real_checkout_never() {
    let base = tempfile::tempdir().unwrap();
    let root = base.path().join("repo");
    let wt = base.path().join("repo.wt");
    std::fs::create_dir_all(wt.join("rest")).unwrap();
    std::fs::create_dir_all(wt.join("clone").join(".git")).unwrap();
    let o = ops(base.path());
    let mut gone = HashMap::new();
    run_disk_job(&disk_job(&root, 0.0), &mut gone, &o);
    assert!(wt.join("rest").is_dir(), "erstmals gesehen: nur die Uhr");
    run_disk_job(&disk_job(&root, WT_DISK_ORPHAN_GRACE_S), &mut gone, &o);
    assert!(!wt.join("rest").exists());
    assert!(wt.join("clone").is_dir(), "ein .git-Ordner ist tabu");
}

#[test]
fn a_folder_named_after_a_living_agents_ticket_is_left_alone() {
    let base = tempfile::tempdir().unwrap();
    let root = base.path().join("repo");
    std::fs::create_dir_all(base.path().join("repo.wt").join("ABC-7")).unwrap();
    let o = ops(base.path());
    let mut gone = HashMap::new();
    let job = |now| DiskJob { owned_slugs: set(&["abc-7"]), ..disk_job(&root, now) };
    run_disk_job(&job(0.0), &mut gone, &o);
    run_disk_job(&job(1000.0), &mut gone, &o);
    assert!(base.path().join("repo.wt").join("ABC-7").is_dir());
    assert!(gone.is_empty());
}

#[test]
fn a_skipped_repo_keeps_its_clocks_paused_and_vanished_folders_lose_theirs() {
    let base = tempfile::tempdir().unwrap();
    let root = base.path().join("repo");
    let wt = base.path().join("repo.wt");
    std::fs::create_dir_all(wt.join("a")).unwrap();
    let o = ops(base.path());
    let mut gone = HashMap::new();
    run_disk_job(&disk_job(&root, 0.0), &mut gone, &o);
    assert_eq!(gone.len(), 1);
    let skipped = DiskJob { skip_roots: set(&[&normcase(&root.to_string_lossy())]), ..disk_job(&root, 10.0) };
    run_disk_job(&skipped, &mut gone, &o);
    assert_eq!(gone.len(), 1, "Reload: Uhr pausiert, nicht gelöscht");
    std::fs::remove_dir(wt.join("a")).unwrap();
    run_disk_job(&disk_job(&root, 20.0), &mut gone, &o);
    assert!(gone.is_empty(), "Ordner weg: Uhr weg");
}

#[test]
fn a_linked_orphan_worktree_is_removed_through_git_after_the_grace() {
    let base = tempfile::tempdir().unwrap();
    let (repo, wt) = repo_with_worktree(base.path());
    let o = ops(base.path());
    let mut gone = HashMap::new();
    run_disk_job(&disk_job(&repo, 0.0), &mut gone, &o);
    run_disk_job(&disk_job(&repo, WT_DISK_ORPHAN_GRACE_S), &mut gone, &o);
    assert!(!wt.exists());
    assert_eq!(o.worktrees(&repo).len(), 1);
}

// ── Verdrahtung ─────────────────────────────────────────────────────────────

#[test]
fn an_orphaned_slot_loses_its_marker_and_found_ticket() {
    let base = tempfile::tempdir().unwrap();
    let state = real(base.path()).join("state");
    std::fs::create_dir_all(&state).unwrap();
    let wt = real(base.path()).join("repo.wt").join("weg");
    std::fs::write(worktree_marker_path(&state, "A1"), wt.to_string_lossy().as_bytes()).unwrap();
    std::fs::write(found_ticket_path(&state, "A1"), "ABC-1").unwrap();
    let mut sw = WorktreeSweeper::new(Arc::new(ops(base.path())), state.clone());
    let v = SweepView { now: 0.0, markers: map(&[("A1", &wt.to_string_lossy())]), ..Default::default() };
    assert!(sw.tick(&v).is_empty());
    assert_eq!(sw.tick(&SweepView { now: WT_ORPHAN_GRACE_S, ..v }), ["A1"]);
    assert!(!worktree_marker_path(&state, "A1").exists());
    assert!(!found_ticket_path(&state, "A1").exists());
}
