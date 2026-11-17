//! Tests zu `cleanup.rs`: echte Ordner im Temp-Verzeichnis (worktree-Dateien wie von git
//! angelegt), git als Attrappe. Kein Test startet git oder löscht außerhalb des Temp-Ordners.

use super::*;
use deck_core::dash::cleanup::CleanupOutcome;

const ORIGIN: &str = "https://github.com/Org/orpheus-management-ui.git\n";
const REPO: &str = "org/ORPHEUS-management-ui"; // GitHub-Schreibweise darf abweichen

/// Zustand eines worktree, wie die Attrappe ihn meldet.
#[derive(Clone)]
struct Wt {
    clean: bool,
    head: String,
}

/// git als Attrappe: antwortet je Ordnername, „entfernt“ worktrees, indem sie den Ordner
/// löscht (nur im Temp-Ordner), und merkt sich jeden Aufruf.
#[derive(Default)]
struct FakeGit {
    wts: Mutex<HashMap<String, Wt>>,
    calls: Mutex<Vec<String>>,
    fail_remove: Mutex<bool>,
}

impl FakeGit {
    fn set(&self, folder: &str, clean: bool, head: &str) {
        lock(&self.wts).insert(folder.into(), Wt { clean, head: head.into() });
    }

    fn calls_with(&self, needle: &str) -> Vec<String> {
        lock(&self.calls).iter().filter(|c| c.contains(needle)).cloned().collect()
    }
}

impl Git for FakeGit {
    fn run(&self, args: &[&str], cwd: &Path) -> (i32, String) {
        let folder = cwd.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        lock(&self.calls).push(format!("{folder}: {}", args.join(" ")));
        let wt = lock(&self.wts).get(&folder).cloned();
        match args {
            ["config", "--get", "remote.origin.url"] => (0, ORIGIN.into()),
            ["status", ..] => wt.map_or((128, "kein Repo".into()), |w| (0, if w.clean { String::new() } else { " M a.txt\n".into() })),
            ["rev-parse", "HEAD"] => wt.map_or((128, "kein Repo".into()), |w| (0, format!("{}\n", w.head))),
            ["worktree", "remove", path] => {
                assert!(!args.contains(&"--force"), "nie --force");
                if *lock(&self.fail_remove) {
                    return (1, "fatal: cannot remove".into());
                }
                std::fs::remove_dir_all(path).unwrap();
                (0, String::new())
            }
            ["branch", "-D", _] => (0, String::new()),
            _ => (1, "unbekannt".into()),
        }
    }
}

/// Haupt-Repo `main` mit worktree-Verwaltung und dem Sammelordner `wt`.
struct Lab {
    dir: tempfile::TempDir,
    git: Arc<FakeGit>,
    svc: CleanupService,
}

impl Lab {
    fn new() -> Lab {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("main").join(".git").join("worktrees")).unwrap();
        std::fs::create_dir_all(dir.path().join("wt")).unwrap();
        let git = Arc::new(FakeGit::default());
        let log = LogFile::new(dir.path().join(LOG_FILE), 100_000, Rotate::Truncate);
        let svc = CleanupService::new(git.clone(), log, dir.path().join("wt"));
        Lab { dir, git, svc }
    }

    fn wt_path(&self, folder: &str) -> PathBuf {
        self.dir.path().join("wt").join(folder)
    }

    /// worktree `wt/<folder>` auf `branch`, wie `git worktree add` ihn anlegt.
    fn add(&self, folder: &str, branch: &str, clean: bool, head: &str) {
        let meta = self.dir.path().join("main").join(".git").join("worktrees").join(folder);
        std::fs::create_dir_all(&meta).unwrap();
        std::fs::write(meta.join("HEAD"), format!("ref: refs/heads/{branch}\n")).unwrap();
        let wt = self.wt_path(folder);
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(wt.join(".git"), format!("gitdir: {}\n", meta.display())).unwrap();
        self.git.set(folder, clean, head);
    }

    fn merged(&self, branch: &str, oid: &str) -> MergedPr {
        MergedPr { repo: REPO.into(), head_ref: branch.into(), head_oid: oid.into() }
    }

    fn states(&self) -> Vec<(String, WtState)> {
        let mut v: Vec<(String, WtState)> = self.svc.snapshot().candidates.into_iter().map(|c| (c.folder, c.state)).collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.dir.path().join(LOG_FILE)).unwrap_or_default()
    }
}

