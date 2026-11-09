//! Blöcke und Slots pflegen (`deck/ui/windows.py`, Teile von `refresh.py`): Heimat-Block
//! anlegen, verwaiste Bindungen abräumen, neuen Chat fokussieren, „gesehen“ setzen, neue
//! Agenten in ihren Startmodus treiben.

use std::collections::BTreeSet;
use std::path::Path;

use deck_core::auto_mode::{AutoStep, PendingAuto, decide};
use deck_core::claude::settings::read_values;
use deck_core::config::{MODE_START, NEW_AGENT_MODE, WINDOWS, terminal_home};
use deck_core::paths::{load_json, state_path};
use deck_core::field::number;
use deck_core::slot_state::{read_all, write_state};

use crate::deck::Deck;

impl Deck {
    /// Den Heimat-Block sicherstellen: ein Windows-Terminal-Block über dem Ordner aller
    /// Repos, beim Start schon da, noch ohne Agenten. Gibt es ihn (wt_hosts.json), behält
    /// er seinen Buchstaben; sonst bekommt er den ersten freien. Läuft einmal beim Start.
    pub fn ensure_home_terminal(&mut self) {
        self.home_win = None;
        let folder = self.store.settings.get("terminal_home").and_then(|v| v.as_str()).filter(|s| !s.trim().is_empty()).map(String::from);
        let folder = folder.unwrap_or_else(|| terminal_home().to_string_lossy().into_owned());
        let path = Path::new(&folder);
        if !path.is_dir() {
            return;
        }
        let key = norm(&folder);
        let existing = self.wt.blocks().into_iter().find(|(_, f)| norm(f) == key).map(|(w, _)| w);
        let win = match existing {
            Some(w) => w,
            None => {
                let free = WINDOWS.iter().find(|w| !self.store.bindings.contains_key(**w) && !self.wt.has(w));
                let Some(free) = free else { return };
                self.wt.open_repo(free, &folder);
                free.to_string()
            }
        };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if self.store.bindings.get(&win) != Some(&name) {
            self.store.bindings.insert(win.clone(), name);
            self.store.save_bindings();
        }
        self.home_win = Some(win);
    }

    /// Eine Bindung ohne Windows-Terminal-Block abräumen (etwa die eines früheren
    /// VS-Code-Fensters), samt der Spuren ihrer Agenten: dieselbe Aufräumung wie beim
    /// Schließen, sonst blieben worktree und Marker verwaist.
    pub fn prune_stale_bindings(&mut self) {
        let stale: Vec<String> = self.store.bindings.keys().filter(|w| !self.wt.has(w)).cloned().collect();
        if stale.is_empty() {
            return;
        }
        for w in &stale {
            for slot in self.slots_for_window(w) {
                self.cleanup_worktrees(&slot);
                if self.store.tickets.remove(&slot).is_some() {
                    self.store.save_tickets();
                }
                self.clear_found_ticket(&slot);
                self.forget_slot(&slot);
            }
            self.store.bindings.remove(w);
            if self.active_slot.as_ref().is_some_and(|a| Deck::letter(a) == *w) {
                self.active_slot = None;
            }
        }
        self.store.save_bindings();
    }

    /// Neuen ＋-Chat auswählen und fokussieren, sobald sein Slot da ist. Das Vormerken
    /// für den Startmodus geschieht sofort; das Fokus-Holen nicht, solange ein Dialog
    /// des Decks den Fokus hat.
    pub fn autofocus_new(&mut self, now: f64) {
        let Some(a) = self.await_new.clone() else { return };
        let fresh: Vec<String> = self.wt.terminals(&a.win).into_iter().filter(|s| !a.before.contains(s)).collect();
        if let Some(last) = fresh.last().cloned() {
            self.register_pending_auto(&fresh, now);
            self.await_new = None;
            if !self.modal {
                self.focus_slot(&last);
            }
        } else if !self.modal && now - a.ts > 8.0 {
            self.await_new = None;
        }
    }

    /// Frisch angelegte Slots für den Startmodus vormerken. Gibt settings.json einen
    /// Start-Permission-Modus vor, startet claude schon darin: dann nicht weiterschalten.
    pub fn register_pending_auto(&mut self, slots: &[String], now: f64) {
        if NEW_AGENT_MODE.is_empty() || read_values(&self.paths.claude_settings).mode.is_some() {
            return;
        }
        for slot in slots {
            let base = load_json(&state_path(&self.paths.state_dir, slot)).and_then(|v| number(v.get("ts"))).unwrap_or(0.0);
            self.pending_auto.insert(slot.clone(), PendingAuto::new(base, now));
        }
    }

    /// Vorgemerkte Agenten in den Startmodus treiben (Regeln in `deck_core::auto_mode`).
    pub fn apply_pending_auto(&mut self, now: f64, cycle: &[String]) {
        if self.pending_auto.is_empty() {
            return;
        }
        let Some(target) = cycle.iter().position(|m| m == NEW_AGENT_MODE) else {
            self.pending_auto.clear();
            return;
        };
        let start = cycle.iter().position(|m| m == MODE_START).unwrap_or(0);
        let slots: Vec<String> = self.pending_auto.keys().cloned().collect();
        for slot in slots {
            let Some(mut p) = self.pending_auto.remove(&slot) else { continue };
            match decide(&mut p, self.states.get(&slot), now, cycle, target, start) {
                AutoStep::Forget => continue,
                AutoStep::Wait => {}
                AutoStep::Drive { from } => {
                    if self.set_slot_mode(&slot, NEW_AGENT_MODE, cycle, Some(from)) {
                        p.sent(now);
                    }
                }
            }
            self.pending_auto.insert(slot, p);
        }
    }

    /// In Windows Terminal vorgeholte Agentenfenster: „ungelesen“ → idle, wie ein Klick
    /// auf die Kachel. Die Kachel des zuletzt vorgeholten Agenten wird ausgewählt.
    pub fn mark_seen_read(&mut self, now: f64) {
        let seen = self.wt.drain_seen();
        if seen.is_empty() {
            return;
        }
        let states = read_all(&self.paths.state_dir);
        for slot in &seen {
            if states.get(slot).and_then(|s| s.get("status")).and_then(|v| v.as_str()) == Some("done") {
                let _ = write_state(&self.paths.state_dir, slot, "idle", now);
            }
        }
        if let Some(last) = seen.iter().rev().find(|s| self.shown_slots.contains(s)) {
            self.active_slot = Some(last.clone());
        }
    }

    /// Alle Slots dieses Fensters, an denen ein worktree hängen kann: aus Ticket,
    /// gemeldeter ID und worktree-Marker.
    pub fn slots_for_window(&self, win: &str) -> Vec<String> {
        let markers = deck_core::slot_state::read_found_worktrees(&self.paths.state_dir);
        let mut all: BTreeSet<String> = self.store.tickets.keys().cloned().collect();
        all.extend(self.found.keys().cloned());
        all.extend(markers.into_keys());
        all.into_iter().filter(|s| Deck::letter(s) == win).collect()
    }
}

/// Ordnerpfad zum Vergleichen: Trenner vereinheitlicht, ohne Schluss-Trenner, klein.
fn norm(p: &str) -> String {
    p.replace('/', "\\").trim_end_matches('\\').to_lowercase()
}
