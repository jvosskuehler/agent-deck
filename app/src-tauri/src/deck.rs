//! Das Deck ohne Fenster: Zustand, Poll-Takt und Anzeigemodell.
//!
//! Was `deck/ui/panel.py` und `refresh.py` je Takt tun, in der Reihenfolge von dort:
//! verwaiste Bindungen abräumen, neuen Chat auto-fokussieren, „gesehen“ setzen, Zustände lesen,
//! Hook-Modi übernehmen, Startmodus antreiben, Ansicht bauen, Erweiterungen anstoßen. Die Themen stehen in
//! eigenen Dateien (`window_sync.rs`, `actions.rs`, `ticket.rs`) als `impl Deck`.
//!
//! Alles mit Wirkung nach außen geht über den Windows-Terminal-Host ([`WtHost`]), den
//! `Desktop`-Trait und [`crate::services::Services`]; Tests reichen Attrappen herein.

use std::collections::HashMap;
use std::sync::Arc;

use deck_core::appearance::Appearance;
use deck_core::auto_compact::CompactMemo;
use deck_core::auto_mode::PendingAuto;
use deck_core::bind_store::BindStore;
use deck_core::claude::refs::Refs;
use deck_core::claude::settings::{BYPASS_MODE, DefaultModeCache};
use deck_core::config::{MODE_CYCLE, WINDOWS};
use deck_core::crash_resume::{Pending, ResumeMemo};
use deck_core::deck_view::{DeckInput, StatusMemory, WindowInfo, build_deck_view, chat_worktree, slot_dirs};
use deck_core::front::BackToggle;
use deck_core::i18n::Lang;
use deck_core::slot_state::{SlotState, read_all, read_found_tickets, read_found_worktrees, read_live};
use deck_core::status_model::{adopt_hook_mode, mode_cycle};
use deck_core::view::{DeckView, RefView};
use deck_win::effects::Desktop;

use crate::app_paths::Paths;
use crate::extension::Extensions;
use crate::refs_files::RefsFiles;
use crate::services::Services;
use crate::wt_host::WtHost;

/// Ein per ＋ angeforderter Agent, dessen Slot noch nicht gemeldet ist.
#[derive(Debug, Clone)]
pub struct AwaitNew {
    pub win: String,
    pub before: Vec<String>,
    pub ts: f64,
}

pub struct Deck {
    pub paths: Paths,
    /// Wo die Agenten laufen (`agent_env.rs`); gilt bis zum nächsten Start.
    pub env: deck_core::wsl::AgentEnv,
    /// WSL-Distros zur Wahl im Einstellungsfenster, einmal je Lauf gefragt.
    pub wsl_distros: std::sync::OnceLock<Vec<String>>,
    pub store: BindStore,
    pub wt: WtHost,
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
    /// Buchstabe des Heimat-Blocks (Windows Terminal über allen Repos).
    pub home_win: Option<String>,
    /// Repos-Ordner, wenn er taugt (`deck_settings::usable_repos_root`); gesetzt von
    /// `ensure_home_terminal`, beim Start und bei jeder Änderung der Einstellung.
    pub repos_root: Option<std::path::PathBuf>,
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
    /// Dasselbe für die Statuszeile im Terminal (`state\<slot>.refs.json`).
    refs_files: RefsFiles,
    /// Die Slots der zuletzt gebauten Ansicht, in Anzeigereihenfolge.
    pub shown_slots: Vec<String>,
    /// Taste „alle nach vorn“: das Fenster, aus dem man kam.
    pub front_toggle: BackToggle,
    /// Automatisches `/compact`: für welchen Cache-Stand je Slot schon gesendet wurde.
    pub compact_memo: CompactMemo,
    /// Abgestürzte Agenten, die gerade fortgesetzt werden (`crash_resume.rs`).
    pub resumes: Vec<Pending>,
    /// Wann welche Session zuletzt fortgesetzt wurde (gegen Kreisläufe).
    pub resume_memo: ResumeMemo,
    /// Erweiterungen dieser Ausgabe (`extension.rs`); in der Team-Ausgabe keine.
    pub ext: Extensions,
    /// Letzte Reserve-Pflege (`reserve_agent.rs`).
    pub reserve_ts: f64,
    last_cycle: Option<Vec<String>>,
    memory: StatusMemory,
    mode_cache: DefaultModeCache,
}

impl Deck {
    pub fn new(paths: Paths, mut wt: WtHost, desktop: Arc<dyn Desktop>, services: Services) -> Deck {
        let store = BindStore::load(&paths.app_dir);
        wt.set_color_scheme(Appearance::from_settings(&store.settings).wt_scheme());
        let lang = Lang::from_settings(&paths.claude_settings);
        let refs_files = RefsFiles::new(paths.state_dir.clone());
        Deck {
            env: deck_core::wsl::AgentEnv::from_settings(&store.settings),
            wsl_distros: std::sync::OnceLock::new(),
            paths,
            store,
            wt,
            desktop,
            services,
            lang,
            active_slot: None,
            slot_mode: HashMap::new(),
            mode_ts: HashMap::new(),
            pending_auto: HashMap::new(),
            await_new: None,
            home_win: None,
            repos_root: None,
            modal: false,
            states: HashMap::new(),
            live: HashMap::new(),
            found: HashMap::new(),
            worktrees: HashMap::new(),
            detected_refs: HashMap::new(),
            tile_refs: HashMap::new(),
            refs_files,
            shown_slots: Vec::new(),
            front_toggle: BackToggle::default(),
            compact_memo: CompactMemo::default(),
            resumes: Vec::new(),
            resume_memo: ResumeMemo::default(),
            ext: Extensions::default(),
            reserve_ts: f64::NEG_INFINITY,
            last_cycle: None,
            memory: StatusMemory::default(),
            mode_cache: DefaultModeCache::new(),
        }
    }

