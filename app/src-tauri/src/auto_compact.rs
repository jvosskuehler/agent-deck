//! Automatisch `/compact` schicken, kurz bevor der Prompt-Cache eines Agenten abläuft
//! (Python-Fassung: `git show 2856494:deck/ui/auto_compact.py`). Läuft einmal je Takt
//! in `Deck::tick`.
//!
//! Ob es soweit ist und wie oft, rechnet `deck_core::auto_compact`; hier steht nur, was
//! dafür das System gefragt wird (Host verbunden, Fenster vorn, letzte Eingabe) und
//! wie gesendet wird: derselbe Weg wie `/model` aus dem Kontextmenü, also für beide
//! Hosts (VS Code über die Extension, Windows Terminal über die Konsoleneingabe).

use deck_core::auto_compact::{due, enabled};
use deck_core::config::VSCODE_MARKER;
use deck_core::prompt_cache::user_busy;
use deck_core::protocol::Command;
use deck_core::wt::slot_from_title;

use crate::applog;
use crate::deck::Deck;

/// Der Text, den das Deck schickt.
pub const COMPACT: &str = "/compact";

impl Deck {
    /// Die angezeigten Agenten kompaktieren, bei denen es soweit ist. Gibt die Slots
    /// zurück, an die `/compact` ging.
    pub fn auto_compact(&mut self, now: f64) -> Vec<String> {
        if !enabled(&self.store.settings) {
            return Vec::new();
        }
        let slots = self.shown_slots.clone();
        slots.into_iter().filter(|slot| self.compact_one(slot, now)).collect()
    }

    fn compact_one(&mut self, slot: &str, now: f64) -> bool {
        let Some(until) = due(self.states.get(slot), self.live.get(slot), now) else { return false };
        if self.compact_memo.already(slot, until) {
            return false;
        }
        let win = Deck::letter(slot);
        // Remote: die Hooks schreiben drüben, was hier steht, ist nicht dieser Agent.
        if self.hosts.remote(&win).is_some() || !self.hosts.connected(&win) {
            return false;
        }
        if user_busy(self.agent_in_front(slot), self.desktop.input_idle_s()) {
            return false;
        }
        let cmd = Command::Send { slot: slot.to_string(), text: COMPACT.into(), execute: true, submit: false };
        if !self.hosts.send(&win, &cmd) {
            // Nicht gemerkt: im nächsten Takt wieder, solange Restzeit bleibt.
            return false;
        }
        self.compact_memo.mark(slot, until);
        let tokens = self.live.get(slot).and_then(|l| l.get("ctx_tokens")).and_then(|v| v.as_f64()).unwrap_or(0.0);
        applog::note(format!("auto-compact {slot}: Cache läuft in {} s ab, Kontext {} Tokens", (until - now) as i64, tokens as i64));
        true
    }

    /// Liegt das Fenster dieses Agenten vorn? In Windows Terminal hat jeder Agent ein
    /// eigenes Fenster mit dem Slot als Titel. Bei VS Code zählt jedes VS-Code-Fenster:
    /// welches Fenster welchen Buchstaben trägt, sagt der Titel nicht, und lieber einmal
    /// zu vorsichtig als dir in die Eingabe zu tippen.
    fn agent_in_front(&self, slot: &str) -> bool {
        let Some(fg) = self.desktop.foreground() else { return false };
        if self.hosts.is_wt(&Deck::letter(slot)) {
            slot_from_title(Some(&fg.title)).as_deref() == Some(slot)
        } else {
            fg.title.contains(VSCODE_MARKER)
        }
    }
}
