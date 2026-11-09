//! Die Hintergrunddienste aus `crates/deck-services`, so verdrahtet, wie das Deck sie
//! braucht: Usage, KI-Zusammenfassung samt Ticket/PR-Erkennung, git-Branches der
//! Agenten-Ordner (gecacht), worktrees (Aufräumen beim Schließen, Sweep im Takt).
//!
//! Jeder Dienst ist optional: Tests bauen [`Services::inert`] ohne einen davon (nichts
//! läuft an, nichts geht nach außen), die App [`Services::system`] mit allen.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use deck_core::claude::refs::Refs;
use deck_core::config::{HOVER_SUMMARY, HOVER_SUMMARY_PREFETCH, TICKET_AUTODETECT};
use deck_core::git_head::{BranchCache, GitFs, NoGit};
use deck_core::i18n::Lang;
use deck_core::view::{OrphanView, UsageView};
use deck_services::orphans::{OrphanWatch, Win32Source};
use deck_services::summary::{ChatInfo, InfoCallback, SummaryService};
use deck_services::usage::UsageService;
use deck_services::worktree_sweep::{SweepView, WorktreeSweeper};

use crate::app_paths::Paths;

/// So oft (s) wird nach Chat-Infos offener Agenten geschaut (Python `PREFETCH_EVERY_S`).
const PREFETCH_EVERY_S: f64 = 5.0;
/// Alte Zusammenfassungen so lange behalten (Tage, Python `summarize.prune`).
const SUMMARY_KEEP_DAYS: f64 = 14.0;

/// Weckt den Takt, wenn ein Dienst etwas Neues hat.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

pub struct Services {
    usage: Option<UsageService>,
    summary: Option<Arc<SummaryService>>,
    sweeper: Option<WorktreeSweeper>,
    /// „Läuft noch ohne Fenster“ (eigener Scan-Thread, `crate::orphans`).
    orphans: Option<Arc<OrphanWatch>>,
    refs: Arc<Mutex<HashMap<String, Refs>>>,
    wake: Wake,
    /// Zusammenfassungen vorab erzeugen (kostet `claude`-Läufe).
    prefetch_summaries: bool,
    last_prefetch: f64,
    /// Liest `.git/HEAD` (Tests: nie).
    git: Box<dyn GitFs>,
    /// Gelesene Branches je Ordner, höchstens alle paar Sekunden neu.
    branch_cache: BranchCache,
}

impl Services {
    /// Ohne jeden Dienst (Tests).
    pub fn inert() -> Services {
        Services {
            usage: None,
            summary: None,
            sweeper: None,
            orphans: None,
            refs: Arc::default(),
            wake: Arc::new(|| {}),
            prefetch_summaries: false,
            last_prefetch: f64::NEG_INFINITY,
            git: Box::new(NoGit),
            branch_cache: BranchCache::default(),
        }
    }

    /// Alle Dienste, echt.
    pub fn system(paths: &Paths, lang: Lang, wake: Wake) -> Services {
        let summary = Arc::new(SummaryService::system(&paths.state_dir));
        summary.prune(SUMMARY_KEEP_DAYS);
        let usage = UsageService::system(lang);
        let w = wake.clone();
        usage.start(Box::new(move |_| w()));
        let orphans = Arc::new(OrphanWatch::new(Arc::new(Win32Source), &paths.app_dir, &paths.state_dir));
        crate::orphans::start(orphans.clone(), wake.clone());
        Services {
            usage: Some(usage),
            summary: Some(summary),
            sweeper: Some(WorktreeSweeper::system(&paths.state_dir)),
            orphans: Some(orphans),
            refs: Arc::default(),
            wake,
            prefetch_summaries: HOVER_SUMMARY && HOVER_SUMMARY_PREFETCH,
            last_prefetch: f64::NEG_INFINITY,
            git: Box::new(deck_services::git_fs::StdGitFs),
            branch_cache: BranchCache::default(),
        }
    }

    /// Ab sofort mit diesem Jira-Projekt im Chat suchen (`jira_prefix`).
    pub fn set_jira_prefix(&self, prefix: &str) {
        if let Some(s) = &self.summary {
            s.set_project_key(Some(prefix));
        }
    }

    // ── Branches ─────────────────────────────────────────────────────────────────

    /// git-Branch je Ordner der angezeigten Agenten. Läuft im 400-ms-Takt; gelesen wird
    /// ein Ordner aber höchstens alle `git_head::BRANCH_TTL_S` Sekunden, sonst kommt der
    /// Wert aus dem Cache. Ordner, nach denen keiner mehr fragt, fallen heraus.
    pub fn branches(&mut self, dirs: &[String], now: f64) -> HashMap<String, String> {
        self.branch_cache.retain(dirs);
        let mut out = HashMap::new();
        for d in dirs {
            if let Some(b) = self.branch_cache.get(d, now, self.git.as_ref()) {
                out.insert(d.clone(), b);
            }
        }
        out
    }

