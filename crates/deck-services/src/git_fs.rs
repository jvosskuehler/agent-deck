//! Die echte Fassung von `deck_core::git_head::GitFs`: liest `.git`-Dateien von der
//! Platte. Nur lesen, nie schreiben, kein `git`-Prozess. Wie oft gelesen wird, regelt
//! `deck_core::git_head::BranchCache`.

use std::path::Path;

use deck_core::git_head::GitFs;

/// HEAD- und `.git`-Dateien sind ein paar Byte groß; mehr wird nie gelesen (ein
/// fremder Ordner namens `.git` mit großer Datei bremst so nicht den Takt).
const MAX_BYTES: u64 = 4096;

#[derive(Debug, Default, Clone, Copy)]
pub struct StdGitFs;

impl GitFs for StdGitFs {
    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn read_text(&self, path: &Path) -> Option<String> {
        let meta = std::fs::metadata(path).ok()?;
        if !meta.is_file() || meta.len() > MAX_BYTES {
            return None;
        }
        String::from_utf8(std::fs::read(path).ok()?).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deck_core::git_head::find_branch;

    #[test]
    fn a_real_worktree_file_leads_to_its_branch() {
        let d = tempfile::tempdir().unwrap();
        let wt = d.path().join("wt").join("4116-ui");
        let meta = d.path().join("repo").join(".git").join("worktrees").join("4116-ui");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::create_dir_all(&meta).unwrap();
        std::fs::write(wt.join(".git"), format!("gitdir: {}\n", meta.display())).unwrap();
        std::fs::write(meta.join("HEAD"), "ref: refs/heads/feature/ORPH-4116\n").unwrap();
        assert_eq!(find_branch(&wt.join("src"), &StdGitFs).as_deref(), Some("feature/ORPH-4116"));
        assert_eq!(StdGitFs.read_text(&d.path().join("fehlt")), None);
    }
}
