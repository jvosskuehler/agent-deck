//! Hell/Dunkel im Einstellungsfenster: was gespeichert wird und womit neue Agenten
//! aufgehen. Gegen Attrappen wie alle App-Tests (`tests::rig`).

use serde_json::json;

use crate::settings::Effect;
use crate::tests::rig;

/// Das Farbschema, mit dem der zuletzt gestartete Agent aufging (`--colorScheme`).
fn last_scheme(r: &crate::tests::Rig) -> Option<String> {
    let argv = r.launcher.0.lock().unwrap().last().cloned().expect("kein Start");
    argv.iter().position(|a| a == "--colorScheme").map(|i| argv[i + 1].clone())
}

#[test]
fn a_new_install_follows_windows_and_new_agents_get_no_fixed_scheme() {
    let mut r = rig();
    let v = r.deck.settings_view();
    assert_eq!(v.appearance, "system");
    assert!(!v.sun_place_name.is_empty(), "Ort aus der Zeitzone");
    r.wt_block("C", "repo");
    r.deck.create_agent("C", 100.0);
    assert_eq!(last_scheme(&r), None);
}

#[test]
fn fixed_light_or_dark_opens_new_agents_in_that_scheme() {
    let mut r = rig();
    r.wt_block("C", "repo");
    assert_eq!(r.deck.set_setting("appearance", &json!("dark")), Ok(Effect::Appearance));
    assert_eq!(r.deck.store.settings["appearance"], "dark");
    r.deck.create_agent("C", 100.0);
    assert_eq!(last_scheme(&r).as_deref(), Some(deck_core::wt::SCHEME_DARK));
    r.deck.set_setting("appearance", &json!("light")).unwrap();
    r.deck.create_agent("C", 101.0);
    assert_eq!(last_scheme(&r).as_deref(), Some(deck_core::wt::SCHEME_LIGHT));
    r.deck.set_setting("appearance", &json!("sun")).unwrap();
    r.deck.create_agent("C", 102.0);
    assert_eq!(last_scheme(&r), None, "Sonnenstand: das Deck stellt Windows um, das Profil folgt");
}

#[test]
fn only_known_values_are_saved_and_there_is_no_place_setting() {
    let mut r = rig();
    assert!(r.deck.set_setting("appearance", &json!("lila")).is_err());
    assert!(r.deck.set_setting("appearance", &json!(true)).is_err());
    assert!(!r.deck.store.settings.contains_key("appearance"));
    assert!(r.deck.set_setting("sunPlace", &json!("wien")).is_err(), "Ortswahl gibt es nur mit Erweiterung");
    assert_eq!(r.deck.store.settings.len(), 1, "nur die Version, kein Ort: {:?}", r.deck.store.settings);
}
