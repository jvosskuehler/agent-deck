//! Einen an einen Agenten gehängten git worktree wieder aufräumen (Python
//! `ops/worktree.py`).
//!
//! Einem Agenten wird ein Ticket zugewiesen, er legt sich für den Branch einen
//! isolierten worktree NEBEN dem Repo an (`<repo>.wt/<slug>`) und schreibt den Pfad in
//! `state\<slot>.worktree`. Wird der Agent geschlossen, soll der worktree verschwinden.
//!
//! SICHERHEIT (das Modul LÖSCHT Verzeichnisse):
//! * Entfernt wird nur, was zweifelsfrei ein *verlinkter* worktree ist: seine `.git` ist
//!   eine DATEI, deren `gitdir:` unter `…/.git/worktrees/<name>` liegt. Haupt-Checkout,
//!   Submodul (`…/modules/…`) und `--separate-git-dir` werden verweigert.
//! * **Nie mit nicht committeter Arbeit.** Vor dem ersten Löschversuch fragt
//!   `git status --porcelain` nach; geänderte oder neue Dateien, oder eine Antwort, die
//!   nicht zu bekommen ist, heißen: stehen lassen. Hier weicht diese Fassung bewusst von
//!   Python ab (dort `git worktree remove --force` auch mit offenen Änderungen).
//! * Der Branch und seine Commits bleiben immer erhalten, nur das Arbeitsverzeichnis geht.
//!
//! Die Läufe blockieren (git, Wartezeiten): nur aus einem Hintergrund-Thread aufrufen.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use crate::fsutil::{force_remove_dir, normpath};
use crate::logfile::{LogFile, Rotate};

/// Wartezeiten vor den Versuchen: das geschlossene Terminal und seine Kinder müssen erst
/// sterben und ihre Datei-Handles freigeben, sonst scheitert das Löschen unter Windows.
pub const RETRY_DELAYS: [Duration; 4] =
    [Duration::from_secs(1), Duration::from_secs(2), Duration::from_secs(4), Duration::from_secs(8)];
