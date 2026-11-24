//! Die Team-Ausgabe enthält keine persönlichen Teile, auch nicht abgeschaltet.
//!
//! Info-Fenster, PR-Automatik, Reviewer, Testserver-Kachel, Diktiermikrofon, Store-Update,
//! worktree-Aufräumen und Jira-Präfix leben in einer eigenen Ausgabe, die dieses Repo über
//! `extension.rs` einbindet. Hier darf nichts davon stehen, nicht einmal ein Schalter: was
//! im Quelltext fehlt, kann keine Einstellung freischalten. Der Test liest den Quellbaum
//! (Rust, Oberfläche, Skripte) und sucht nach Spuren.

use std::path::{Path, PathBuf};

/// Spuren der persönlichen Teile, klein geschrieben (verglichen wird ohne Groß/klein).
const TRACES: &[&str] = &[
    "\"gh\"", "'gh'", "gh pr ", "gh api", "gh auth", "acli", "titan", "pr_feed", "prfeed", "speechmike", "paste_guard",
    "dash_window", "comment_window", "dashview", "deck://dash", "get_dash", "dash_action", "reviewer", "store_update",
    "storeupdate", "worktree_sweep", "jira_prefix", "jiraprefix", "github_org", "githuborg", "pr_automation",
    "prautomation", "sun_place\"", "vscode_cleanup", "darkmode_task", "migrate-darkmode",
];

/// Wo gesucht wird (relativ zur Wurzel des Repos) und welche Endungen zählen.
const ROOTS: &[&str] = &["crates", "app/src", "app/src-tauri/src", "app/src-tauri/capabilities", "app/scripts", "tools"];
const EXTS: &[&str] = &["rs", "ts", "tsx", "css", "json", "mjs", "ps1", "toml", "html"];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if p.is_dir() {
            if !matches!(name.as_str(), "node_modules" | "target" | "gen" | "dist" | "binaries") {
                files(&p, out);
            }
        } else if p.extension().is_some_and(|x| EXTS.contains(&x.to_string_lossy().as_ref())) {
            out.push(p);
        }
    }
}

#[test]
fn the_team_edition_carries_no_trace_of_the_personal_parts() {
    let root = repo_root();
    let me = Path::new(file!()).file_name().unwrap().to_owned();
    let mut all = Vec::new();
    for r in ROOTS {
        files(&root.join(r), &mut all);
    }
    assert!(all.len() > 100, "Quellbaum nicht gefunden unter {}", root.display());
    let mut hits = Vec::new();
    for f in all.iter().filter(|f| f.file_name() != Some(me.as_os_str())) {
        let Ok(text) = std::fs::read_to_string(f) else { continue };
        for (n, line) in text.lines().enumerate() {
            let low = line.to_lowercase();
            for t in TRACES.iter().filter(|t| low.contains(*t)) {
                hits.push(format!("{}:{}: {t}", f.strip_prefix(&root).unwrap_or(f).display(), n + 1));
            }
        }
    }
    assert!(hits.is_empty(), "Spuren der persönlichen Ausgabe:\n{}", hits.join("\n"));
}
