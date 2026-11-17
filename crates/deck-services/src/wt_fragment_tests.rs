use super::*;

#[test]
fn the_fragment_brings_the_hidden_profile_the_deck_looks_for() {
    let f = fragment(None);
    let p = &f["profiles"][0];
    assert_eq!(p["name"], PROFILE);
    assert_eq!(p["hidden"], true);
    assert_eq!(p["closeOnExit"], "always", "sonst bleibt „[process exited …]“ stehen");
    assert_eq!(p["suppressApplicationTitle"], true);
    assert!(p.get("icon").is_none());
    assert!(p.get("guid").is_none(), "Windows Terminal leitet die guid aus Name und Quelle ab");
    assert!(p["font"].get("face").is_none(), "keine Schrift, die auf fremden Rechnern fehlt");
}

#[test]
fn both_schemes_exist_and_the_profile_uses_them() {
    let f = fragment(Some(r"C:\Users\x\.local\bin\claude.exe"));
    let names: Vec<&str> = f["schemes"].as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap()).collect();
    assert_eq!(names, [SCHEME_LIGHT, SCHEME_DARK]);
    assert_eq!(f["profiles"][0]["colorScheme"]["light"], SCHEME_LIGHT);
    assert_eq!(f["profiles"][0]["colorScheme"]["dark"], SCHEME_DARK);
    assert_eq!(f["profiles"][0]["icon"], r"C:\Users\x\.local\bin\claude.exe");
    for s in f["schemes"].as_array().unwrap() {
        assert_eq!(s.as_object().unwrap().len(), 21, "Name, 4 Grundfarben, 16 ANSI-Farben: {}", s["name"]);
    }
}

#[test]
fn the_light_paper_matches_the_border_the_deck_draws() {
    let f = fragment(None);
    let bg = f["schemes"][0]["background"].as_str().unwrap();
    assert_eq!(u32::from_str_radix(&bg[1..], 16).unwrap(), deck_core::wt::PAPER_LIGHT);
}

#[test]
fn an_own_profile_in_the_terminal_settings_wins_over_the_fragment() {
    let own = r#"{"profiles": {"list": [{"name": "PowerShell"}, {"name": "Agent Deck", "hidden": true}]}}"#;
    let stub = r#"{"profiles": {"list": [{"name": "Agent Deck", "source": "AgentDeck"}]}}"#;
    let none = r#"{"profiles": {"list": [{"name": "PowerShell"}]}}"#;
    let flat = r#"{"profiles": [{"name": "Agent Deck"}]}"#;
    let comment = "// Kommentar\n{\"profiles\": {\"list\": [{\"name\": \"Agent Deck\"}]}}";
    assert!(has_own_profile(own));
    assert!(!has_own_profile(stub), "der Eintrag, den das Terminal für unser Fragment anlegt");
    assert!(!has_own_profile(none));
    assert!(has_own_profile(flat));
    assert!(has_own_profile(comment), "kein JSON: der Text entscheidet");
}

#[test]
fn install_writes_once_and_remove_takes_the_folder_with_it() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("Fragments").join(SOURCE).join(FILE);
    let wt = d.path().join("wt-settings.json");
    let r = install(&path, &wt, None);
    assert!(r.is_ok() && r.changed, "{r:?}");
    let written: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(written, fragment(None));
    assert!(!install(&path, &wt, None).changed, "zweiter Lauf: nichts zu tun");
    let r = remove(&path);
    assert!(r.changed);
    assert!(!path.parent().unwrap().exists());
    assert!(remove(&path).is_ok());
}

#[test]
fn with_an_own_profile_no_fragment_is_written() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join(SOURCE).join(FILE);
    let wt = d.path().join("wt-settings.json");
    std::fs::write(&wt, r#"{"profiles": {"list": [{"name": "Agent Deck"}]}}"#).unwrap();
    let r = install(&path, &wt, None);
    assert!(r.is_ok() && !r.changed);
    assert!(!path.exists());
}

#[test]
fn the_real_fragment_is_locked_in_tests() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(install(&fragment_path(), &d.path().join("none.json"), None).fails(), 1);
    assert_eq!(remove(&fragment_path()).fails(), 1);
}
