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
    assert_eq!(v.sun_place, "hamburg");
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
fn only_known_values_and_places_are_saved() {
    let mut r = rig();
    assert!(r.deck.set_setting("appearance", &json!("lila")).is_err());
    assert!(r.deck.set_setting("appearance", &json!(true)).is_err());
    assert!(!r.deck.store.settings.contains_key("appearance"));
    assert_eq!(r.deck.set_setting("sunPlace", &json!("wien")), Ok(Effect::Appearance));
    assert_eq!(r.deck.settings_view().sun_place, "wien");
    assert!(r.deck.set_setting("sunPlace", &json!("atlantis")).is_err());
    assert_eq!(r.deck.store.settings["sun_place"], "wien");
}

#[test]
fn the_places_are_sorted_by_name_in_the_deck_language() {
    let r = rig();
    let names: Vec<String> = r.deck.settings_view().sun_places.into_iter().map(|p| p.name).collect();
    let at = |n: &str| names.iter().position(|x| x == n).unwrap_or_else(|| panic!("{n} fehlt in {names:?}"));
    assert!(at("Dresden") < at("Düsseldorf") && at("Düsseldorf") < at("Erfurt"), "{names:?}");
    assert!(names.contains(&"Wien".to_string()) && names.contains(&"Zürich".to_string()));
}
