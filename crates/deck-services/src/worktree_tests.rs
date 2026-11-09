//! Tests zu `worktree.rs` (aus `tests/test_ops_worktree.py`), dazu echte Läufe gegen
//! Wegwerf-Repos im Temp-Ordner: nur dort darf `git worktree remove` laufen.

use super::*;

const PORCELAIN: &str = "worktree C:/repo/my-backend\n\
HEAD 1111111111111111111111111111111111111111\n\
branch refs/heads/main\n\
\n\
worktree C:/repo/my-backend.wt/abc-123\n\
HEAD 2222222222222222222222222222222222222222\n\
branch refs/heads/ticket/abc-123\n\
\n\
worktree C:/repo/detached-one\n\
HEAD 3333333333333333333333333333333333333333\n\
detached\n";

#[test]
fn porcelain_is_parsed_and_refs_heads_cut_off() {
    let e = parse_porcelain(PORCELAIN);
    let paths: Vec<&str> = e.iter().map(|x| x.path.as_str()).collect();
    assert_eq!(paths, ["C:/repo/my-backend", "C:/repo/my-backend.wt/abc-123", "C:/repo/detached-one"]);
    let branches: Vec<Option<&str>> = e.iter().map(|x| x.branch.as_deref()).collect();
    assert_eq!(branches, [Some("main"), Some("ticket/abc-123"), None]);
    assert!(parse_porcelain("").is_empty());
}

#[test]
fn the_main_checkout_is_the_first_entry_and_branches_are_looked_up() {
    let e = parse_porcelain(PORCELAIN);
    assert_eq!(main_path(&e), Some("C:/repo/my-backend"));
    assert_eq!(path_for_branch(&e, "ticket/abc-123"), Some("C:/repo/my-backend.wt/abc-123"));
    assert_eq!(path_for_branch(&e, "ticket/nope"), None);
    assert_eq!(path_for_branch(&e, ""), None);
    assert_eq!(path_for_branch(&[], "ticket/abc-123"), None);
    assert_eq!(main_path(&[]), None);
}

fn mk(base: &Path, name: &str, gitfile: Option<&str>, gitdir: bool) -> PathBuf {
    let d = base.join(name);
    std::fs::create_dir_all(&d).unwrap();
    if gitdir {
        std::fs::create_dir_all(d.join(".git")).unwrap();
    } else if let Some(content) = gitfile {
        std::fs::write(d.join(".git"), content).unwrap();
    }
    d
}

#[test]
fn only_a_linked_worktree_passes_the_guard() {
    let base = tempfile::tempdir().unwrap();
    let b = base.path();
    assert!(is_linked_worktree(&mk(b, "linked", Some("gitdir: C:/repo/.git/worktrees/abc-123\n"), false)));
    assert!(!is_linked_worktree(&mk(b, "submod", Some("gitdir: ../.git/modules/sub\n"), false)));
    assert!(!is_linked_worktree(&mk(b, "sepdir", Some("gitdir: C:/elsewhere/store.git\n"), false)));
    assert!(!is_linked_worktree(&mk(b, "maindir", None, true)));
    assert!(!is_linked_worktree(&mk(b, "plaindir", None, false)));
    assert!(!is_linked_worktree(&mk(b, "garbage", Some("not a gitdir line\n"), false)));
    assert!(!is_linked_worktree(&b.join("does-not-exist")));
}

#[test]
fn the_wt_folder_convention_round_trips() {
    let root = normpath("C:/repo/my-web-ui");
    let wt = wt_dir_for_repo(&root).unwrap();
    assert_eq!(wt, format!("{root}.wt"));
    assert_eq!(repo_root_from_wt_dir(&wt).as_deref(), Some(root.as_str()));
    let marker = format!(r"{wt}\46845651463");
    assert_eq!(repo_root_from_wt_dir(crate::fsutil::dirname(&marker)).as_deref(), Some(root.as_str()));
    assert_eq!(repo_root_from_wt_dir(&normpath("C:/repo/plain")), None);
    assert_eq!(wt_dir_for_repo(""), None);
    assert_eq!(repo_root_from_wt_dir(""), None);
}

#[test]
fn only_child_folders_are_listed() {
    let base = tempfile::tempdir().unwrap();
    let wt = base.path().join("repo.wt");
    std::fs::create_dir_all(wt.join("a")).unwrap();
    std::fs::create_dir_all(wt.join("b")).unwrap();
    std::fs::write(wt.join("loose.txt"), "x").unwrap();
    let names: Vec<String> = list_child_dirs(&wt).iter().map(|p| p.file_name().unwrap().to_string_lossy().into()).collect();
    assert_eq!(names, ["a", "b"]);
    assert!(list_child_dirs(&base.path().join("nope")).is_empty());
}

/// Ohne Wartezeiten und mit Protokoll im Temp-Ordner.
fn ops(log_dir: &Path) -> WorktreeOps {
    WorktreeOps::new(Arc::new(GitCli), LogFile::new(log_dir.join(LOG_FILE), 200_000, Rotate::Truncate), vec![Duration::ZERO])
}