#[test]
fn scan_finds_merged_worktrees_and_classifies_them_by_the_safety_rule() {
    let lab = Lab::new();
    lab.add("1-ok", "feat/1", true, "aaa");
    lab.add("2-dirty", "feat/2", false, "bbb");
    lab.add("3-ahead", "feat/3", true, "ccc-lokal");
    lab.add("4-used", "feat/4", true, "ddd");
    lab.add("5-open", "feat/5", true, "eee");
    // Ein normaler Checkout (.git ist ein Ordner) ist nie Kandidat.
    std::fs::create_dir_all(lab.wt_path("6-clone").join(".git")).unwrap();
    let merged = vec![
        lab.merged("feat/1", "aaa"),
        lab.merged("feat/2", "bbb"),
        lab.merged("feat/3", "ccc"),
        lab.merged("feat/4", "ddd"),
        lab.merged("feat/6", "fff"),
        MergedPr { repo: "Org/other".into(), head_ref: "feat/5".into(), head_oid: "eee".into() },
    ];
    lab.svc.set_merged(merged);
    lab.svc.set_in_use(vec![lab.wt_path("4-used").join("src").to_string_lossy().replace('\\', "/").to_uppercase()]);
    lab.svc.scan_now();
    assert_eq!(
        lab.states(),
        [
            ("1-ok".to_string(), WtState::Removable),
            ("2-dirty".to_string(), WtState::Changed),
            ("3-ahead".to_string(), WtState::Changed),
            ("4-used".to_string(), WtState::InUse),
        ],
        "5 ist in einem anderen Repo gemergt, 6 ist kein worktree"
    );
    let c = &lab.svc.snapshot().candidates[0];
    assert_eq!((c.repo.as_str(), c.branch.as_str()), ("Org/orpheus-management-ui", "feat/1"));
    assert_eq!(Path::new(&c.main_repo), lab.dir.path().join("main"));
    assert!(lab.git.calls_with("4-used: status").is_empty(), "in benutzten worktrees läuft nicht einmal git status");
    assert!(lab.log().contains("Scan: 1 entfernbar, 2 gemergt mit Aenderungen"));
}

#[test]
fn without_merged_prs_nothing_is_scanned() {
    let lab = Lab::new();
    lab.add("1-ok", "feat/1", true, "aaa");
    lab.svc.scan_now();
    assert!(lab.svc.snapshot().candidates.is_empty());
    assert!(lab.svc.snapshot().scanned_at > 0.0);
    assert!(lock(&lab.git.calls).is_empty());
}

#[test]
fn remove_all_removes_only_removable_worktrees_and_their_branches() {
    let lab = Lab::new();
    lab.add("1-ok", "feat/1", true, "aaa");
    lab.add("2-dirty", "feat/2", false, "bbb");
    lab.add("3-ahead", "feat/3", true, "ccc-lokal");
    lab.add("4-used", "feat/4", true, "ddd");
    lab.svc.set_merged(vec![lab.merged("feat/1", "aaa"), lab.merged("feat/2", "bbb"), lab.merged("feat/3", "ccc"), lab.merged("feat/4", "ddd")]);
    lab.svc.set_in_use(vec![lab.wt_path("4-used").to_string_lossy().into_owned()]);
    lab.svc.scan_now();
    let out = lab.svc.remove_all_now().unwrap();
    assert_eq!(out, CleanupOutcome { removed: 1, skipped: 0, failed: 0 });
    let removes = lab.git.calls_with("worktree remove");
    assert_eq!(removes.len(), 1);
    assert!(removes[0].starts_with("main: worktree remove ") && removes[0].ends_with("1-ok"), "{removes:?}");
    assert_eq!(lab.git.calls_with("branch -D"), ["main: branch -D feat/1"]);
    for keep in ["2-dirty", "3-ahead", "4-used"] {
        assert!(lab.wt_path(keep).is_dir(), "{keep} bleibt");
    }
    let snap = lab.svc.snapshot();
    assert_eq!((snap.busy, snap.last_result), (false, Some(out)));
    assert_eq!(snap.removable().count(), 0, "nach dem Lauf neu gescannt");
}

