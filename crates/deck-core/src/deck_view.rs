//! Aus Zustandsdateien, Bindungen und Reihenfolge das fertige `view::DeckView` bauen.
//!
//! Die Rechnung der Python-Poll-Schleife (`ui/refresh.py`, `ui/tiles.py`) ohne Zeichnen:
//! Status normalisieren und Frische prüfen, „getrennt“ errechnen, Effort-Kollision
//! auflösen, Ticket und PR wählen (`ref_sources.rs`), Cache-Restzeit, Blöcke und Kacheln
//! ordnen, Heimat-Block oben. Was nur die App wissen kann (Verbindung, lebende Agenten je
//! Block, Fensterlage), kommt als `WindowInfo` herein.

use std::collections::HashMap;

use crate::bind_store::BindStore;
use crate::claude::refs::Refs;
use crate::config::{STALE_S, TICKET_AUTODETECT_ON_CARD, WINDOWS};
use crate::i18n::{Lang, status_label};
use crate::ordering::{apply_order, pin_first};
use crate::field::text;
use crate::prompt_cache::tile_cache;
use crate::ref_sources::{RefEvidence, pick_pr, pick_ticket, ref_dirs};
use crate::slot_state::SlotState;
use crate::status_model::{VALID_STATUSES, dominant_status, initial_status, is_fresh, is_lost, normalize_status, resolve_effort};
use crate::view::{BlockView, DeckView, RefView, Status, TileView, UsageView};

pub use crate::ref_sources::TICKET_MAX_CHARS;

/// Was die App über ein Fenster weiß.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowInfo {
    pub letter: String,
    pub connected: bool,
    /// Gemeldete Slots in Melde-Reihenfolge.
    pub terminals: Vec<String>,
    /// Wie die Fenster der Agenten auf dem Bildschirm liegen, als
    /// Reihen (`arrange::screen_rows`). Leer = unbekannt, dann gilt die gezogene
    /// Reihenfolge. Minimierte und nicht gefundene Fenster fehlen hier.
    pub screen: Vec<Vec<String>>,
    /// So viele Fenster passen auf dem Monitor des Blocks
    /// nebeneinander (`arrange::max_cols_for`). 0 = unbekannt.
    pub screen_cols: u32,
}

/// Alles, woraus die Ansicht entsteht.
pub struct DeckInput<'a> {
    pub windows: &'a [WindowInfo],
    /// Bindungen, gezogene Reihenfolgen, zugewiesene Tickets, gemerktes Effort.
    pub store: &'a BindStore,
    /// Buchstabe des Heimat-Blocks (steht immer oben).
    pub home: Option<&'a str>,
    pub states: &'a HashMap<String, SlotState>,
    pub live: &'a HashMap<String, SlotState>,
    /// Vom Agenten gemeldete Ticket-IDs (`<slot>.ticket`).
    pub found_tickets: &'a HashMap<String, String>,
    /// Gemeldete worktrees (`<slot>.worktree`).
    pub worktrees: &'a HashMap<String, String>,
    /// Im Chat erkannte Bezüge je Session-ID.
    pub detected_refs: &'a HashMap<String, Refs>,
    /// git-Branch je Ordner (`dirs_of`), von der App gelesen; fehlt = keiner.
    pub branches: &'a HashMap<String, String>,
    /// Jira-Projekt (`jira_prefix`), z. B. `ORPH`.
    pub jira_prefix: &'a str,
    /// Gemerkter Modus-Index je Slot (Index in `mode_cycle`).
    pub slot_mode: &'a HashMap<String, usize>,
    pub mode_cycle: &'a [String],
    /// Zuletzt angeklickte bzw. fokussierte Kachel.
    pub active_slot: Option<&'a str>,
    pub now: f64,
    pub lang: Lang,
    pub usage: Option<UsageView>,
}

/// Merkt sich je Slot den zuletzt gezeigten Status und seit wann, für
/// `TileView::status_changed_at` (die Oberfläche lässt die Kachel dann aufblühen).
#[derive(Debug, Default)]
pub struct StatusMemory {
    seen: HashMap<String, (Status, f64)>,
}

impl StatusMemory {
    pub fn observe(&mut self, slot: &str, status: Status, now: f64) -> f64 {
        match self.seen.get(slot) {
            Some((s, at)) if *s == status => *at,
            _ => {
                self.seen.insert(slot.to_string(), (status, now));
                now
            }
        }
    }
}

