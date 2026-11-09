//! Den Branch eines Ordners lesen, ohne `git` zu starten: `.git/HEAD` parsen, bei einem
//! worktree erst die `.git`-Datei (`gitdir: …`) und dann dessen `HEAD`.
//!
//! Das läuft im 400-ms-Takt für jeden Agenten. Ein `git`-Prozess je Takt und Agent wäre
//! zu teuer, und auch das Lesen der Dateien soll nicht in jedem Takt passieren: der
//! [`BranchCache`] liest einen Ordner höchstens alle [`BRANCH_TTL_S`] Sekunden neu.
//! Das Dateisystem steckt hinter [`GitFs`]; die echte Fassung liegt in `deck-services`,
//! Tests reichen eine Attrappe herein.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// So lange gilt ein gelesener Branch, bevor der Ordner neu gelesen wird (s). Ein
/// Branchwechsel taucht damit nach spätestens ein paar Sekunden auf der Kachel auf.
pub const BRANCH_TTL_S: f64 = 5.0;

/// Was vom Dateisystem gebraucht wird: ist da ein Ordner, und was steht in einer Datei.
pub trait GitFs: Send + Sync {
    fn is_dir(&self, path: &Path) -> bool;
    /// Inhalt einer Textdatei; `None`, wenn sie fehlt oder kein Text ist.
    fn read_text(&self, path: &Path) -> Option<String>;
}

/// Ohne Dateisystem (Tests der App, Dienste aus): nie ein Branch.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoGit;

impl GitFs for NoGit {
    fn is_dir(&self, _: &Path) -> bool {
        false
    }
    fn read_text(&self, _: &Path) -> Option<String> {
        None
    }
}

/// `ref: refs/heads/feature/ORPH-4116` → `feature/ORPH-4116`. Ein losgelöster HEAD (nur
/// ein Commit-Hash) hat keinen Branch.
pub fn parse_head(content: &str) -> Option<String> {
    let line = content.lines().next()?.trim();
    let name = line.strip_prefix("ref:")?.trim();
    let name = name.strip_prefix("refs/heads/").unwrap_or(name).trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// `.git`-Datei eines worktree (`gitdir: C:/repo/.git/worktrees/4116-ui`) → der Ordner,
/// in dem dessen `HEAD` liegt. Ein relativer Pfad gilt relativ zum Ordner der Datei.
pub fn parse_gitdir(content: &str, base: &Path) -> Option<PathBuf> {
    let line = content.lines().next()?.trim();
    let dir = line.strip_prefix("gitdir:")?.trim();
    if dir.is_empty() {
        return None;
    }
    let p = PathBuf::from(dir);
    Some(if p.is_absolute() { p } else { base.join(p) })
}

/// Branch des Repos, in dem `cwd` liegt: vom Ordner aufwärts bis zum ersten `.git`
/// (Ordner = normales Repo, Datei = worktree). Kein Repo oder losgelöster HEAD → `None`.
pub fn find_branch(cwd: &Path, fs: &dyn GitFs) -> Option<String> {
    for dir in cwd.ancestors() {
        let dot = dir.join(".git");
        if fs.is_dir(&dot) {
            return parse_head(&fs.read_text(&dot.join("HEAD"))?);
        }
        if let Some(text) = fs.read_text(&dot) {
            let gitdir = parse_gitdir(&text, dir)?;
            return parse_head(&fs.read_text(&gitdir.join("HEAD"))?);
        }
    }
    None
}

/// Gelesene Branches je Ordner, mit Lesezeitpunkt.
#[derive(Debug, Default)]
pub struct BranchCache {
    seen: HashMap<String, (f64, Option<String>)>,
}

impl BranchCache {
    /// Branch von `dir`, höchstens alle [`BRANCH_TTL_S`] Sekunden frisch gelesen.
    pub fn get(&mut self, dir: &str, now: f64, fs: &dyn GitFs) -> Option<String> {
        if dir.trim().is_empty() {
            return None;
        }
        if let Some((at, branch)) = self.seen.get(dir)
            && now - at < BRANCH_TTL_S
            && now >= *at
        {
            return branch.clone();
        }
        let branch = find_branch(Path::new(dir), fs);
        self.seen.insert(dir.to_string(), (now, branch.clone()));
        branch
    }

    /// Nur die gefragten Ordner behalten (geschlossene Agenten fallen heraus).
    pub fn retain(&mut self, dirs: &[String]) {
        self.seen.retain(|d, _| dirs.contains(d));
    }
}
