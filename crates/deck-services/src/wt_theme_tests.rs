use super::*;

const PLAIN: &str = "{\r\n    \"$schema\": \"x\",\r\n    \"defaultProfile\": \"{1}\",\r\n    \"profiles\": {}\r\n}\r\n";

#[test]
fn without_a_theme_the_terminal_follows_windows_afterwards() {
    let out = with_system_theme(PLAIN).expect("eingetragen");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["theme"], "system");
    assert_eq!(v["defaultProfile"], "{1}", "der Rest bleibt");
    assert!(out.contains("\r\n    \"theme\": \"system\",\r\n"), "Zeilenende der Datei bleibt");
}

#[test]
fn an_own_theme_choice_is_never_touched() {
    let own = "{\n  \"theme\": \"light\",\n  \"profiles\": {}\n}\n";
    assert_eq!(with_system_theme(own), None);
}

#[test]
fn comments_in_the_file_do_not_stop_it() {
    let commented = "// Kommentar des Nutzers\n{\n    // noch einer\n    \"profiles\": {}\n}\n";
    let out = with_system_theme(commented).expect("eingetragen");
    assert!(out.starts_with("// Kommentar des Nutzers\n{\n    \"theme\": \"system\",\n"));
}

#[test]
fn an_unreadable_file_stays_as_it_is() {
    assert_eq!(with_system_theme("{ kaputt"), None);
}

#[test]
fn an_empty_object_stays_valid_json() {
    let out = with_system_theme("{}").unwrap();
    assert_eq!(serde_json::from_str::<Value>(&out).unwrap()["theme"], "system");
}

#[test]
fn removing_gives_back_the_exact_text() {
    let added = with_system_theme(PLAIN).unwrap();
    assert_eq!(without_system_theme(&added).as_deref(), Some(PLAIN));
}

#[test]
fn a_later_own_choice_survives_the_uninstall() {
    let changed = with_system_theme(PLAIN).unwrap().replace("\"theme\": \"system\"", "\"theme\": \"dark\"");
    assert_eq!(without_system_theme(&changed), None);
}

#[test]
fn install_and_remove_round_trip_on_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(&path, PLAIN).unwrap();
    let mut m = Installed::default();
    assert!(install(&path, &mut m).is_ok());
    assert!(m.wt_theme_added);
    assert!(std::fs::read_to_string(&path).unwrap().contains("\"theme\": \"system\""));
    let mut again = m.clone();
    install(&path, &mut again);
    assert_eq!(std::fs::read_to_string(&path).unwrap().matches("\"theme\"").count(), 1, "ein zweiter Lauf fügt nichts hinzu");
    assert!(remove(&path, &mut m).is_ok());
    assert!(!m.wt_theme_added);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), PLAIN);
}

#[test]
fn remove_without_record_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let own = "{\n    \"theme\": \"system\",\n    \"profiles\": {}\n}\n";
    std::fs::write(&path, own).unwrap();
    remove(&path, &mut Installed::default());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), own, "nicht von uns eingetragen: bleibt");
}
