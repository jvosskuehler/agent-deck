//! Das Deck ohne Fenster: Zustand, Poll-Takt und Anzeigemodell.
//!
//! Was `deck/ui/panel.py` und `refresh.py` je Takt tun, in der Reihenfolge von dort:
//! Bindungen nachziehen, geschlossene Fenster abräumen, neuen Chat auto-fokussieren, „gesehen“ setzen, Zustände lesen, worktrees fegen,
//! Hook-Modi übernehmen, Startmodus antreiben, Ansicht bauen. Die Themen stehen in
//! eigenen Dateien (`window_sync.rs`, `actions.rs`, `ticket.rs`) als `impl Deck`.
//!
//! Alles mit Wirkung nach außen geht über die Hosts (Broker, Windows Terminal), den
//! `Desktop`-Trait und [`crate::services::Services`]; Tests reichen Attrappen herein.

use std::collections::HashMap;
use std::sync::Arc;

use deck_core::auto_compact::CompactMemo;
use deck_core::auto_mode::PendingAuto;
use deck_core::bind_store::BindStore;
use deck_core::claude::refs::Refs;
use deck_core::claude::settings::{BYPASS_MODE, DefaultModeCache};
use deck_core::config::{MODE_CYCLE, WINDOWS};
use deck_core::deck_view::{DeckInput, StatusMemory, WindowInfo, build_deck_view, slot_dirs};
use deck_core::front::BackToggle;
use deck_core::i18n::Lang;
use deck_core::slot_state::{SlotState, read_all, read_found_tickets, read_found_worktrees, read_live};
use deck_core::status_model::{adopt_hook_mode, mode_cycle};
use deck_core::view::{DeckView, Host, RefView};
use deck_services::worktree_sweep::SweepView;
use deck_win::effects::Desktop;

use crate::app_paths::Paths;
use crate::block_head::BindRequest;
use crate::hosts::Hosts;
use crate::services::Services;

/// Ein per ＋ angeforderter Agent, dessen Slot noch nicht gemeldet ist.
#[derive(Debug, Clone)]
pub struct AwaitNew {
    pub win: String,
    pub before: Vec<String>,
    pub ts: f64,
}

pub struct Deck {
    pub paths: Paths,
    pub store: BindStore,
    pub hosts: Hosts,
    pub desktop: Arc<dyn Desktop>,
    pub services: Services,
    pub lang: Lang,
    /// Zuletzt angeklickte bzw. fokussierte Kachel (weißer Ring).
    pub active_slot: Option<String>,
    /// Slot → angenommener Permission-Modus (Index in der Shift+Tab-Reihenfolge).
    pub slot_mode: HashMap<String, usize>,
    /// Slot → `ts` der zuletzt übernommenen Hook-Meldung mit Modus.
    pub mode_ts: HashMap<String, f64>,
    pub pending_auto: HashMap<String, PendingAuto>,
    pub await_new: Option<AwaitNew>,
    /// Laufendes „Mit Fenster verbinden“ (Kopf eines Blocks).
    pub bind: Option<BindRequest>,
    /// Fenster → seit wann getrennt UND ohne offenes VS-Code-Fenster.
    pub gone_since: HashMap<String, f64>,
    /// Buchstabe des Heimat-Blocks (Windows Terminal über allen Repos).
    pub home_win: Option<String>,
    /// Ein Dialog des Decks hat den Fokus: Auto-Fokus auslassen (er klaute ihn sonst).
    pub modal: bool,
    pub states: HashMap<String, SlotState>,
    pub live: HashMap<String, SlotState>,
    pub found: HashMap<String, String>,
    pub worktrees: HashMap<String, String>,
    /// Session-ID → im Chat erkannte Ticket-/PR-Bezüge (vom Hintergrund gefüllt).
    pub detected_refs: HashMap<String, Refs>,
    /// Slot → Ticket und PR, wie die zuletzt gebaute Ansicht sie zeigt (für den Tooltip).
    pub tile_refs: HashMap<String, (Option<RefView>, Option<RefView>)>,
    /// Die Slots der zuletzt gebauten Ansicht, in Anzeigereihenfolge.
    pub shown_slots: Vec<String>,
    /// Taste „alle nach vorn“: das Fenster, aus dem man kam.
    pub front_toggle: BackToggle,
    /// Automatisches `/compact`: für welchen Cache-Stand je Slot schon gesendet wurde.
    pub compact_memo: CompactMemo,
    last_cycle: Option<Vec<String>>,
    memory: StatusMemory,
    mode_cache: DefaultModeCache,
}