/// Interner Status-Name → Anzeigemodell (`running` ist `thinking`).
pub fn to_status(key: &str) -> Status {
    match key {
        "thinking" | "running" => Status::Thinking,
        "waiting" => Status::Waiting,
        "done" => Status::Done,
        "lost" => Status::Lost,
        "none" => Status::None,
        _ => Status::Idle,
    }
}

/// Die Ordner eines Slots (`ref_sources::ref_dirs`): gemeldeter worktree, aktueller
/// Ordner aus `live.json`, cwd der Hooks.
pub fn slot_dirs(
    slot: &str,
    states: &HashMap<String, SlotState>,
    live: &HashMap<String, SlotState>,
    worktrees: &HashMap<String, String>,
) -> Vec<String> {
    let cwd = |m: &HashMap<String, SlotState>| m.get(slot).and_then(|s| text(s.get("cwd"))).map(String::from);
    let (lv, st) = (cwd(live), cwd(states));
    ref_dirs(worktrees.get(slot).map(String::as_str), lv.as_deref(), st.as_deref())
}

/// Modellname fürs Kartenlabel kürzen: „Opus 5 (1M context)“ → „Opus 5 (1M)“.
pub fn short_model(name: &str) -> String {
    name.replace(" context)", ")")
}

fn str_field(st: Option<&SlotState>, key: &str) -> String {
    st.and_then(|s| text(s.get(key))).unwrap_or_default().to_string()
}

impl DeckInput<'_> {
    fn window(&self, letter: &str) -> Option<&WindowInfo> {
        self.windows.iter().find(|w| w.letter == letter)
    }

    fn connected(&self, letter: &str) -> bool {
        self.window(letter).is_some_and(|w| w.connected)
    }

    fn binding(&self, letter: &str) -> Option<&str> {
        self.store.bindings.get(letter).map(String::as_str).filter(|b| !b.is_empty())
    }

    /// Blöcke in Zeichenreihenfolge: die gezogene Reihenfolge hängt am REPO-Namen (der
    /// Buchstabe wird beim Schließen neu vergeben), unbekannte hinten in Buchstaben-
    /// Reihenfolge, der Heimat-Block oben. Gezeigt wird, was gebunden oder verbunden ist.
    pub fn shown_windows(&self) -> Vec<String> {
        let key = |w: &str| self.binding(w).unwrap_or(w).to_string();
        let keys: Vec<String> = WINDOWS.iter().map(|w| key(w)).collect();
        let mut rank: HashMap<String, usize> = HashMap::new();
        for (i, k) in apply_order(&self.store.win_order, &keys).into_iter().enumerate() {
            rank.insert(k, i);
        }
        let mut wins: Vec<&str> = WINDOWS.to_vec();
        wins.sort_by_key(|w| rank.get(&key(w)).copied().unwrap_or(usize::MAX));
        pin_first(&wins, self.home)
            .into_iter()
            .filter(|w| self.binding(w).is_some() || self.connected(w))
            .collect()
    }

    /// Die Slots eines Fensters samt Reihenlängen. Liegen die Fenster auf dem
    /// Bildschirm, bestimmt ihre Lage Reihenfolge und Reihen; was
    /// dort fehlt (minimiert), hängt in gezogener Reihenfolge hinten an. Sonst gilt die
    /// gezogene Reihenfolge, ohne feste Reihen.
    pub fn layout(&self, letter: &str) -> (Vec<String>, Vec<u32>) {
        let win = self.window(letter);
        let live = win.map(|w| w.terminals.as_slice()).unwrap_or(&[]);
        let saved = self.store.order.get(letter).map(Vec::as_slice).unwrap_or(&[]);
        let ordered = apply_order(saved, live);
        let mut out: Vec<String> = Vec::new();
        let mut rows: Vec<u32> = Vec::new();
        for row in win.map(|w| w.screen.as_slice()).unwrap_or(&[]) {
            let row: Vec<&String> = row.iter().filter(|s| live.contains(s) && !out.contains(s)).collect();
            if !row.is_empty() {
                rows.push(row.len() as u32);
                out.extend(row.into_iter().cloned());
            }
        }
        out.extend(ordered.into_iter().filter(|s| !out.contains(s)).collect::<Vec<_>>());
        (out, rows)
    }

    /// Die Ordner, in denen der Agent eines Slots arbeitet (`ref_sources::ref_dirs`).
    /// Die App liest für genau diese den Branch.
    pub fn dirs_of(&self, slot: &str) -> Vec<String> {
        slot_dirs(slot, self.states, self.live, self.worktrees)
    }

    /// Ticket und PR der Kachel, jeweils aus der stärksten Quelle (`ref_sources.rs`).
    /// Ein zugewiesenes Ticket gilt sofort, nicht erst, wenn der worktree gemeldet ist.
    pub fn refs(&self, slot: &str, session_id: &str) -> (Option<RefView>, Option<RefView>) {
        let lv = self.live.get(slot);
        let dirs = self.dirs_of(slot);
        let branches: Vec<String> = dirs.iter().filter_map(|d| self.branches.get(d).cloned()).collect();
        let ev = RefEvidence {
            assigned: self.store.tickets.get(slot).or_else(|| self.found_tickets.get(slot)).map(String::as_str),
            branches: &branches,
            dirs: &dirs,
            statusline_pr: lv.and_then(|l| l.get("pr")).and_then(|p| p.get("number")).and_then(serde_json::Value::as_u64),
            title: lv.and_then(|l| text(l.get("session_name"))),
            chat: if TICKET_AUTODETECT_ON_CARD { self.detected_refs.get(session_id) } else { None },
            prefix: self.jira_prefix,
        };
        (pick_ticket(&ev, self.lang), pick_pr(&ev, self.lang))
    }

    /// Eine Kachel samt Status-Schlüssel (`lost` oder der normalisierte Status).
    fn tile(&self, slot: &str, memory: &mut StatusMemory) -> (TileView, String) {
        let st = self.states.get(slot);
        let lv = self.live.get(slot);
        let letter: String = slot.chars().take(1).collect();
        let win = self.window(&letter);
        let fresh = is_fresh(st, self.now, STALE_S);
        let status = normalize_status(&initial_status(st), fresh, &VALID_STATUSES);
        let lost = is_lost(&status, fresh, win.is_some_and(|w| w.connected));
        let key = if lost { "lost".to_string() } else { status };
        let shown = to_status(&key);

        let live_eff = Some(str_field(lv, "effort")).filter(|e| !e.is_empty()).unwrap_or_else(|| str_field(st, "effort"));
        let effort = resolve_effort(&live_eff, self.store.effort.get(slot).map(String::as_str));
        let mode = match self.slot_mode.get(slot).and_then(|i| self.mode_cycle.get(*i)) {
            Some(m) => m.clone(),
            None => str_field(st, "mode"),
        };
        let session_id = str_field(st, "session_id");
        let (ticket, pr) = self.refs(slot, &session_id);
        let title = lv.and_then(|l| l.get("session_name")).and_then(|v| v.as_str()).map(String::from);
        let tile = TileView {
            slot: slot.to_string(),
            status: shown,
            status_label: status_label(self.lang, &key).to_string(),
            model: short_model(&str_field(lv, "model")),
            effort,
            mode,
            title,
            ticket,
            pr,
            cache: tile_cache(st, lv, self.now, self.lang),
            selected: self.active_slot == Some(slot),
            status_changed_at: memory.observe(slot, shown, self.now),
        };
        (tile, key)
    }
}

