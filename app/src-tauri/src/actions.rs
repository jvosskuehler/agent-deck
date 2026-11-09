//! Was ein Klick auslöst (`deck/ui/actions.py`, Teile von `connect.py`): Fenster nach
//! vorn, Pane fokussieren, Agent anlegen und schließen, Modell/Effort/Modus setzen,
//! Sprung-Taste, Repo in Windows Terminal öffnen.

use std::path::Path;

use deck_core::attention::next_slot;
use deck_core::claude::settings::{MODEL_CHOICES, effort_label, read_values};
use deck_core::config::{MODE_START, VSCODE_MARKER, WINDOWS};
use deck_core::front::focus_target;
use deck_core::protocol::Command;
use deck_core::restart_plan::flags;
use deck_core::slot_state::{clear_state, read_all, write_state};
use deck_core::status_model::mode_steps;
use deck_win::Hwnd;
use serde_json::Value;

use crate::deck::{AwaitNew, Deck};

impl Deck {
    /// Das Fenster eines Blocks nach vorn: VS Code per Titelsuche, ein WT-Block über
    /// seinen Host (der kennt die Fenster seiner Agenten).
    pub fn raise_window(&mut self, win: &str) -> bool {
        let Some(repo) = self.store.bindings.get(win).cloned() else { return false };
        if self.hosts.is_wt(win) {
            return self.hosts.wt.raise_window(win);
        }
        let Some(hwnd) = self.desktop.find_window(&[&repo, VSCODE_MARKER]) else { return false };
        self.desktop.focus(hwnd).unwrap_or(false)
    }

    /// Klick auf den Repo-Namen: das Fenster nach vorn, bewusst OHNE Pane (der Kopf
    /// meint das Fenster, nicht einen Agenten). Kein Fenster → nichts.
    pub fn show_window(&mut self, win: &str) {
        self.raise_window(win);
    }

    /// Klick auf eine Kachel: Auswahl setzen, eine ungelesene Antwort als gelesen
    /// markieren, Fenster nach vorn und das Pane fokussieren.
    pub fn focus_slot(&mut self, slot: &str) {
        let win = Deck::letter(slot);
        if !self.store.bindings.contains_key(&win) {
            return;
        }
        self.active_slot = Some(slot.to_string());
        self.mark_read(slot);
        if !self.hosts.is_wt(&win) {
            self.raise_window(&win);
        }
        // In Windows Terminal holt focusPane das Fenster UND das Pane.
        self.hosts.send(&win, &Command::FocusPane { slot: slot.to_string() });
    }

    /// „ungelesen“ → idle, in der Datei (der Merker muss über den Takt hinaus halten).
    pub fn mark_read(&mut self, slot: &str) {
        let st = read_all(&self.paths.state_dir);
        if st.get(slot).and_then(|s| s.get("status")).and_then(|v| v.as_str()) == Some("done") {
            let _ = write_state(&self.paths.state_dir, slot, "idle", crate::now());
        }
    }

    /// ＋-Kachel: ein weiteres Claude-Terminal mit Modell und Effort aus den Einstellungen
    /// als `--model` und `--effort`. Startschalter schlagen alles: `--model` das zuletzt
    /// per /model gemerkte Modell, `--effort` den Wert, den `/effort` je Modell in
    /// settings.json merkt. Das globale `effortLevel` zählt seit Claude Code 2.1.3xx für
    /// aktuelle Modelle nicht mehr (nur noch als Altwert); ohne den Schalter startete am
    /// 2026-11-09 jeder neue Agent mit der Modell-Vorgabe medium.
    pub fn create_agent(&mut self, win: &str, now: f64) {
        if !self.hosts.connected(win) {
            return;
        }
        let model = self.setting_str("model").unwrap_or_else(|| MODEL_CHOICES[0].1.to_string());
        let args = flags(None, Some(&model), self.chosen_effort(), None);
        let before = self.hosts.terminals(win);
        let cmd = Command::CreateAgent { model: Some(model), args: Some(args), cwd: None, slot: None };
        if self.hosts.send(win, &cmd) {
            self.await_new = Some(AwaitNew { win: win.to_string(), before, ts: now });
        }
    }

    /// Sprung-Taste: das Pane, das am längsten wartet; steht man schon bei einem
    /// Wartenden, der nächste (reihum). Wartet niemand, passiert nichts.
    pub fn jump_to_waiting(&mut self) {
        let states = read_all(&self.paths.state_dir);
        if let Some(slot) = next_slot(&states, &self.shown_slots, self.active_slot.as_deref()) {
            self.focus_slot(&slot);
        }
    }