#[test]
fn an_orphan_rest_without_git_is_removed_anything_with_git_is_refused() {
    let base = tempfile::tempdir().unwrap();
    let o = ops(base.path());
    let leftover = base.path().join("repo.wt").join("2701");
    std::fs::create_dir_all(&leftover).unwrap();
    std::fs::write(leftover.join("file.txt"), "stale").unwrap();
    assert!(o.remove_orphan_dir(leftover.to_str().unwrap(), None));
    assert!(!leftover.exists());

    let checkout = mk(&base.path().join("repo.wt"), "real", None, true);
    assert!(!o.remove_orphan_dir(checkout.to_str().unwrap(), None));
    assert!(checkout.is_dir());

    let submod = mk(&base.path().join("repo.wt"), "submod", Some("gitdir: ../.git/modules/submod\n"), false);
    assert!(!o.remove_orphan_dir(submod.to_str().unwrap(), None));
    assert!(submod.is_dir());

    assert!(o.remove_orphan_dir(base.path().join("gone").to_str().unwrap(), None), "schon weg = erledigt");
    assert!(!o.remove_orphan_dir("", None));
}

// ── Echte Wegwerf-Repos ─────────────────────────────────────────────────────

pub(crate) fn git(cwd: &Path, args: &[&str]) {
    let (rc, out) = GitCli.run(args, cwd);
    assert_eq!(rc, 0, "git {args:?}: {out}");
}

/// Langer Pfad ohne `\\?\`: `%TEMP%` steht oft als Kurzname da (`JORRIT~1`), git gibt
/// aber Langnamen aus.
pub(crate) fn real(p: &Path) -> PathBuf {
    let c = std::fs::canonicalize(p).unwrap();
    PathBuf::from(c.to_string_lossy().trim_start_matches(r"\\?\").to_string())
}

/// Repo `repo` mit einem Commit und einem verlinkten worktree `repo.wt/abc-123`.
pub(crate) fn repo_with_worktree(base: &Path) -> (PathBuf, PathBuf) {
    let base = real(base);
    let repo = base.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "a.txt"]);
    git(&repo, &["-c", "user.name=deck-test", "-c", "user.email=deck@test.invalid", "commit", "-q", "-m", "init"]);
    let wt = base.join("repo.wt").join("abc-123");
    git(&repo, &["worktree", "add", "-q", "-b", "ticket/abc-123", wt.to_str().unwrap()]);
    (repo, wt)
}

#[test]
fn a_clean_linked_worktree_is_removed_and_its_branch_stays() {
    let base = tempfile::tempdir().unwrap();
    let (repo, wt) = repo_with_worktree(base.path());
    let o = ops(base.path());
    assert_eq!(o.worktree_for_branch(repo.to_str().unwrap(), "ticket/abc-123").map(|p| normpath(&p)), Some(normpath(wt.to_str().unwrap())));
    assert!(o.remove_worktree(wt.to_str().unwrap()));
    assert!(!wt.exists());
    let (_, branches) = GitCli.run(&["branch", "--list", "ticket/abc-123"], &repo);
    assert!(branches.contains("ticket/abc-123"), "der Branch bleibt erhalten");
    assert_eq!(o.worktrees(&repo).len(), 1, "git führt den worktree nicht mehr");
}

#[test]
fn a_worktree_with_uncommitted_work_is_never_removed() {
    let base = tempfile::tempdir().unwrap();
    let (_repo, wt) = repo_with_worktree(base.path());
    let o = ops(base.path());
    std::fs::write(wt.join("neu.txt"), "noch nicht committet").unwrap();
    assert!(!o.remove_worktree(wt.to_str().unwrap()));
    assert!(wt.join("neu.txt").is_file());
    std::fs::remove_file(wt.join("neu.txt")).unwrap();
    std::fs::write(wt.join("a.txt"), "geaendert\n").unwrap();
    assert!(!o.remove_worktree(wt.to_str().unwrap()), "auch eine geänderte Datei hält ihn fest");
    let log = std::fs::read_to_string(base.path().join(LOG_FILE)).unwrap();
    assert!(log.contains("nicht committete"), "{log}");
}

#[test]
fn the_main_checkout_is_never_removed() {
    let base = tempfile::tempdir().unwrap();
    let (repo, _wt) = repo_with_worktree(base.path());
    assert!(!ops(base.path()).remove_worktree(repo.to_str().unwrap()));
    assert!(repo.join("a.txt").is_file());
}

/// git, das nicht antwortet.
struct DeadGit;

impl Git for DeadGit {
    fn run(&self, _: &[&str], _: &Path) -> (i32, String) {
        (1, "git nicht gefunden".into())
    }
}

#[test]
fn without_an_answer_from_git_nothing_is_removed() {
    // „Weiß nicht“ ist kein „sauber“: ohne git-Status bleibt der worktree stehen, auch
    // wenn die .git-Datei ihn als verlinkt ausweist.
    let base = tempfile::tempdir().unwrap();
    let wt = mk(base.path(), "linked", Some("gitdir: C:/repo/.git/worktrees/x\n"), false);
    let o = WorktreeOps::new(Arc::new(DeadGit), LogFile::new(base.path().join(LOG_FILE), 1000, Rotate::Truncate), vec![Duration::ZERO]);
    assert!(!o.remove_worktree(wt.to_str().unwrap()));
    assert!(wt.is_dir());
}
