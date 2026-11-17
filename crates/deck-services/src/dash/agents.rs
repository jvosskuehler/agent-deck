//! Hintergrund-Agenten als Prozesse: `claude -p` ohne Fenster in einem worktree, die
//! Ausgabe (`stream-json`) in eine Datei, beenden samt Kindern. Und der worktree dazu:
//! vorhanden (der Branch ist schon irgendwo ausgecheckt) oder neu unter `Repos\wt\`.
//!
//! Wirkung nach außen hinter [`AgentRunner`] und `worktree::Git`; Tests bekommen
//! Attrappen, die echte Fassung prüft zusätzlich das Schutzgitter.
//!
//! Der Agent bekommt weder `AGENT_SLOT` noch die Herkunftsmarke des Decks: er ist keine
//! Agenten-Session des Decks, seine Hooks melden nichts (CLAUDE.md, Vertrag 4).

use std::fs::File;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use crate::worktree::{Git, parse_porcelain, path_for_branch};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Ein laufender Agent.
pub trait AgentProc: Send {
    /// `Some(exit code)`, sobald er beendet ist.
    fn poll(&mut self) -> Option<i32>;
    /// Beenden samt Kindprozessen.
    fn kill(&mut self);
}

/// Startet Hintergrund-Agenten.
pub trait AgentRunner: Send + Sync {
    /// `dir`: Arbeitsordner, `out`: Datei für die Ausgabe, `extra_dir`: weiterer Ordner,
    /// in den der Agent schreiben darf (Entwürfe).
    fn start(&self, dir: &Path, prompt: &str, out: &Path, extra_dir: &Path) -> Result<Box<dyn AgentProc>, String>;
}

/// Die echte Fassung: `claude -p … --output-format stream-json --verbose
/// --permission-mode bypassPermissions`.
#[derive(Debug, Clone, Default)]
pub struct ClaudeCli {
    /// Pfad zu claude; leer = über PATH, sonst `~\.local\bin\claude.exe`.
    pub exe: Option<PathBuf>,
}

fn claude_exe() -> PathBuf {
    let local = deck_core::paths::home_dir().join(".local").join("bin").join("claude.exe");
    if local.is_file() { local } else { PathBuf::from("claude") }
}

struct RealProc {
    child: Child,
}

impl AgentProc for RealProc {
    fn poll(&mut self) -> Option<i32> {
        match self.child.try_wait() {
            Ok(Some(s)) => Some(s.code().unwrap_or(1)),
            Ok(None) => None,
            Err(_) => Some(1),
        }
    }