pub const LOG_FILE: &str = "worktree-cleanup.log";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Ein Eintrag aus `git worktree list --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub path: String,
    /// Ohne `refs/heads/`; `None` bei detached HEAD.
    pub branch: Option<String>,
}

/// `git worktree list --porcelain` zerlegen. Blöcke durch Leerzeilen getrennt, je Block
/// eine `worktree <pfad>`-Zeile und optional `branch refs/heads/<name>`.
pub fn parse_porcelain(text: &str) -> Vec<WorktreeEntry> {
    let mut out = Vec::new();
    let mut cur: Option<WorktreeEntry> = None;
    for raw in text.lines() {
        let line = raw.trim_end();
        if let Some(p) = line.strip_prefix("worktree ") {
            out.extend(cur.take());
            cur = Some(WorktreeEntry { path: p.trim().to_string(), branch: None });
        } else if let (Some(r), Some(c)) = (line.strip_prefix("branch "), cur.as_mut()) {
            let r = r.trim();
            c.branch = Some(r.strip_prefix("refs/heads/").unwrap_or(r).to_string());
        }
    }
    out.extend(cur);
    out
}

/// Der Haupt-Checkout ist der erste Eintrag.
pub fn main_path(entries: &[WorktreeEntry]) -> Option<&str> {
    entries.first().map(|e| e.path.as_str())
}

/// Pfad des worktree, der auf `branch` ausgecheckt ist.
pub fn path_for_branch<'a>(entries: &'a [WorktreeEntry], branch: &str) -> Option<&'a str> {
    if branch.is_empty() {
        return None;
    }
    entries.iter().find(|e| e.branch.as_deref() == Some(branch)).map(|e| e.path.as_str())
}

/// Sammelordner der deck-beauftragten worktrees eines Repos: per Konvention
/// (`config::TICKET_PROMPT`) immer `<repo-root>.wt`.
pub fn wt_dir_for_repo(repo_root: &str) -> Option<String> {
    (!repo_root.is_empty()).then(|| normpath(repo_root) + ".wt")
}

/// Umkehr: `<x>.wt` → `<x>`; sonst `None` (kein Sammelordner).
pub fn repo_root_from_wt_dir(wt_dir: &str) -> Option<String> {
    if wt_dir.is_empty() {
        return None;
    }
    let n = normpath(wt_dir);
    n.to_lowercase().ends_with(".wt").then(|| n[..n.len() - 3].to_string())
}

/// Ist das ein VERLINKTER worktree? Die zentrale Sicherung: bewusst ohne git geprüft,
/// denn fehlt git oder hängt es, ist dies die einzige Schranke vor dem Löschen.
pub fn is_linked_worktree(path: &Path) -> bool {
    let gitfile = path.join(".git");
    if !gitfile.is_file() {
        return false;
    }
    let Ok(text) = std::fs::read(&gitfile) else { return false };
    let text = String::from_utf8_lossy(&text);
    let first = text.lines().next().unwrap_or("").trim();
    let Some(gitdir) = first.strip_prefix("gitdir:") else { return false };
    let n = normpath(gitdir.trim());
    let parent = crate::fsutil::dirname(&n);
    crate::fsutil::basename(parent) == "worktrees"
}

/// Direkte Unterordner (keine Dateien); leer, wenn `parent` kein Ordner ist.
pub fn list_child_dirs(parent: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(parent) else { return Vec::new() };
    let mut out: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    out.sort();
    out
}

/// git aufrufen; (Exit-Code, stdout+stderr). Nie ein Fehler: git fehlt, Zeitüberschreitung
/// oder kein Repo werden zu `(1, Meldung)`.
pub trait Git: Send + Sync {
    fn run(&self, args: &[&str], cwd: &Path) -> (i32, String);
}

/// Die echte Fassung. Ausgabe hart als UTF-8 gelesen und `core.quotepath=false`: der
/// Heimatordner des Nutzers enthält Umlaute, und ein falsch dekodierter Pfad aus
/// `worktree list` ließe jedes Aufräumen ins Leere laufen.
#[derive(Debug, Default, Clone, Copy)]
pub struct GitCli;

impl Git for GitCli {
    fn run(&self, args: &[&str], cwd: &Path) -> (i32, String) {
        let out = Command::new("git")
            .arg("-c")
            .arg("core.quotepath=false")
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        match out {
            Ok(o) => {
                let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
                s.push_str(&String::from_utf8_lossy(&o.stderr));
                (o.status.code().unwrap_or(1), s)
            }
            Err(e) => (1, e.to_string()),
        }
    }
}

/// Die Aufräum-Maschinerie mit ihren Abhängigkeiten.
#[derive(Clone)]
pub struct WorktreeOps {
    git: Arc<dyn Git>,
    log: LogFile,
    delays: Vec<Duration>,
}

impl WorktreeOps {
    pub fn new(git: Arc<dyn Git>, log: LogFile, delays: Vec<Duration>) -> WorktreeOps {
        WorktreeOps { git, log, delays }
    }

    /// Echtes git, Protokoll `state\worktree-cleanup.log`, Python-Wartezeiten.
    pub fn system(state_dir: &Path) -> WorktreeOps {
        WorktreeOps::new(Arc::new(GitCli), LogFile::new(state_dir.join(LOG_FILE), 200_000, Rotate::Truncate), RETRY_DELAYS.to_vec())
    }

    /// Protokolleintrag (auch für Aufrufer, die bewusst NICHT löschen).
    pub fn note(&self, msg: &str) {
        self.log.note(msg);
    }

    /// Geparste worktree-Liste des Repos, in dem `cwd` liegt (leer bei Fehler).
    pub fn worktrees(&self, cwd: &Path) -> Vec<WorktreeEntry> {
        match self.git.run(&["worktree", "list", "--porcelain"], cwd) {
            (0, out) => parse_porcelain(&out),
            _ => Vec::new(),
        }
    }

    /// Im Repo unter `repo` den worktree, der auf `branch` steht.
    pub fn worktree_for_branch(&self, repo: &str, branch: &str) -> Option<String> {
        if repo.is_empty() || branch.is_empty() || !Path::new(repo).is_dir() {
            return None;
        }
        path_for_branch(&self.worktrees(Path::new(repo)), branch).map(String::from)
    }

    /// Hat der worktree nicht committete Arbeit (geänderte, gelöschte oder neue Dateien)?
    /// `None` = nicht herauszufinden; das gilt beim Löschen wie ein Ja.
    pub fn has_uncommitted(&self, path: &Path) -> Option<bool> {
        match self.git.run(&["status", "--porcelain", "--untracked-files=normal"], path) {
            (0, out) => Some(!out.trim().is_empty()),
            _ => None,
        }
    }

    /// Einen verlinkten worktree entfernen. `true`, wenn er weg ist (oder schon war).
    /// Verweigert alles, was kein verlinkter worktree ist, den Haupt-Checkout selbst und
    /// jeden worktree mit nicht committeter Arbeit.
    pub fn remove_worktree(&self, path: &str) -> bool {
        if path.is_empty() {
            return false;
        }
        let norm = normpath(path);
        let p = Path::new(&norm);
        if !p.is_dir() {
            return true; // schon weg
        }
        if !is_linked_worktree(p) {
            self.note(&format!("REFUSE (kein verlinkter worktree, .git ist kein File): {norm}"));
            return false;
        }
        // Das Listing läuft VON INNEN (nur lesend); gelöscht wird vom Haupt-Checkout aus,
        // dessen cwd nicht im zu löschenden Ordner liegt.
        let main = main_path(&self.worktrees(p)).map(normpath);
        if main.as_deref() == Some(norm.as_str()) {
            self.note(&format!("REFUSE (Pfad ist selbst der Haupt-Checkout): {norm}"));
            return false;
        }
        match self.has_uncommitted(p) {
            Some(false) => {}
            Some(true) => {
                self.note(&format!("REFUSE (nicht committete Aenderungen, bleibt stehen): {norm}"));
                return false;
            }
            None => {
                self.note(&format!("REFUSE (git status nicht lesbar, im Zweifel stehen lassen): {norm}"));
                return false;
            }
        }
        let mut last = String::new();
        for delay in &self.delays {
            std::thread::sleep(*delay);
            if let Some(main) = &main {
                // Sauber geprüft ist oben; `--force` nur, weil git sonst auch an
                // ignorierten Dateien (Build-Ausgaben) scheitert.
                let (rc, out) = self.git.run(&["worktree", "remove", "--force", &norm], Path::new(main));
                if rc == 0 && !p.is_dir() {
                    self.note(&format!("entfernt (git worktree remove): {norm}"));
                    return true;
                }
                last = out;
            }
            // Rückfall: Verzeichnis selbst löschen (absoluter Pfad, unabhängig vom cwd).
            if let Err(e) = force_remove_dir(p) {
                last = e.to_string();
            }
            if !p.is_dir() {
                if let Some(main) = &main {
                    // --expire=now: sonst bleibt der Verwaltungseintrag 3 Monate stehen.
                    self.git.run(&["worktree", "prune", "--expire=now"], Path::new(main));
                }
                self.note(&format!("entfernt (rmtree{}): {norm}", if main.is_some() { " + prune" } else { "" }));
                return true;
            }
        }
        let short: String = last.chars().take(200).collect();
        self.note(&format!("FEHLGESCHLAGEN nach Versuchen: {norm} ({short})"));
        false
    }

    /// Einen halb abgeräumten Rest entfernen: ein Ordner, den git nicht mehr führt und der
    /// GAR KEIN `.git` mehr hat (Windows: `git worktree remove` kam bis zur `.git`-Datei,
    /// dann stoppte eine gesperrte Datei). Sobald ein `.git` da ist (Datei oder Ordner),
    /// wird verweigert: das ist ein echter Checkout, ein Submodul oder ein worktree.
    pub fn remove_orphan_dir(&self, path: &str, repo: Option<&str>) -> bool {
        if path.is_empty() {
            return false;
        }
        let norm = normpath(path);
        let p = Path::new(&norm);
        if !p.is_dir() {
            return true;
        }
        if p.join(".git").exists() {
            self.note(&format!(
                "REFUSE orphan-dir (hat noch ein .git -> kein abgeraeumter Rest; \
                 Checkout/Submodul/separate-git-dir/worktree): {norm}"
            ));
            return false;
        }
        let mut last = String::new();
        for delay in &self.delays {
            std::thread::sleep(*delay);
            if let Err(e) = force_remove_dir(p) {
                last = e.to_string();
            }
            if !p.is_dir() {
                let repo = repo.filter(|r| Path::new(r).is_dir());
                if let Some(r) = repo {
                    self.git.run(&["worktree", "prune", "--expire=now"], Path::new(r));
                }
                self.note(&format!("entfernt (orphan-dir rmtree{}): {norm}", if repo.is_some() { " + prune" } else { "" }));
                return true;
            }
        }
        let short: String = last.chars().take(200).collect();
        self.note(&format!("FEHLGESCHLAGEN orphan-dir nach Versuchen: {norm} ({short})"));
        false
    }
}

#[cfg(test)]
#[path = "worktree_tests.rs"]
pub(crate) mod tests;
