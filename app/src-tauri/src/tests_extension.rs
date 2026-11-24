//! Erweiterungen am Deck (`extension.rs`): was der Kern ohne sie tut und wo er sie fragt.
//! Gegen Attrappen wie alle App-Tests (`tests::rig`).

use std::sync::Mutex;

use serde_json::{Value, json};

use crate::deck::Deck;
use crate::extension::{DeckExtension, Extensions, Settings};
use crate::settings::Effect;
use crate::tests::{Rig, rig};
use deck_core::view::SettingsSection;

/// Eine Erweiterung für Tests: Projektkürzel, ein Schalter `extraFlag`, ein Abschnitt, ein
/// Kürzel, und sie merkt sich, welche Slots frei wurden.
#[derive(Default)]
struct Probe {
    prefix: Option<&'static str>,
    released: Mutex<Vec<(String, Option<String>)>>,
    ticks: Mutex<u32>,
}

impl DeckExtension for Probe {
    fn name(&self) -> &'static str {
        "probe"
    }
    fn ticket_prefix(&self, _: &Settings) -> Option<String> {
        self.prefix.map(String::from)
    }
    fn on_tick(&self, _: &mut Deck, _: &deck_core::view::DeckView, _: f64) {
        *self.ticks.lock().unwrap() += 1;
    }
    fn settings_section(&self, deck: &Deck) -> Option<SettingsSection> {
        Some(SettingsSection { id: "probe".into(), values: json!({"extraFlag": deck.store.settings.get("extra_flag").cloned().unwrap_or(Value::Bool(false))}) })
    }
    fn set_setting(&self, deck: &mut Deck, key: &str, value: &Value) -> Option<Result<Effect, String>> {
        (key == "extraFlag").then(|| {
            let on = value.as_bool().ok_or("extraFlag: true/false erwartet")?;
            deck.put("extra_flag", on.into());
            Ok(Effect::None)
        })
    }
    fn shortcuts(&self, _: &Settings, _: deck_core::i18n::Lang) -> Vec<deck_core::shortcuts::ShortcutRow> {
        vec![deck_core::shortcuts::ShortcutRow { keys: "Strg+Alt+X".into(), action: "Probe".into() }]
    }
    fn on_slot_released(&self, _: &Deck, slot: &str, ticket: Option<&str>) {
        self.released.lock().unwrap().push((slot.into(), ticket.map(String::from)));
    }
}

/// Das Deck des Rigs mit einer Erweiterung, die nur ein Projektkürzel vorgibt.
pub(crate) fn with_prefix(r: &mut Rig, prefix: &'static str) {
    r.deck.ext = Extensions::new(vec![Box::new(Probe { prefix: Some(prefix), ..Probe::default() })]);
}

#[test]
fn without_extensions_a_bare_number_stays_a_number_and_extra_keys_are_refused() {
    let mut r = rig();
    r.wt_block("A", "repo");
    r.wt_agent("A1", 81);
    r.deck.assign_ticket("A1", "2701");
    let view = r.deck.tick(100.0);
    assert_eq!(view.blocks[0].tiles[0].ticket.as_ref().map(|t| t.text.as_str()), Some("2701"), "kein ausgedachtes Projekt");
    assert!(r.deck.set_setting("extraFlag", &json!(true)).is_err());
    let v = r.deck.settings_view();
    assert!(v.extras.is_empty());
    assert_eq!(v.shortcuts.last().map(|s| s.keys.as_str()), Some("Alt+V"));
}

#[test]
fn an_extension_gets_its_keys_its_section_and_its_shortcut_before_alt_v() {
    let mut r = rig();
    r.deck.ext = Extensions::new(vec![Box::new(Probe::default())]);
    assert_eq!(r.deck.set_setting("extraFlag", &json!(true)), Ok(Effect::None));
    assert!(r.deck.set_setting("extraFlag", &json!("ja")).is_err());
    assert!(r.deck.set_setting("nonsense", &json!(1)).is_err(), "was keiner kennt, bleibt ein Fehler");
    let v = r.deck.settings_view();
    assert_eq!(v.extras, [SettingsSection { id: "probe".into(), values: json!({"extraFlag": true}) }]);
    let keys: Vec<&str> = v.shortcuts.iter().map(|s| s.keys.as_str()).collect();
    assert_eq!(&keys[keys.len() - 2..], ["Strg+Alt+X", "Alt+V"]);
}

#[test]
fn an_extension_runs_each_tick_and_hears_when_a_slot_is_released() {
    let mut r = rig();
    let probe = std::sync::Arc::new(Probe::default());
    struct Shared(std::sync::Arc<Probe>);
    impl DeckExtension for Shared {
        fn name(&self) -> &'static str {
            "shared"
        }
        fn on_tick(&self, d: &mut Deck, v: &deck_core::view::DeckView, now: f64) {
            self.0.on_tick(d, v, now);
        }
        fn on_slot_released(&self, d: &Deck, slot: &str, ticket: Option<&str>) {
            self.0.on_slot_released(d, slot, ticket);
        }
    }
    r.deck.ext = Extensions::new(vec![Box::new(Shared(probe.clone()))]);
    r.wt_block("A", "repo");
    r.wt_agent("A1", 81);
    r.deck.tick(100.0);
    r.deck.tick(100.4);
    assert_eq!(*probe.ticks.lock().unwrap(), 2);
    r.deck.assign_ticket("A1", "ABC-1");
    r.deck.assign_ticket("A1", "ABC-2");
    assert_eq!(probe.released.lock().unwrap().last().cloned(), Some(("A1".into(), Some("ABC-1".into()))), "das alte Ticket, solange es noch bekannt ist");
}
