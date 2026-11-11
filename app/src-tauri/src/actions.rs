//! Was ein Klick auslöst (`deck/ui/actions.py`, Teile von `connect.py`): Fenster nach
//! vorn, Pane fokussieren, Agent anlegen und schließen, Modell/Effort/Modus setzen,
//! Sprung-Taste.

use deck_core::agent_flags::flags;
use deck_core::attention::next_slot;
use deck_core::claude::settings::{MODEL_CHOICES, effort_label, read_values};
use deck_core::config::MODE_START;
use deck_core::slot_state::{clear_state, read_all, write_state};
use deck_core::status_model::mode_steps;
use deck_win::Hwnd;
use serde_json::Value;

use crate::deck::{AwaitNew, Deck};

impl Deck {
    /// Klick auf den Repo-Namen: die Fenster des Blocks nach vorn, bewusst ohne einen
    /// Agenten auszuwählen (der Kopf meint den Block). Kein Fenster → nichts.
    pub fn show_window(&mut self, win: &str) {
        if self.store.bindings.contains_key(win) {
            self.wt.raise_window(win);
        }
    }

    /// Klick auf eine Kachel: Auswahl setzen, eine ungelesene Antwort als gelesen
    /// markieren, das Fenster des Agenten nach vorn.
    pub fn focus_slot(&mut self, slot: &str) {
        let win = Deck::letter(slot);
        if !self.store.bindings.contains_key(&win) {
            return;
        }
        self.active_slot = Some(slot.to_string());
        self.mark_read(slot);
        self.wt.focus_pane(slot);
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
        if !self.wt.has(win) {
            return;
        }
        let model = self.setting_str("model").unwrap_or_else(|| MODEL_CHOICES[0].1.to_string());
        let args = flags(None, Some(&model), self.chosen_effort(), None);
        let before = self.wt.terminals(win);
        if self.wt.create_agent(win, Some(&model), Some(&args)) {
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
    /// Terminal als Raster auf den Monitor unter `cursor`, über alle anderen Fenster,
    /// aber ohne Tastatur: die bleibt, wo sie war. Liegen sie schon alle offen und frei
    /// (kein fremdes Fenster verdeckt eines, egal wer die Tastatur hat), verschwinden sie
    /// wieder (minimiert), und es geht zurück in das Fenster, aus dem man kam
    /// (`deck_core::front`). Verdeckt sie etwas, etwa ein Browser, den ein Agent geöffnet
    /// hat, holt die Taste sie erst nach vorn. So lassen sie sich ein- und ausblenden.
    pub fn bring_all_front(&mut self, cursor: Option<(i32, i32)>) {
        let slots = self.shown_slots.clone();
        let fg = self.desktop.foreground().filter(|w| w.pid != std::process::id());
        let on_agent = fg.is_some() && self.wt.slot_in_front().is_some();
        let hide = self.wt.all_open(&slots) && self.wt.coverer(&slots).is_none();
        let desktop = self.desktop.clone();
        // Ein Agent ist kein Ausgangspunkt: er wird beim Verstecken selbst minimiert.
        let origin = fg.filter(|_| !on_agent).map(|w| w.hwnd.0);
        let back = self.front_toggle.press(origin, hide, |h| desktop.is_alive(Hwnd(h)));
        if hide {
            self.wt.hide_all(&slots, back.map(Hwnd));
            return;
        }
        self.wt.front_all(&slots, cursor);
    }

    /// Einen Agenten schließen; hing ein worktree daran, wird er mit abgeräumt.
    pub fn close_agent(&mut self, slot: &str) {
        if !self.wt.has(&Deck::letter(slot)) {
            return;
        }
        if self.wt.close_agent(slot) {
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

    /// Die Spuren eines geschlossenen Agenten tilgen: Slot-Namen werden recycelt,
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
        if steps > 0 && !self.wt.key(slot, "shift-tab", steps as u32) {
            return false;
        }
        self.slot_mode.insert(slot.to_string(), tgt);
        true
    }

    /// Kontextmenü „Model“: `/model <wert>` an den Agenten.
    pub fn set_slot_model(&mut self, slot: &str, value: &str) {
        if !self.wt.has(&Deck::letter(slot)) {
            return;
        }
        self.active_slot = Some(slot.to_string());
        self.wt.send_text(slot, &format!("/model {value}"), false);
    }

    /// Kontextmenü „Effort“: `/effort <level>` und den Wert merken (die statusLine meldet
    /// für xhigh UND ultracode nur `xhigh`).
    pub fn set_slot_effort(&mut self, slot: &str, label: &str) {
        if !self.wt.has(&Deck::letter(slot)) {
            return;
        }
        let level = label.to_lowercase();
        self.active_slot = Some(slot.to_string());
        self.wt.send_text(slot, &format!("/effort {level}"), false);
        self.store.effort.insert(slot.to_string(), level);
        self.store.save_effort();
    }

    /// Kontextmenü „Mode“.
    pub fn menu_set_mode(&mut self, slot: &str, target: &str) {
        if !self.wt.has(&Deck::letter(slot)) {
            return;
        }
        self.active_slot = Some(slot.to_string());
        let cycle = self.mode_cycle();
        self.set_slot_mode(slot, target, &cycle, None);
    }

    /// Einen Block vergessen: Bindung und Windows-Terminal-Eintrag. Laufende Agenten
    /// bleiben offen.
    pub fn forget_window(&mut self, win: &str) {
        self.wt.forget(win);
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
        // Die Karten liegen wie die Fenster, also zieht der Bildschirm nach. Sonst holte
        // die gemessene Lage die Karte im nächsten Takt zurück.
        self.wt.retile_in(win, &slots);
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
