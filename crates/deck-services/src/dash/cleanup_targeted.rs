//! Gezieltes Aufräumen nach einem Merge durch das Deck (PR-Feed, `pr_feed.rs`): nur die
//! worktrees bestimmter Branches, nach denselben Regeln wie „Alle entfernen“.

use super::{lock, mark_busy, notify, remove_some, CleanupService};

impl CleanupService {
    /// Nur die worktrees dieser (Repo, Branch)-Paare entfernen, sofern sie nach den Regeln
    /// entfernbar sind (die Automatik nach einem eigenen Merge). Synchron; die entfernten
    /// Ordner zurück. Leer, wenn schon ein Lauf im Gang ist oder nichts passt.
    pub fn remove_branches(&self, only: &[(String, String)]) -> Vec<String> {
        let todo: Vec<String> = lock(&self.state.snap)
            .removable()
            .filter(|c| only.iter().any(|(r, b)| r.eq_ignore_ascii_case(&c.repo) && *b == c.branch))
            .map(|c| c.folder.clone())
            .collect();
        if todo.is_empty() || !mark_busy(&self.state) {
            return Vec::new();
        }
        let out = remove_some(&self.state, Some(only));
        lock(&self.state.snap).busy = false;
        notify(&self.state);
        let left: Vec<String> = lock(&self.state.snap).candidates.iter().map(|c| c.folder.clone()).collect();
        if out.removed == 0 { Vec::new() } else { todo.into_iter().filter(|f| !left.contains(f)).collect() }
    }
}