    /// Ein Takt des Polls (alle 400 ms). Liefert die neue Ansicht.
    pub fn tick(&mut self, now: f64) -> DeckView {
        self.prune_stale_bindings();
        self.autofocus_new(now);
        self.mark_seen_read(now);
        self.read_files();
        // Ohne das Deck verschwundene Agenten: protokollieren und fortsetzen.
        self.handle_ended(now);
        self.drive_resumes(now);
        let cycle = self.mode_cycle();
        self.adopt_hook_modes(&cycle);
        self.apply_pending_auto(now, &cycle);
        let sessions = self.sessions();
        self.services.set_ticket_prefix(&self.ticket_prefix());
        self.detected_refs = self.services.detected_refs(&sessions);
        let view = self.view(now);
        for e in self.ext.clone().iter() {
            e.on_tick(self, &view, now);
        }
        // Kurz bevor der Prompt-Cache eines wartenden Agenten abläuft: /compact.
        self.auto_compact(now);
        // Einen Agenten versteckt bereithalten, wenn eine Erweiterung das will.
        self.keep_reserve(now);
        // Ticket/PR und Zusammenfassung offener Agenten im Hintergrund vorwärmen.
        let sessions = self.sessions();
        let settled = self.settled_sessions();
        self.services.prefetch(now, &sessions, &settled, self.lang);
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

    /// Sessions der angezeigten Agenten, die gerade nicht arbeiten (fertig, wartend,
    /// untätig): für sie lohnt ein neues Lagebild.
    fn settled_sessions(&self) -> Vec<String> {
        self.shown_slots
            .iter()
            .filter_map(|s| self.states.get(s))
            .filter(|st| !deck_core::prompt_cache::WORKING.contains(&st.get("status").and_then(|v| v.as_str()).unwrap_or("")))
            .filter_map(|st| st.get("session_id").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(String::from))
            .collect()
    }

    fn read_files(&mut self) {
        let dir = &self.paths.state_dir;
        self.states = read_all(dir);
        self.live = read_live(dir);
        self.found = read_found_tickets(dir);
        // Agenten in WSL melden ihren worktree als Linux-Pfad.
        self.worktrees = read_found_worktrees(dir).into_iter().map(|(s, p)| (s, self.env.to_windows(&p))).collect();
        self.drop_inherited();
    }

    /// Meldungen des Vorgängers im selben Slot wegwerfen (`wt::inherited`): der Agent ist
    /// neu, was in seinen Dateien steht, ist älter als seine Shell.
    fn drop_inherited(&mut self) {
        let ts = |st: Option<&SlotState>| st.and_then(|s| deck_core::field::number(s.get("ts")));
        for (slot, start) in self.wt.started() {
            if deck_core::wt::inherited(ts(self.states.get(&slot)), start) {
                deck_core::slot_state::clear_state(&self.paths.state_dir, &slot);
                self.states.remove(&slot);
                self.slot_mode.remove(&slot);
                self.mode_ts.remove(&slot);
            }
            if deck_core::wt::inherited(ts(self.live.get(&slot)), start) {
                let _ = std::fs::remove_file(deck_core::paths::live_path(&self.paths.state_dir, &slot));
                self.live.remove(&slot);
            }
        }
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
            let connected = self.wt.has(w);
            let terminals = if connected { self.wt.terminals(w) } else { Vec::new() };
            let (screen, screen_cols) = if connected { self.wt.screen_rows(w) } else { (Vec::new(), 0) };
            out.push(WindowInfo { letter: w.to_string(), connected, terminals, screen, screen_cols });
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
            .flat_map(|slot| {
                let mut d = slot_dirs(slot, &self.states, &self.live, &self.worktrees);
                d.extend(chat_worktree(slot, &self.states, &self.detected_refs));
                d
            })
            .collect();
        let branches = self.services.branches(&dirs, now);
        let prefix = self.ticket_prefix();
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
            ticket_prefix: &prefix,
            slot_mode: &self.slot_mode,
            mode_cycle: self.last_cycle.as_deref().unwrap_or(&[]),
            active_slot: self.active_slot.as_deref(),
            now,
            lang: self.lang,
            usage,
        };
        let mut view = build_deck_view(&input, &mut self.memory);
        view.orphans = self.services.orphan_rows(now, self.lang);
        self.shown_slots = view.blocks.iter().flat_map(|b| b.tiles.iter().map(|t| t.slot.clone())).collect();
        self.tile_refs = view.blocks.iter().flat_map(|b| b.tiles.iter()).map(|t| (t.slot.clone(), (t.ticket.clone(), t.pr.clone()))).collect();
        self.refs_files.sync(&self.tile_refs);
        // Die Auswahl gilt einer Kachel, die es gibt; sonst bliebe ein Ring an einem
        // später gleichnamigen Agenten hängen.
        if self.active_slot.as_ref().is_some_and(|a| !self.shown_slots.contains(a)) {
            self.active_slot = None;
        }
        view
    }

    /// Projektkürzel für Tickets: das einer Erweiterung, sonst leer (dann zählen nur
    /// ausgeschriebene Keys wie `ABC-123`).
    pub fn ticket_prefix(&self) -> String {
        self.ext.ticket_prefix(&self.store.settings)
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