impl Deck {
    pub fn new(paths: Paths, hosts: Hosts, desktop: Arc<dyn Desktop>, services: Services) -> Deck {
        let store = BindStore::load(&paths.app_dir);
        let lang = Lang::from_settings(&paths.claude_settings);
        Deck {
            paths,
            store,
            hosts,
            desktop,
            services,
            lang,
            active_slot: None,
            slot_mode: HashMap::new(),
            mode_ts: HashMap::new(),
            pending_auto: HashMap::new(),
            await_new: None,
            bind: None,
            gone_since: HashMap::new(),
            home_win: None,
            modal: false,
            states: HashMap::new(),
            live: HashMap::new(),
            found: HashMap::new(),
            worktrees: HashMap::new(),
            detected_refs: HashMap::new(),
            tile_refs: HashMap::new(),
            shown_slots: Vec::new(),
            front_toggle: BackToggle::default(),
            compact_memo: CompactMemo::default(),
            last_cycle: None,
            memory: StatusMemory::default(),
            mode_cache: DefaultModeCache::new(),
        }
    }

    /// Ein Takt des Polls (alle 400 ms). Liefert die neue Ansicht.
    pub fn tick(&mut self, now: f64) -> DeckView {
        self.sync_bindings();
        self.poll_bind(now);
        self.cleanup_closed_windows(now);
        self.autofocus_new(now);
        self.mark_seen_read(now);
        self.read_files();
        let cycle = self.mode_cycle();
        let view = self.sweep_view(now);
        let orphans = self.services.sweep(&view);
        let mut changed = false;
        for slot in &orphans {
            changed |= self.store.tickets.remove(slot).is_some();
        }
        if changed {
            self.store.save_tickets();
        }
        self.adopt_hook_modes(&cycle);
        self.apply_pending_auto(now, &cycle);
        let sessions = self.sessions();
        self.services.set_jira_prefix(&self.jira_prefix());
        self.detected_refs = self.services.detected_refs(&sessions);
        let view = self.view(now);
        // Kurz bevor der Prompt-Cache eines wartenden Agenten abläuft: /compact.
        self.auto_compact(now);
        // Ticket/PR und Zusammenfassung offener Agenten im Hintergrund vorwärmen.
        let sessions = self.sessions();
        self.services.prefetch(now, &sessions, self.lang);
        view
    }

    /// `(session_id, cwd)` der angezeigten Agenten.
    pub fn sessions(&self) -> Vec<(String, String)> {
        let field = |st: &SlotState, k: &str| st.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        self.shown_slots
            .iter()
            .filter_map(|s| self.states.get(s))
            .map(|st| (field(st, "session_id"), field(st, "cwd")))
            .filter(|(sid, _)| !sid.is_empty())
            .collect()
    }

    /// Was der worktree-Sweep über diesen Takt wissen muss.
    fn sweep_view(&mut self, now: f64) -> SweepView {
        let mut v = SweepView { now, bindings: self.store.bindings.clone(), ..SweepView::default() };
        v.markers = self.worktrees.iter().map(|(k, p)| (k.clone(), p.clone())).collect();
        for (slot, st) in &self.states {
            if let Some(cwd) = st.get("cwd").and_then(|c| c.as_str()).filter(|c| !c.is_empty()) {
                v.cwds.insert(slot.clone(), cwd.to_string());
            }
        }
        v.tickets = self.found.iter().map(|(k, t)| (k.clone(), t.clone())).collect();
        v.tickets.extend(self.store.tickets.iter().map(|(k, t)| (k.clone(), t.clone())));
        for w in WINDOWS {
            if self.hosts.connected(w) {
                v.connected.insert(w.to_string());
                v.live.extend(self.hosts.terminals(w));
            }
        }
        v
    }

    fn read_files(&mut self) {
        let dir = &self.paths.state_dir;
        self.states = read_all(dir);
        self.live = read_live(dir);
        self.found = read_found_tickets(dir);
        self.worktrees = read_found_worktrees(dir);
    }