    /// Taste „alle nach vorn“ (`config::FRONT_HOTKEYS`): jedes Agentenfenster in Windows
    /// Terminal als Raster auf den Monitor unter `cursor`, Fokus auf den, der am längsten
    /// wartet. Steht man schon bei einem Agenten, geht es zurück in das Fenster, aus dem
    /// man kam (`deck_core::front`). VS-Code-Blöcke bleiben außen vor: ein maximiertes
    /// VS Code deckte die Terminals zu; für sie gibt es den Klick auf den Repo-Namen.
    pub fn bring_all_front(&mut self, cursor: Option<(i32, i32)>) {
        let fg = self.desktop.foreground().filter(|w| w.pid != std::process::id());
        let on_agent = fg.is_some() && self.hosts.wt.slot_in_front().is_some();
        let desktop = self.desktop.clone();
        let back = self.front_toggle.press(fg.map(|w| w.hwnd.0), on_agent, |h| desktop.is_alive(Hwnd(h)));
        if let Some(h) = back {
            let _ = self.desktop.focus(Hwnd(h));
            return;
        }
        let slots: Vec<String> =
            self.shown_slots.iter().filter(|s| self.hosts.is_wt(&Deck::letter(s))).cloned().collect();
        let states = read_all(&self.paths.state_dir);
        let target = focus_target(&states, &slots, self.active_slot.as_deref());
        self.hosts.wt.front_all(&slots, target, cursor);
    }

    /// Einen Agenten schließen; hing ein worktree daran, wird er mit abgeräumt.
    pub fn close_agent(&mut self, slot: &str) {
        let win = Deck::letter(slot);
        if !self.hosts.connected(&win) {
            return;
        }
        if self.hosts.send(&win, &Command::CloseAgent { slot: slot.to_string() }) {
            self.cleanup_worktrees(slot);
            if self.store.tickets.remove(slot).is_some() {
                self.store.save_tickets();
            }
            self.clear_found_ticket(slot);
            self.forget_slot(slot);
            if self.active_slot.as_deref() == Some(slot) {
                self.active_slot = None;
            }
        }
    }

    /// Die Spuren eines geschlossenen Agenten tilgen: die Extension recycelt Slot-Namen,
    /// ein späterer Agent darf Modus, Vormerkung und Status nicht erben.
    pub fn forget_slot(&mut self, slot: &str) {
        self.slot_mode.remove(slot);
        self.mode_ts.remove(slot);
        self.pending_auto.remove(slot);
        clear_state(&self.paths.state_dir, slot);
    }

    /// Permission-Modus eines Slots per Shift+Tab setzen. `current` = angenommener
    /// Ist-Index; `None` → gemerkter Modus bzw. Startmodus. `false` nur, wenn das Senden
    /// scheiterte (dann nicht gemerkt).
    pub fn set_slot_mode(&mut self, slot: &str, target: &str, cycle: &[String], current: Option<usize>) -> bool {
        let remembered = current.or_else(|| self.slot_mode.get(slot).copied());
        let Some((steps, tgt)) = mode_steps(remembered, target, cycle, MODE_START) else { return false };
        if steps > 0 {
            let cmd = Command::Key { slot: slot.to_string(), key: "shift-tab".into(), repeat: steps as u32 };
            if !self.hosts.send(&Deck::letter(slot), &cmd) {
                return false;
            }
        }
        self.slot_mode.insert(slot.to_string(), tgt);
        true
    }

    /// Kontextmenü „Model“: `/model <wert>` an den Agenten.
    pub fn set_slot_model(&mut self, slot: &str, value: &str) {
        let win = Deck::letter(slot);
        if !self.hosts.connected(&win) {
            return;
        }
        self.active_slot = Some(slot.to_string());
        self.hosts.send(&win, &Command::Send { slot: slot.into(), text: format!("/model {value}"), execute: true, submit: false });
    }

    /// Kontextmenü „Effort“: `/effort <level>` und den Wert merken (die statusLine meldet
    /// für xhigh UND ultracode nur `xhigh`).
    pub fn set_slot_effort(&mut self, slot: &str, label: &str) {
        let win = Deck::letter(slot);
        if !self.hosts.connected(&win) {
            return;
        }
        let level = label.to_lowercase();
        self.active_slot = Some(slot.to_string());
        self.hosts.send(&win, &Command::Send { slot: slot.into(), text: format!("/effort {level}"), execute: true, submit: false });
        self.store.effort.insert(slot.to_string(), level);
        self.store.save_effort();
    }

