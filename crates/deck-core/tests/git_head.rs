//! Branch aus `.git/HEAD` lesen, ohne `git` zu starten, und höchstens alle paar
//! Sekunden je Ordner. Das Dateisystem ist eine Attrappe.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use deck_core::git_head::*;

/// Dateien und Ordner im Speicher; zählt, wie oft gelesen wurde.
#[derive(Default)]
struct FakeFs {
    dirs: Vec<PathBuf>,
    files: HashMap<PathBuf, String>,
    reads: Mutex<u32>,
}

impl FakeFs {
    fn dir(mut self, p: &str) -> Self {
        self.dirs.push(PathBuf::from(p));
        self
    }
    fn file(mut self, p: &str, text: &str) -> Self {
        self.files.insert(PathBuf::from(p), text.into());
        self
    }
    fn reads(&self) -> u32 {
        *self.reads.lock().unwrap()
    }
}

impl GitFs for FakeFs {
    fn is_dir(&self, path: &Path) -> bool {
        self.dirs.iter().any(|d| d == path)
    }
    fn read_text(&self, path: &Path) -> Option<String> {
        *self.reads.lock().unwrap() += 1;
        self.files.get(path).cloned()
    }
}

#[test]
fn head_names_the_branch_and_a_detached_head_has_none() {
    assert_eq!(parse_head("ref: refs/heads/feature/ORPH-4116-x\n").as_deref(), Some("feature/ORPH-4116-x"));
    assert_eq!(parse_head("ref: refs/heads/main").as_deref(), Some("main"));
    assert_eq!(parse_head("3f8f23f2a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6\n"), None);
    assert_eq!(parse_head(""), None);
}

#[test]
fn a_worktree_points_to_its_head_by_gitdir_absolute_or_relative() {
    assert_eq!(parse_gitdir("gitdir: C:/Repos/api/.git/worktrees/4116-api\n", Path::new("C:/x")), Some(PathBuf::from("C:/Repos/api/.git/worktrees/4116-api")));
    assert_eq!(parse_gitdir("gitdir: ../.git/worktrees/a", Path::new("C:/x")), Some(Path::new("C:/x").join("../.git/worktrees/a")));
    assert_eq!(parse_gitdir("kein gitdir", Path::new("C:/x")), None);
}

#[test]
fn the_branch_is_found_upwards_from_a_subfolder_in_a_repo_and_in_a_worktree() {
    let fs = FakeFs::default()
        .dir("C:/Repos/api/.git")
        .file("C:/Repos/api/.git/HEAD", "ref: refs/heads/main\n")
        .file("C:/Repos/wt/4116-api/.git", "gitdir: C:/Repos/api/.git/worktrees/4116-api\n")
        .file("C:/Repos/api/.git/worktrees/4116-api/HEAD", "ref: refs/heads/feature/ORPH-4116\n");
    assert_eq!(find_branch(Path::new("C:/Repos/api/src/Deep"), &fs).as_deref(), Some("main"));
    assert_eq!(find_branch(Path::new("C:/Repos/wt/4116-api/src"), &fs).as_deref(), Some("feature/ORPH-4116"));
    assert_eq!(find_branch(Path::new("C:/Elsewhere"), &fs), None);
}

#[test]
fn a_folder_is_read_at_most_every_few_seconds() {
    let fs = FakeFs::default().dir("C:/r/.git").file("C:/r/.git/HEAD", "ref: refs/heads/ORPH-1-a\n");
    let mut cache = BranchCache::default();
    assert_eq!(cache.get("C:/r", 100.0, &fs).as_deref(), Some("ORPH-1-a"));
    let after_first = fs.reads();
    for i in 0..10 {
        cache.get("C:/r", 100.0 + i as f64 * 0.4, &fs);
    }
    assert_eq!(fs.reads(), after_first, "im 400-ms-Takt kein erneutes Lesen");
    cache.get("C:/r", 100.0 + BRANCH_TTL_S, &fs);
    assert!(fs.reads() > after_first, "nach Ablauf wird neu gelesen");
    assert_eq!(cache.get("", 200.0, &fs), None);
    assert_eq!(NoGit.read_text(Path::new("C:/r/.git/HEAD")), None);
}