#[test]
fn rules_are_checked_again_right_before_deleting() {
    let lab = Lab::new();
    lab.add("1-now-dirty", "feat/1", true, "aaa");
    lab.add("2-new-commit", "feat/2", true, "bbb");
    lab.add("3-agent-came", "feat/3", true, "ccc");
    lab.add("4-unmerged", "feat/4", true, "ddd");
    lab.svc.set_merged(vec![lab.merged("feat/1", "aaa"), lab.merged("feat/2", "bbb"), lab.merged("feat/3", "ccc"), lab.merged("feat/4", "ddd")]);
    lab.svc.scan_now();
    assert_eq!(lab.svc.snapshot().removable().count(), 4);
    // Zwischen Scan und Klick ändert sich die Welt (ohne neuen Scan).
    lab.git.set("1-now-dirty", false, "aaa");
    lab.git.set("2-new-commit", true, "bbb2");
    {
        let mut inbox = lock(&lab.svc.state.inbox);
        inbox.in_use = vec![lab.wt_path("3-agent-came").to_string_lossy().into_owned()];
        inbox.merged.retain(|m| m.head_ref != "feat/4");
    }
    let out = lab.svc.remove_all_now().unwrap();
    assert_eq!(out, CleanupOutcome { removed: 0, skipped: 4, failed: 0 });
    assert!(lab.git.calls_with("worktree remove").is_empty());
    assert!(lab.git.calls_with("branch -D").is_empty());
    assert_eq!(lab.log().matches("stehen gelassen (").count(), 4);
}

#[test]
fn a_failed_worktree_remove_keeps_the_branch() {
    let lab = Lab::new();
    lab.add("1-ok", "feat/1", true, "aaa");
    lab.svc.set_merged(vec![lab.merged("feat/1", "aaa")]);
    lab.svc.scan_now();
    *lock(&lab.git.fail_remove) = true;
    let out = lab.svc.remove_all_now().unwrap();
    assert_eq!(out, CleanupOutcome { removed: 0, skipped: 0, failed: 1 });
    assert!(lab.git.calls_with("branch -D").is_empty());
    assert!(lab.log().contains("worktree remove gescheitert (Exit 1): 1-ok"));
}

#[test]
fn only_one_removal_runs_at_a_time() {
    let lab = Lab::new();
    lock(&lab.svc.state.snap).busy = true;
    assert_eq!(lab.svc.remove_all_now(), None);
    assert!(!lab.svc.remove_all());
}

#[test]
fn the_real_git_refuses_deleting_commands_in_tests() {
    let d = tempfile::tempdir().unwrap();
    let (rc, msg) = TimedGit::default().run(&["worktree", "remove", "x"], d.path());
    assert_eq!(rc, 1);
    assert!(msg.contains("gesperrt"), "{msg}");
    let (rc, _) = TimedGit::default().run(&["branch", "-D", "x"], d.path());
    assert_eq!(rc, 1);
}

#[test]
fn the_thread_scans_at_once_and_rescans_when_merged_prs_arrive() {
    let lab = Lab::new();
    lab.add("1-ok", "feat/1", true, "aaa");
    let (tx, rx) = std::sync::mpsc::channel();
    let tx = Mutex::new(tx);
    lab.svc.start(Arc::new(move || {
        let _ = lock(&tx).send(());
    }));
    rx.recv_timeout(Duration::from_secs(5)).expect("erster Scan sofort");
    assert!(lab.svc.snapshot().candidates.is_empty());
    lab.svc.set_merged(vec![lab.merged("feat/1", "aaa")]);
    rx.recv_timeout(Duration::from_secs(5)).expect("Scan nach neuen PRs");
    assert_eq!(lab.svc.snapshot().removable().count(), 1);
    lab.svc.stop();
}

/// Echter Probelauf, nur lesend (gh und git status/rev-parse/config, kein Löschen):
/// `cargo test -p deck-services dash::cleanup -- --ignored --nocapture`.
#[test]
#[ignore]
fn real_scan_of_the_wt_folder() {
    use crate::dash::github::GithubService;
    let d = tempfile::tempdir().unwrap();
    let gh = GithubService::system(d.path());
    assert!(gh.fetch_once(), "{:?}", gh.snapshot().error);
    let svc = CleanupService::system(d.path());
    svc.set_merged(gh.snapshot().merged);
    svc.scan_now();
    let s = svc.snapshot();
    for c in &s.candidates {
        println!("{:?}  {}  {}  {}", c.state, c.folder, c.repo, c.branch);
    }
    println!("gemergt: {}, entfernbar: {}, Fehler: {:?}", s.candidates.len(), s.removable().count(), s.error);
}
