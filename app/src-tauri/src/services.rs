//! Die Hintergrunddienste aus `crates/deck-services`, so verdrahtet, wie das Deck sie
//! braucht: Usage, KI-Zusammenfassung samt Ticket/PR-Erkennung, worktrees (Aufräumen
//! beim Schließen, Sweep im Takt), VS-Code-Neustart, Fenster einsammeln, Ring um den
//! Chat.
//!
//! Jeder Dienst ist optional: Tests bauen [`Services::inert`] ohne einen davon (nichts
//! läuft an, nichts geht nach außen), die App [`Services::system`] mit allen.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use deck_core::claude::refs::Refs;
use deck_core::config::{HOVER_SUMMARY, HOVER_SUMMARY_PREFETCH, TICKET_AUTODETECT};
use deck_core::i18n::Lang;
use deck_core::protocol::Command;
use deck_core::restart_plan::Plan;
use deck_core::view::UsageView;
use deck_services::restart::{EditorBridge, Reporter, Restarter};
use deck_services::restart_host::{PlanFile, SystemRestartHost};
use deck_services::summary::{ChatInfo, InfoCallback, SummaryService};
use deck_services::usage::UsageService;
use deck_services::worktree_sweep::{SweepView, WorktreeSweeper};
use deck_services::{gather, vscode_glow};
use deck_win::effects::{Desktop, Processes};

use crate::app_paths::Paths;
use crate::applog::note;
use crate::hosts::VsCodeHost;

/// So oft (s) wird nach Chat-Infos offener Agenten geschaut (Python `PREFETCH_EVERY_S`).
const PREFETCH_EVERY_S: f64 = 5.0;
/// Alte Zusammenfassungen so lange behalten (Tage, Python `summarize.prune`).
const SUMMARY_KEEP_DAYS: f64 = 14.0;

/// Der Neustart spricht nur mit VS Code, nie mit Windows-Terminal-Blöcken: er bekommt
/// den rohen Broker (wie in Python).
struct BrokerBridge(Arc<dyn VsCodeHost>);

fn letter(slot: &str) -> String {
    slot.chars().take(1).collect()
}

impl EditorBridge for BrokerBridge {
    fn save_all(&self, window: &str) -> bool {
        self.0.send(window, &Command::SaveAll)
    }
    fn close_agent(&self, slot: &str) -> bool {
        self.0.send(&letter(slot), &Command::CloseAgent { slot: slot.into() })
    }
    fn close_window(&self, window: &str) -> bool {
        self.0.send(window, &Command::CloseWindow)
    }
    fn create_agent(&self, window: &str, slot: &str, args: &str, cwd: &str) -> bool {
        let opt = |s: &str| Some(s.to_string()).filter(|s| !s.is_empty());
        self.0.send(window, &Command::CreateAgent { model: None, args: opt(args), cwd: opt(cwd), slot: opt(slot) })
    }
    fn connected(&self, window: &str) -> bool {
        self.0.connected(window)
    }
    fn terminals(&self, window: &str) -> Vec<String> {
        self.0.terminals(window)
    }
}

/// Weckt den Takt, wenn ein Dienst etwas Neues hat.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

pub struct Services {
    usage: Option<UsageService>,
    summary: Option<Arc<SummaryService>>,
    sweeper: Option<WorktreeSweeper>,
    restarter: Option<Restarter>,
    desktop: Option<Arc<dyn Desktop>>,
    refs: Arc<Mutex<HashMap<String, Refs>>>,
    wake: Wake,
    /// Zusammenfassungen vorab erzeugen (kostet `claude`-Läufe; im Probebetrieb aus,
    /// dort erzeugt sie noch das Python-Deck in denselben Cache).
    prefetch_summaries: bool,
    last_prefetch: f64,
    glow_css: PathBuf,
}

impl Services {
    /// Ohne jeden Dienst (Tests).
    pub fn inert() -> Services {
        Services {
            usage: None,
            summary: None,
            sweeper: None,
            restarter: None,
            desktop: None,
            refs: Arc::default(),
            wake: Arc::new(|| {}),
            prefetch_summaries: false,
            last_prefetch: f64::NEG_INFINITY,
            glow_css: PathBuf::new(),
        }
    }

