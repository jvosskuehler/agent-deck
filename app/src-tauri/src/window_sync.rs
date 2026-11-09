//! Fenster und Slots pflegen (`deck/ui/windows.py`, Teile von `refresh.py`): Bindungen
//! nachziehen, geschlossene Fenster abräumen, neuen Chat fokussieren, „gesehen“ setzen,
//! neue Agenten in ihren Startmodus treiben.

use std::collections::BTreeSet;
use std::path::Path;

use deck_core::auto_mode::{AutoStep, PendingAuto, decide};
use deck_core::binding::{is_placeholder_ws, repo_from_title, strip_remote_tag};
use deck_core::claude::settings::read_values;
use deck_core::config::{MODE_START, NEW_AGENT_MODE, STALE_WINDOW_S, VSCODE_MARKER, WINDOWS, terminal_home};
use deck_core::paths::{load_json, state_path};
use deck_core::pystr::py_float;
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
        let existing = self.hosts.wt.blocks().into_iter().find(|(_, f)| norm(f) == key).map(|(w, _)| w);
        let win = match existing {
            Some(w) => w,
            None => {
                let free = WINDOWS.iter().find(|w| !self.store.bindings.contains_key(**w) && !self.hosts.wt.has(w));
                let Some(free) = free else { return };
                self.hosts.wt.open_repo(free, &folder);
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

    /// Auto-Verknüpfung: eine gemerkte Extension bekommt ihren Buchstaben; jede
    /// verbundene OHNE Buchstaben das nächste freie Fenster, bevorzugt den Buchstaben,
    /// den ihre Terminals schon tragen (der bleibt so über Neuverbinden stabil).
    pub fn sync_bindings(&mut self) {
        for (w, repo) in self.store.bindings.clone() {
            if !repo.is_empty() && !self.hosts.connected(&w) {
                self.hosts.assign(&repo, &w);
            }
        }
        let mut bound: BTreeSet<String> = self.store.bindings.values().cloned().collect();
        for ws in self.hosts.workspaces() {
            if is_placeholder_ws(Some(&ws)) || bound.contains(&ws) {
                continue;
            }
            let slots = self.hosts.workspace_slots(&ws);
            let pref = slots.first().and_then(|s| s.chars().next()).map(|c| c.to_uppercase().to_string());
            let free = match pref {
                Some(p) if WINDOWS.contains(&p.as_str()) && !self.store.bindings.contains_key(&p) => Some(p),
                _ => WINDOWS.iter().find(|w| !self.store.bindings.contains_key(**w)).map(|w| w.to_string()),
            };
            let Some(free) = free else { break };
            self.store.bindings.insert(free.clone(), ws.clone());
            bound.insert(ws.clone());
            self.store.save_bindings();
            self.hosts.assign(&ws, &free);
        }
    }

    /// Ein gebundenes Fenster abräumen, sobald sein VS-Code-Fenster WIRKLICH zu ist. Ein
    /// Reload lässt das native Fenster (mit dem Repo im Titel) offen: abgeräumt wird nur,
    /// wenn kein offenes VS-Code-Fenster mehr zu diesem Repo existiert, nach kurzer Frist.
    pub fn cleanup_closed_windows(&mut self, now: f64) {
        let pending: Vec<String> =
            WINDOWS.iter().filter(|w| self.store.bindings.contains_key(**w)).map(|w| w.to_string()).collect();
        let pending: Vec<String> = pending.into_iter().filter(|w| !self.hosts.connected(w)).collect();
        if pending.is_empty() {
            self.gone_since.clear();
            return;
        }
        let open: BTreeSet<String> = self
            .desktop
            .list_titles(&[VSCODE_MARKER])
            .iter()
            .map(|t| repo_from_title(t))
            .filter(|r| !r.is_empty() && !is_placeholder_ws(Some(r)) && r != VSCODE_MARKER)
            .map(|r| r.to_lowercase())
            .collect();
        let mut changed = false;
        for w in &pending {
            let repo = self.store.bindings.get(w).cloned().unwrap_or_default();
            // Gebunden ist der Name MIT Remote-Anhängsel, die Titel sind nackt.
            if open.contains(&strip_remote_tag(&repo).to_lowercase()) {
                self.gone_since.remove(w);
                continue;
            }
            match self.gone_since.get(w) {
                None => {
                    self.gone_since.insert(w.clone(), now);
                }
                Some(t0) if now - t0 >= STALE_WINDOW_S => {
                    // Nicht über das Deck geschlossen (Alt+F4, Absturz): dieselbe
                    // Aufräumung wie beim Schließen nachholen, sonst bleiben worktree und
                    // Marker verwaist.
                    for slot in self.slots_for_window(w) {
                        self.cleanup_worktrees(&slot);
                        if self.store.tickets.remove(&slot).is_some() {
                            self.store.save_tickets();
                        }
                        self.clear_found_ticket(&slot);
                        self.forget_slot(&slot);
                    }
                    self.store.bindings.remove(w);
                    self.gone_since.remove(w);
                    if self.active_slot.as_ref().is_some_and(|a| Deck::letter(a) == *w) {
                        self.active_slot = None;
                    }
                    changed = true;
                }
                Some(_) => {}
            }
        }
        self.gone_since.retain(|w, _| pending.contains(w));
        if changed {
            self.store.save_bindings();
        }
    }

    /// Neuen ＋-Chat auswählen und fokussieren, sobald sein Slot da ist. Das Vormerken
    /// für den Startmodus geschieht sofort; das Fokus-Holen nicht, solange ein Dialog
    /// des Decks den Fokus hat.
    pub fn autofocus_new(&mut self, now: f64) {
        let Some(a) = self.await_new.clone() else { return };
        let fresh: Vec<String> = self.hosts.terminals(&a.win).into_iter().filter(|s| !a.before.contains(s)).collect();
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
            let base = load_json(&state_path(&self.paths.state_dir, slot)).and_then(|v| v.get("ts").and_then(py_float)).unwrap_or(0.0);
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

    /// In VS Code bzw. Windows Terminal angeklickte Panes: „ungelesen“ → idle, wie ein
    /// Klick auf die Kachel. Die Kachel des zuletzt vorgeholten Panes wird ausgewählt.
    pub fn mark_seen_read(&mut self, now: f64) {
        let seen = self.hosts.drain_seen();
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
