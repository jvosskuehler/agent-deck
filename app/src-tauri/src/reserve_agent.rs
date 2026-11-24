//! Reserve-Agent aus Sicht des Decks: welche Reserve es geben soll. Das Wie steht in
//! `wt_reserve.rs`, die Regeln in `deck_core::reserve`.
//!
//! Ob es eine gibt, entscheidet allein eine Erweiterung (`DeckExtension::reserve_agent`).
//! Die Team-Ausgabe hat keine und damit nie eine Reserve, auch keinen Schalter dafür; eine
//! Reserve aus einer anderen Ausgabe (`wt_reserve.json` nach einem Wechsel) räumt sie ab.
//!
//! Gewärmt wird für den Block, in dem die Taste „Neuer Agent“ am ehesten landet: den
//! Heimat-Block, sonst den ersten. Modell und Effort wie beim ＋ ([`Deck::create_agent`]);
//! ändert sich eines davon, passt die Reserve nicht mehr und wird ausgetauscht.

use deck_core::agent_flags::flags;
use deck_core::reserve::Recipe;

use crate::deck::Deck;

/// So oft (s) schaut das Deck nach der Reserve. Liest settings.json (Effort), darum nicht
/// in jedem 400-ms-Takt.
const EVERY_S: f64 = 2.0;

impl Deck {
    /// Ein Takt der Reserve-Pflege (gedrosselt auf [`EVERY_S`]).
    pub fn keep_reserve(&mut self, now: f64) {
        if (0.0..EVERY_S).contains(&(now - self.reserve_ts)) {
            return;
        }
        self.reserve_ts = now;
        let want = self.wanted_reserve();
        self.wt.keep_reserve(want, now);
    }

    /// Die Reserve, die es geben soll; `None` = keine Erweiterung will eine, oder kein Block.
    fn wanted_reserve(&self) -> Option<Recipe> {
        if !self.ext.reserve_agent(&self.store.settings) {
            return None;
        }
        let win = self.home_win.clone().filter(|w| self.wt.has(w)).or_else(|| self.wt.blocks().into_iter().map(|(w, _)| w).next())?;
        let model = self.chosen_model();
        let args = flags(None, model.as_deref(), self.chosen_effort(), None);
        self.wt.recipe(&win, model.as_deref(), Some(&args))
    }
}
