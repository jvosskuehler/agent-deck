//! Der PR-Feed in der App: den Feed-Dienst (`deck_services::dash::feed_service`) je Takt
//! mit Einstellungen und Agenten-Ordnern versorgen, die worktrees der vom Deck gemergten
//! PRs an den Aufräum-Dienst reichen und die Knöpfe des Feeds ausführen.
//!
//! Einstellungen in `deck_settings.json`: `pr_reviewer` (wer als Reviewer nachgetragen
//! wird) und `github_org` (Organisation für die Jira-Suche). Beide stehen nicht im Code:
//! das Repo ist öffentlich.

use std::sync::Arc;

use deck_core::dash::feed_view::{CommentPopup, FeedView};
use deck_core::i18n::Lang;
use deck_services::dash::cleanup::CleanupService;
use deck_services::dash::feed_actions::FeedReply;
use deck_services::dash::feed_service::{FeedConfig, FeedService};
use deck_services::dash::github::GithubService;
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use crate::applog::note;

/// Was der Feed aus den Einstellungen braucht.
#[derive(Debug, Clone, Default)]
pub struct FeedSettings {
    pub reviewer: String,
    pub org: String,
}

impl FeedSettings {
    pub fn from_settings(s: &serde_json::Map<String, serde_json::Value>) -> FeedSettings {
        let get = |k: &str| s.get(k).and_then(serde_json::Value::as_str).unwrap_or("").trim().to_string();
        FeedSettings { reviewer: get("pr_reviewer"), org: get("github_org") }
    }
}

/// Je Takt: Einstellungen und Ordner der offenen Agenten weiterreichen, worktrees nach
/// eigenem Merge aufräumen lassen, Ansicht holen.
pub fn tick(feed: &Arc<FeedService>, cleanup: &Arc<CleanupService>, cfg: &FeedSettings, prefix: &str, in_use: &[String], now: f64, lang: Lang) -> (FeedView, Option<CommentPopup>) {
    feed.set_config(FeedConfig {
        reviewer: cfg.reviewer.clone(),
        org: cfg.org.clone(),
        prefix: prefix.to_string(),
        repos_root: deck_core::paths::home_dir().join("Repos"),
    });
    feed.set_in_use(in_use.to_vec());
    clean_after_merge(feed, cleanup, now);
    feed.view(now, lang)
}

/// Nach einem eigenen Merge den worktree entfernen, sobald der Aufräum-Dienst ihn kennt
/// (er braucht den Merge erst im nächsten Abruf). Er prüft seine Regeln selbst (sauber,
/// Kopf = gemergter PR-Kopf, kein offener Agent, keine Stashes); geänderte worktrees
/// bleiben stehen, einer in Benutzung wartet.
fn clean_after_merge(feed: &Arc<FeedService>, cleanup: &Arc<CleanupService>, now: f64) {
    use deck_core::dash::cleanup::WtState;
    let wanted = feed.clean_wanted(now);
    if wanted.is_empty() {
        return;
    }
    let snap = cleanup.snapshot();
    if snap.busy {
        return;
    }
    let mut ready = Vec::new();
    for (repo, branch) in wanted {
        match snap.candidates.iter().find(|c| c.repo.eq_ignore_ascii_case(&repo) && c.branch == branch).map(|c| c.state) {
            Some(WtState::Removable) => ready.push((repo, branch)),
            Some(WtState::Changed) => feed.cleaned(&repo, &branch, None),
            Some(WtState::InUse) | None => {}
        }
    }
    if ready.is_empty() {
        return;
    }
    // git braucht Sekunden: nicht im Takt (der läuft unter der Sperre des Decks). Während
    // der Lauf arbeitet, steht der Aufräum-Dienst auf `busy`, der nächste Takt wartet.
    let (cleanup, feed, candidates) = (cleanup.clone(), feed.clone(), snap.candidates.clone());
    let _ = std::thread::Builder::new().name("feed-cleanup".into()).spawn(move || {
        let removed = cleanup.remove_branches(&ready);
        for (repo, branch) in &ready {
            let folder = candidates
                .iter()
                .find(|c| c.repo.eq_ignore_ascii_case(repo) && c.branch == *branch)
                .map(|c| c.folder.as_str())
                .filter(|f| removed.iter().any(|r| r == f));
            feed.cleaned(repo, branch, folder);
        }
        note(format!("PR-Feed: nach eigenem Merge {} von {} worktrees entfernt", removed.len(), ready.len()));
    });
}

/// Ein Knopf des Feeds. `Ok(text)`: kurz im Fenster zeigen (leer = nichts).
pub fn action(app: &AppHandle, feed: &FeedService, github: &GithubService, command: &str) -> Result<String, String> {
    let reply = feed.action(command)?;
    if command.starts_with("draftSend:") {
        // Die eigene Antwort soll die Karte gleich verschwinden lassen.
        github.refresh();
    }
    feed.kick();
    match reply {
        FeedReply::Quiet => Ok(String::new()),
        FeedReply::Say(t) => Ok(t),
        FeedReply::Open(path) => {
            // Der Entwurf ist eine Textdatei des Decks; geöffnet wird sie im Standard-Editor.
            app.opener().open_path(path.to_string_lossy(), None::<&str>).map_err(|e| e.to_string())?;
            Ok(String::new())
        }
    }
}