    /// Alle Dienste, echt. `trial`: Probebetrieb neben dem Python-Deck.
    pub fn system(paths: &Paths, vscode: Arc<dyn VsCodeHost>, desktop: Arc<dyn Desktop>, procs: Arc<dyn Processes>, lang: Lang, trial: bool, wake: Wake) -> Services {
        let summary = Arc::new(SummaryService::system(&paths.state_dir));
        summary.prune(SUMMARY_KEEP_DAYS);
        let usage = UsageService::system(lang);
        let w = wake.clone();
        usage.start(Box::new(move |_| w()));
        let host = SystemRestartHost::new(desktop.clone(), procs);
        let mut restarter = Restarter::new(Box::new(BrokerBridge(vscode)), Box::new(host), Box::new(PlanFile::standard()), lang);
        restarter.set_log(Box::new(|t| note(format!("VS-Code-Neustart: {t}"))));
        Services {
            usage: Some(usage),
            summary: Some(summary),
            sweeper: Some(WorktreeSweeper::system(&paths.state_dir)),
            restarter: Some(restarter),
            desktop: Some(desktop),
            refs: Arc::default(),
            wake,
            prefetch_summaries: !trial && HOVER_SUMMARY && HOVER_SUMMARY_PREFETCH,
            last_prefetch: f64::NEG_INFINITY,
            glow_css: paths.glow_css(),
        }
    }

    pub fn set_lang(&mut self, lang: Lang) {
        if let Some(u) = &self.usage {
            u.set_lang(lang);
        }
        if let Some(r) = &mut self.restarter {
            r.set_lang(lang);
        }
    }

    // ── Neustart von VS Code ────────────────────────────────────────────────────

    /// Einen laufenden Neustart weiterschieben. `true` = er läuft noch (dann wird nichts
    /// abgeräumt: die Fenster sind absichtlich weg).
    pub fn restart_step(&mut self) -> bool {
        match &mut self.restarter {
            Some(r) => {
                r.step();
                r.busy()
            }
            None => false,
        }
    }

    /// Den Neustart beginnen. `false` = nichts zu tun (Grund steht im Log).
    pub fn restart_vscode(&mut self, plan: Plan, report: Reporter) -> bool {
        match &mut self.restarter {
            Some(r) => r.start(&|| plan.clone(), Some(report)),
            None => false,
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
    pub fn sweep(&mut self, view: &SweepView, paused: bool) -> Vec<String> {
        self.sweeper.as_mut().map(|s| s.tick(view, paused)).unwrap_or_default()
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

    // ── Werkzeuge der Einstellungen ──────────────────────────────────────────────

    /// Alle VS-Code-Fenster auf den Hauptmonitor; `None` ohne Desktop (Tests).
    pub fn gather_windows(&self, lang: Lang) -> Option<gather::GatherResult> {
        let d = self.desktop.as_ref()?;
        Some(gather::gather_to_primary(d.as_ref(), lang))
    }

    /// Ring eingeschaltet, Patch aber weg (VS-Code-Update ersetzt workbench.html): still
    /// neu einspielen, im Hintergrund (Python `_glow_self_heal`).
    pub fn glow_self_heal(&self, enabled: bool, lang: Lang) {
        if !enabled || self.desktop.is_none() {
            return;
        }
        let css = self.glow_css.clone();
        std::thread::spawn(move || {
            let st = vscode_glow::status();
            if st.workbench_files > 0 && !st.installed {
                let r = vscode_glow::set_glow(true, &css, lang);
                note(format!("Ring nach VS-Code-Update neu eingespielt: {}/{}", r.done, r.total));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeVsCode;

    #[test]
    fn the_restart_talks_to_vscode_through_the_broker_vocabulary() {
        let vs = Arc::new(FakeVsCode::default());
        vs.open("repo", Some("B"), &["B1"]);
        let bridge = BrokerBridge(vs.clone());
        assert!(bridge.save_all("B"));
        assert!(bridge.create_agent("B", "B2", "--resume abc", "C:/repo"));
        assert!(bridge.close_agent("B1"));
        assert!(!bridge.close_window("C"), "nicht verbunden");
        let sent: Vec<Command> = vs.sent().into_iter().map(|(_, c)| c).collect();
        assert_eq!(
            sent,
            vec![
                Command::SaveAll,
                Command::CreateAgent { model: None, args: Some("--resume abc".into()), cwd: Some("C:/repo".into()), slot: Some("B2".into()) },
                Command::CloseAgent { slot: "B1".into() },
            ]
        );
        assert_eq!(bridge.terminals("B"), vec!["B1"]);
    }

    #[test]
    fn without_services_nothing_runs_and_nothing_breaks() {
        let mut s = Services::inert();
        assert!(!s.restart_step());
        assert!(s.usage_view(Lang::German).is_none());
        assert!(s.sweep(&SweepView::default(), false).is_empty());
        assert_eq!(s.summary("abc"), None);
        s.prefetch(100.0, &[("abc".into(), String::new())], Lang::German);
        assert!(s.detected_refs(&[]).is_empty());
    }
}
