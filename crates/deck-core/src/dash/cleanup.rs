//! Aufräumen: worktrees in `Repos\wt\`, deren Branch als PR gemergt ist und
//! die man gefahrlos löschen kann (Konvention des Nutzers: nach dem Merge kommt der
//! worktree weg, der Branch separat).
//!
//! Die Sicherheitsregel steht hier als reine Funktion ([`classify`]), damit der Dienst
//! (`deck-services/src/dash/cleanup.rs`) sie beim Scan und direkt vor dem Löschen
//! gleich anwendet: entfernbar nur, wenn (a) nichts Uncommittetes da ist, (b) der
//! Branch-Kopf GENAU der Kopf eines gemergten PRs desselben Repos und Branches ist und
//! (c) kein offener Agent darin arbeitet.

/// Ab diesem Alter gilt ein Scan als alt (s); der Dienst scannt alle 5 Min.
pub const STALE_AFTER_S: f64 = 15.0 * 60.0;

/// Was mit einem gemergten worktree geht.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WtState {
    /// Sauber, Kopf gemergt, niemand darin: darf weg.
    Removable,
    /// Gemergt, aber mit nicht committeter Arbeit oder einem Kopf, der nicht gemergt
    /// ist (oder git gab keine Antwort). Wird nie angefasst.
    #[default]
    Changed,
    /// Ein offener Agent arbeitet darin. Wird nie angefasst und nicht gezeigt.
    InUse,
}

/// Ein worktree, dessen Branch als PR gemergt ist.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WtCandidate {
    /// Voller Pfad des worktree.
    pub path: String,
    /// Ordnername (`4062-api`).
    pub folder: String,
    /// `owner/name` laut `origin`.
    pub repo: String,
    /// Haupt-Checkout, von dem aus entfernt wird.
    pub main_repo: String,
    pub branch: String,
    pub state: WtState,
}

/// Ergebnis des letzten Entfernen-Laufs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CleanupOutcome {
    /// worktrees entfernt.
    pub removed: u32,
    /// Bei der Prüfung direkt vor dem Löschen nicht mehr entfernbar (stehen gelassen).
    pub skipped: u32,
    /// `git worktree remove` oder `git branch -D` scheiterte.
    pub failed: u32,
}

/// Letzter Stand des Dienstes.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CleanupSnapshot {
    pub candidates: Vec<WtCandidate>,
    /// Ein Entfernen-Lauf ist im Gang.
    pub busy: bool,
    /// Ergebnis des letzten Laufs (Zahlen; den Text baut `board.rs`).
    pub last_result: Option<CleanupOutcome>,
    /// Unix-Sekunden des letzten Scans; 0 = noch keiner.
    pub scanned_at: f64,
    pub error: Option<String>,
}

impl CleanupSnapshot {
    pub fn removable(&self) -> impl Iterator<Item = &WtCandidate> {
        self.candidates.iter().filter(|c| c.state == WtState::Removable)
    }
}

/// Die Sicherheitsregel. `clean`: `git status --porcelain` leer (`None` = keine Antwort,
/// gilt als schmutzig). `head`: `git rev-parse HEAD`. `merged_oids`: Köpfe der gemergten
/// PRs mit demselben Repo und Branch.
pub fn classify(clean: Option<bool>, head: Option<&str>, merged_oids: &[&str], in_use: bool) -> WtState {
    if in_use {
        return WtState::InUse;
    }
    let head = head.map(str::trim).filter(|h| !h.is_empty());
    let head_merged = head.is_some_and(|h| merged_oids.iter().any(|m| m.trim().eq_ignore_ascii_case(h)));
    if clean == Some(true) && head_merged { WtState::Removable } else { WtState::Changed }
}

/// Pfad als Vergleichsschlüssel: `\` statt `/`, klein, ohne Trenner am Ende.
pub fn path_key(p: &str) -> String {
    p.trim().replace('/', "\\").trim_end_matches('\\').to_lowercase()
}

/// Arbeitet ein Agent in `wt`? Gleicher Pfad oder ein Pfad darunter (cwd im Unterordner).
pub fn is_in_use(wt: &str, in_use: &[String]) -> bool {
    let key = path_key(wt);
    if key.is_empty() {
        return false;
    }
    in_use.iter().map(|p| path_key(p)).any(|p| p == key || p.starts_with(&format!("{key}\\")))
}

/// `origin`-URL → `owner/name` (HTTPS, `git@host:` und `ssh://`, mit oder ohne `.git`).
pub fn repo_from_url(url: &str) -> Option<String> {
    let u = url.trim().trim_end_matches('/');
    let u = u.strip_suffix(".git").unwrap_or(u);
    let rest = if let Some(i) = u.find("://") {
        let after = &u[i + 3..];
        &after[after.find('/')? + 1..]
    } else {
        // scp-Form `git@github.com:owner/name`
        &u[u.find(':')? + 1..]
    };
    let mut parts = rest.rsplit('/');
    let name = parts.next().filter(|s| !s.is_empty())?;
    let owner = parts.next().filter(|s| !s.is_empty())?;
    Some(format!("{owner}/{name}"))
}

#[cfg(test)]
#[path = "cleanup_tests.rs"]
mod tests;