    /// Kontextmenü „Mode“.
    pub fn menu_set_mode(&mut self, slot: &str, target: &str) {
        if !self.hosts.connected(&Deck::letter(slot)) {
            return;
        }
        self.active_slot = Some(slot.to_string());
        let cycle = self.mode_cycle();
        self.set_slot_mode(slot, target, &cycle, None);
    }

    /// Ein Repo als Windows-Terminal-Block öffnen und gleich den ersten Agenten darin
    /// starten. Steht es schon als WT-Block da, kommt nur ein Agent dazu. Hängt es an
    /// einem VS-Code-Fenster OHNE Agenten, wird diese Bindung gelöst (sonst stünde der
    /// Name zweimal da). Rückgabe: Text für die Rückmeldung.
    pub fn open_repo_in_terminal(&mut self, folder: &str, now: f64) -> String {
        let folder = folder.trim_end_matches(['\\', '/']).to_string();
        let path = Path::new(&folder);
        let repo = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let l = self.lang;
        if repo.is_empty() || !path.is_dir() {
            return l.l("Kein Ordner gewählt.", "No folder chosen.").into();
        }
        for (w, r) in self.store.bindings.clone() {
            if r.to_lowercase() != repo.to_lowercase() {
                continue;
            }
            if self.hosts.is_wt(&w) {
                self.create_agent(&w, now);
                return if l.is_english() {
                    format!("“{repo}”: another agent in the terminal.")
                } else {
                    format!("„{repo}“: weiterer Agent im Terminal.")
                };
            }
            if self.hosts.terminals(&w).is_empty() {
                self.forget_window(&w);
            }
        }
        let Some(free) = WINDOWS.iter().find(|w| !self.store.bindings.contains_key(**w)).map(|w| w.to_string()) else {
            return l.l("Kein Buchstabe mehr frei.", "No letter left.").into();
        };
        self.hosts.wt.open_repo(&free, &folder);
        self.store.bindings.insert(free.clone(), repo.clone());
        self.store.save_bindings();
        self.create_agent(&free, now);
        if l.is_english() {
            format!("“{repo}” opens in Windows Terminal.")
        } else {
            format!("„{repo}“ öffnet sich in Windows Terminal.")
        }
    }

    /// Die Bindung eines Buchstabens vergessen (auch eine verbundene, bindungslose).
    pub fn forget_window(&mut self, win: &str) {
        // Ein laufendes Verbinden dieses Blocks mit abbrechen.
        if self.binding(win) {
            self.cancel_bind();
        }
        self.hosts.forget(win);
        if self.store.bindings.remove(win).is_some() {
            self.store.save_bindings();
        }
    }

    /// Neue Kartenreihenfolge eines Blocks (nur bei echter Änderung speichern).
    pub fn reorder_tiles(&mut self, win: &str, slots: Vec<String>) {
        if self.store.order.get(win) != Some(&slots) {
            self.store.order.insert(win.to_string(), slots.clone());
            self.store.save_order();
        }
        // Windows Terminal: die Karten liegen wie die Fenster, also zieht der Bildschirm
        // nach. Sonst holte die gemessene Lage die Karte im nächsten Takt zurück.
        if self.hosts.is_wt(win) {
            self.hosts.wt.retile_in(win, &slots);
        }
    }

    /// Neue Blockreihenfolge. Gemerkt werden REPO-Namen (der Buchstabe wird beim
    /// Schließen neu vergeben), eingewebt statt vorangestellt: geschlossene Repos
    /// behalten ihren Platz.
    pub fn reorder_blocks(&mut self, letters: &[String]) {
        let keys: Vec<String> = letters.iter().map(|w| self.store.bindings.get(w).cloned().unwrap_or_else(|| w.clone())).collect();
        let merged = deck_core::ordering::merge_visible(&self.store.win_order, &keys);
        if merged != self.store.win_order {
            self.store.win_order = merged;
            self.store.save_win_order();
        }
    }

    /// Der im Deck gewählte Effort als Label (`high`, `Ultracode` …). `None`, solange in
    /// settings.json keiner steht: dann startet der Agent ohne `--effort`, statt den
    /// Rückfall der Anzeige (xhigh) aufgezwungen zu bekommen.
    fn chosen_effort(&self) -> Option<&'static str> {
        let cur = read_values(&self.paths.claude_settings);
        let set = cur.effort.as_ref().and_then(Value::as_str);
        (set.is_some() || cur.ultracode).then(|| effort_label(set, cur.ultracode))
    }

    /// Ein Wert aus `deck_settings.json` als Text (leer = fehlt).
    pub fn setting_str(&self, key: &str) -> Option<String> {
        self.store.settings.get(key).and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()).map(String::from)
    }
}