    /// Shift+Tab-Reihenfolge der Agenten. Steht Bypass als Startmodus in settings.json,
    /// nimmt Claude Code ihn in die Reihenfolge auf. Ändert sie sich, zeigen gemerkte
    /// Modus-Nummern auf falsche Namen: dann vergessen, die Hooks liefern sie neu.
    pub fn mode_cycle(&mut self) -> Vec<String> {
        let bypass = self.mode_cache.get(&self.paths.claude_settings).as_ref().and_then(|v| v.as_str()) == Some(BYPASS_MODE);
        let cycle = mode_cycle(&MODE_CYCLE, bypass);
        if self.last_cycle.as_ref().is_some_and(|c| *c != cycle) {
            self.slot_mode.clear();
            self.mode_ts.clear();
        }
        self.last_cycle = Some(cycle.clone());
        cycle
    }

    /// Ist-Permission-Modus aus den Hooks übernehmen (jede neuere Meldung gewinnt).
    fn adopt_hook_modes(&mut self, cycle: &[String]) {
        for (slot, st) in &self.states {
            let prev = self.mode_ts.get(slot).copied().unwrap_or(0.0);
            if let Some((i, ts)) = adopt_hook_mode(prev, st, cycle) {
                self.slot_mode.insert(slot.clone(), i);
                self.mode_ts.insert(slot.clone(), ts);
            }
        }
    }

    /// Was die App über jedes Fenster weiß, für das Anzeigemodell.
    fn window_infos(&mut self) -> Vec<WindowInfo> {
        let mut out = Vec::new();
        for w in WINDOWS {
            let connected = self.hosts.connected(w);
            let host = if self.hosts.is_wt(w) { Host::Terminal } else { Host::Vscode };
            let terminals = if connected { self.hosts.terminals(w) } else { Vec::new() };
            let (screen, screen_cols) =
                if connected && host == Host::Terminal { self.hosts.screen_rows(w) } else { (Vec::new(), 0) };
            out.push(WindowInfo {
                letter: w.to_string(),
                host,
                connected,
                remote: self.hosts.remote(w).is_some(),
                terminals,
                screen,
                screen_cols,
            });
        }
        out
    }

    /// Die Ansicht aus dem zuletzt gelesenen Stand bauen.
    pub fn view(&mut self, now: f64) -> DeckView {
        let windows = self.window_infos();
        let usage = self.services.usage_view(self.lang);
        // Branches nur für die Ordner der Agenten, die gleich gezeigt werden.
        let dirs: Vec<String> = windows
            .iter()
            .filter(|w| w.connected)
            .flat_map(|w| w.terminals.iter())
            .flat_map(|slot| slot_dirs(slot, &self.states, &self.live, &self.worktrees))
            .collect();
        let branches = self.services.branches(&dirs, now);
        let prefix = self.jira_prefix();
        let input = DeckInput {
            windows: &windows,
            store: &self.store,
            home: self.home_win.as_deref(),
            states: &self.states,
            live: &self.live,
            found_tickets: &self.found,
            worktrees: &self.worktrees,
            detected_refs: &self.detected_refs,
            branches: &branches,
            jira_prefix: &prefix,
            slot_mode: &self.slot_mode,
            mode_cycle: self.last_cycle.as_deref().unwrap_or(&[]),
            active_slot: self.active_slot.as_deref(),
            now,
            lang: self.lang,
            usage,
        };
        let mut view = build_deck_view(&input, &mut self.memory);
        for b in &mut view.blocks {
            b.binding = self.binding(&b.letter);
        }
        self.shown_slots = view.blocks.iter().flat_map(|b| b.tiles.iter().map(|t| t.slot.clone())).collect();
        self.tile_refs = view.blocks.iter().flat_map(|b| b.tiles.iter()).map(|t| (t.slot.clone(), (t.ticket.clone(), t.pr.clone()))).collect();
        // Die Auswahl gilt einer Kachel, die es gibt; sonst bliebe ein Ring an einem
        // später gleichnamigen Agenten hängen.
        if self.active_slot.as_ref().is_some_and(|a| !self.shown_slots.contains(a)) {
            self.active_slot = None;
        }
        view
    }

    /// Jira-Projekt aus `deck_settings.json` (`jira_prefix`), sonst der Standard.
    pub fn jira_prefix(&self) -> String {
        self.setting_str("jira_prefix").unwrap_or_else(|| deck_core::config::JIRA_PROJECT_KEY.to_string())
    }

    /// Zustand eines Slots aus dem letzten Takt.
    pub fn state_of(&self, slot: &str) -> Option<&SlotState> {
        self.states.get(slot)
    }

    /// Der Buchstabe eines Slots (`B3` → `B`).
    pub fn letter(slot: &str) -> String {
        slot.chars().take(1).collect()
    }
}