/// Die ganze Ansicht. `memory` hält die Zeitpunkte der Statuswechsel über die Takte.
pub fn build_deck_view(input: &DeckInput, memory: &mut StatusMemory) -> DeckView {
    let mut keys: Vec<String> = Vec::new();
    let mut blocks = Vec::new();
    for letter in input.shown_windows() {
        let win = input.window(&letter);
        let connected = input.connected(&letter);
        let name = match input.binding(&letter) {
            Some(b) => b.to_string(),
            None => format!("{} {letter}", input.lang.l("Fenster", "Window")),
        };
        let mut tiles = Vec::new();
        let (slots, rows) = if connected { input.layout(&letter) } else { (Vec::new(), Vec::new()) };
        if connected {
            for slot in slots {
                let (tile, key) = input.tile(&slot, memory);
                keys.push(key);
                tiles.push(tile);
            }
        }
        blocks.push(BlockView {
            home: input.home == Some(letter.as_str()),
            name,
            connected,
            tiles,
            rows,
            screen_cols: if connected { win.map_or(0, |w| w.screen_cols) } else { 0 },
            letter,
        });
    }
    // Der Griff leuchtet nur, wenn etwas los ist; idle/unbekannt/keiner = ruhig.
    let urgent = match dominant_status(&keys) {
        k @ ("waiting" | "done" | "lost" | "thinking") => Some(to_status(k)),
        _ => None,
    };
    DeckView { blocks, usage: input.usage.clone(), urgent, lang: input.lang.code().into() }
}