    pub fn set_lang(&mut self, lang: Lang) {
        if let Some(u) = &self.usage {
            u.set_lang(lang);
        }
    }

    // ── worktrees ────────────────────────────────────────────────────────────────

    /// Beim Schließen eines Agenten seinen worktree entfernen (im Hintergrund; einer
    /// mit nicht committeter Arbeit bleibt stehen).
    pub fn cleanup_slot(&self, slot: &str, ticket: Option<&str>) {
        if let Some(s) = &self.sweeper {
            s.cleanup_slot(slot, ticket);
        }
    }

    /// Verwaiste worktrees fegen. Rückgabe: Slots, deren Ticket die App vergisst.
    pub fn sweep(&mut self, view: &SweepView) -> Vec<String> {
        self.sweeper.as_mut().map(|s| s.tick(view)).unwrap_or_default()
    }

    // ── Läuft noch ohne Fenster ─────────────────────────────────────────────────

    pub fn orphan_rows(&self, now: f64, lang: Lang) -> Vec<OrphanView> {
        self.orphans.as_ref().map(|o| o.rows(now, lang)).unwrap_or_default()
    }

    /// Der Dienst selbst, für die Befehle „Beenden“ (laufen ohne Sperre des Decks).
    pub fn orphan_watch(&self) -> Option<Arc<OrphanWatch>> {
        self.orphans.clone()
    }

    // ── Usage ────────────────────────────────────────────────────────────────────

    pub fn usage_view(&self, lang: Lang) -> Option<UsageView> {
        self.usage.as_ref().map(|u| u.view(lang))
    }

    // ── Zusammenfassung und Bezüge ───────────────────────────────────────────────

    pub fn summary(&self, session_id: &str) -> Option<String> {
        let s = self.summary.as_ref()?;
        (!session_id.is_empty()).then(|| s.cached_summary(session_id)).flatten()
    }

    /// Bezüge und (mit `summarize`) die Zusammenfassung im Hintergrund sicherstellen.
    /// Was ankommt, weckt den Takt (Karte und Tooltip ziehen nach).
    pub fn request_info(&self, session_id: &str, cwd: &str, lang: Lang, summarize: bool) {
        let Some(s) = &self.summary else { return };
        if !(TICKET_AUTODETECT || summarize) {
            return;
        }
        let (refs, wake) = (self.refs.clone(), self.wake.clone());
        let cb: InfoCallback = Arc::new(move |sid: &str, info: ChatInfo| {
            if let ChatInfo::Refs(r) = info
                && let Ok(mut m) = refs.lock()
            {
                m.insert(sid.to_string(), r);
            }
            wake();
        });
        s.request(session_id, Some(cwd).filter(|c| !c.is_empty()), lang, summarize && HOVER_SUMMARY, cb);
    }

    /// Chat-Infos offener Agenten vorab holen, gedrosselt (alle 5 s).
    pub fn prefetch(&mut self, now: f64, sessions: &[(String, String)], lang: Lang) {
        if now - self.last_prefetch < PREFETCH_EVERY_S {
            return;
        }
        self.last_prefetch = now;
        for (sid, cwd) in sessions {
            self.request_info(sid, cwd, lang, self.prefetch_summaries);
        }
    }

    /// Erkannte Bezüge: frische aus den Aufträgen, sonst aus dem Cache auf der Platte
    /// (der überlebt einen Neustart des Decks).
    pub fn detected_refs(&self, sessions: &[(String, String)]) -> HashMap<String, Refs> {
        let mut out = self.refs.lock().map(|m| m.clone()).unwrap_or_default();
        if let Some(s) = &self.summary {
            for (sid, _) in sessions {
                if !out.contains_key(sid) {
                    let r = s.cached_refs(sid);
                    if let Ok(mut m) = self.refs.lock() {
                        m.insert(sid.clone(), r.clone());
                    }
                    out.insert(sid.clone(), r);
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_services_nothing_runs_and_nothing_breaks() {
        let mut s = Services::inert();
        assert!(s.usage_view(Lang::German).is_none());
        assert!(s.sweep(&SweepView::default()).is_empty());
        assert_eq!(s.summary("abc"), None);
        s.prefetch(100.0, &[("abc".into(), String::new())], Lang::German);
        assert!(s.detected_refs(&[]).is_empty());
        assert!(s.branches(&["C:/Repos/agent-deck".into()], 100.0).is_empty(), "ohne Dateisystem kein Branch");
        s.set_jira_prefix("ORPH");
        assert!(s.orphan_rows(100.0, Lang::German).is_empty() && s.orphan_watch().is_none());
    }
}