    fn kill(&mut self) {
        if crate::guard::check("Hintergrund-Agent beenden").is_err() {
            return;
        }
        // Baum beenden: claude startet Shells und Werkzeuge, die sonst weiterliefen.
        let mut cmd = Command::new("taskkill");
        cmd.args(["/PID", &self.child.id().to_string(), "/T", "/F"]).creation_flags(CREATE_NO_WINDOW);
        let _ = cmd.stdout(Stdio::null()).stderr(Stdio::null()).status();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl AgentRunner for ClaudeCli {
    fn start(&self, dir: &Path, prompt: &str, out: &Path, extra_dir: &Path) -> Result<Box<dyn AgentProc>, String> {
        crate::guard::check("claude -p starten").map_err(|e| e.to_string())?;
        if let Some(p) = out.parent() {
            std::fs::create_dir_all(p).map_err(|e| format!("Ausgabeordner: {}", e.kind()))?;
        }
        let stdout = File::create(out).map_err(|e| format!("Ausgabedatei: {}", e.kind()))?;
        let stderr = File::create(out.with_extension("err")).map_err(|e| format!("Fehlerdatei: {}", e.kind()))?;
        let exe = self.exe.clone().unwrap_or_else(claude_exe);
        let mut cmd = Command::new(exe);
        cmd.arg("-p")
            .arg(prompt)
            .args(["--output-format", "stream-json", "--verbose", "--permission-mode", "bypassPermissions", "--add-dir"])
            .arg(extra_dir)
            .current_dir(dir)
            .env_remove("AGENT_SLOT")
            .env_remove("AGENT_DECK_SHELL")
            .env_remove("GITHUB_TOKEN")
            .env_remove("GH_TOKEN")
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .creation_flags(CREATE_NO_WINDOW);
        let child = cmd.spawn().map_err(|e| format!("claude nicht startbar: {}", e.kind()))?;
        Ok(Box::new(RealProc { child }))
    }
}

/// Den worktree für `branch` von `repo` (`owner/name`) finden oder anlegen. `repos_root`
/// ist `%USERPROFILE%\Repos` (Haupt-Checkouts direkt darin, worktrees in `wt\`).
/// Ist der Branch schon irgendwo ausgecheckt, gilt dieser Ordner (auch der Haupt-Checkout
/// eines Repos, das direkt auf dem Branch steht).
pub fn ensure_worktree(git: &dyn Git, repos_root: &Path, repo: &str, branch: &str, folder: &str) -> Result<PathBuf, String> {
    let name = repo.rsplit('/').next().unwrap_or(repo);
    let main = repos_root.join(name);
    if !main.join(".git").exists() {
        return Err(format!("kein Checkout von {name} unter Repos"));
    }
    let (code, list) = git.run(&["worktree", "list", "--porcelain"], &main);
    if code == 0
        && let Some(p) = path_for_branch(&parse_porcelain(&list), branch)
    {
        return Ok(PathBuf::from(p));
    }
    let (code, err) = git.run(&["fetch", "origin", branch], &main);
    if code != 0 {
        return Err(format!("git fetch scheiterte (Exit {code}): {}", super::hidden_cmd::short_error(&err)));
    }
    let wt = repos_root.join("wt");
    let mut target = wt.join(folder);
    if target.exists() {
        target = wt.join(format!("{folder}-{}", branch.replace(['/', '\\'], "-")));
    }
    let target_s = target.to_string_lossy().to_string();
    let local = git.run(&["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")], &main).0 == 0;
    let remote = format!("origin/{branch}");
    let args: Vec<&str> = if local {
        vec!["worktree", "add", &target_s, branch]
    } else {
        vec!["worktree", "add", "--track", "-b", branch, &target_s, &remote]
    };
    let (code, err) = git.run(&args, &main);
    if code != 0 {
        return Err(format!("git worktree add scheiterte (Exit {code}): {}", super::hidden_cmd::short_error(&err)));
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Antwortet je Befehlsanfang mit festem Ergebnis und merkt sich die Aufrufe.
    struct FakeGit {
        list: String,
        calls: Mutex<Vec<String>>,
        local_branch: bool,
    }

    impl Git for FakeGit {
        fn run(&self, args: &[&str], _cwd: &Path) -> (i32, String) {
            self.calls.lock().unwrap().push(args.join(" "));
            match args {
                ["worktree", "list", ..] => (0, self.list.clone()),
                ["rev-parse", ..] => (if self.local_branch { 0 } else { 1 }, String::new()),
                _ => (0, String::new()),
            }
        }
    }

    fn root() -> (tempfile::TempDir, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().to_path_buf();
        std::fs::create_dir_all(root.join("app").join(".git")).unwrap();
        (d, root)
    }

    #[test]
    fn an_existing_checkout_of_the_branch_is_reused() {
        let (_d, root) = root();
        let git = FakeGit { list: "worktree C:/R/app\nbranch refs/heads/main\n\nworktree C:/R/wt/1-app\nbranch refs/heads/feat/x\n".into(), calls: Mutex::default(), local_branch: true };
        assert_eq!(ensure_worktree(&git, &root, "Org/app", "feat/x", "1-app").unwrap(), PathBuf::from("C:/R/wt/1-app"));
        assert_eq!(git.calls.lock().unwrap().len(), 1, "nichts angelegt");
    }

    #[test]
    fn a_missing_worktree_is_created_tracking_the_remote_branch() {
        let (_d, root) = root();
        let git = FakeGit { list: "worktree C:/R/app\nbranch refs/heads/main\n".into(), calls: Mutex::default(), local_branch: false };
        let p = ensure_worktree(&git, &root, "Org/app", "feat/y", "2-app").unwrap();
        assert_eq!(p, root.join("wt").join("2-app"));
        let calls = git.calls.lock().unwrap();
        assert!(calls.iter().any(|c| c == "fetch origin feat/y"));
        assert!(calls.iter().any(|c| c.starts_with("worktree add --track -b feat/y") && c.ends_with("origin/feat/y")));
    }

    #[test]
    fn without_a_main_checkout_nothing_is_created() {
        let (_d, root) = root();
        let git = FakeGit { list: String::new(), calls: Mutex::default(), local_branch: false };
        assert!(ensure_worktree(&git, &root, "Org/other", "b", "3-other").is_err());
        assert!(git.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn the_real_runner_is_locked_in_tests() {
        let d = tempfile::tempdir().unwrap();
        let err = ClaudeCli::default().start(d.path(), "x", &d.path().join("o.jsonl"), d.path()).err().unwrap();
        assert!(err.contains("gesperrt"));
    }
}
