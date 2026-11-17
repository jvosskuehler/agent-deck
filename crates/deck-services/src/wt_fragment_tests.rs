use super::*;

const SHADER: &str = r"C:\Users\x\AppData\Local\claude-agent-deck\wt-faint-hell.hlsl";
const CLAUDE: &str = r"C:\Users\x\.local\bin\claude.exe";

fn plain() -> Value {
    fragment(None, shell_commandline(false), None)
}

#[test]
fn the_profile_is_exactly_the_authors_papier_profile() {
    let f = fragment(Some(CLAUDE), shell_commandline(true), Some(SHADER));
    let want = json!({"closeOnExit": "always", "colorScheme": {"light": "papier-ebenen", "dark": "papier-ebenen-nacht"},
        "commandline": "pwsh.exe -NoLogo", "font": {"face": "IBM Plex Mono", "size": 10.5}, "hidden": true, "icon": CLAUDE,
        "name": "Agent Deck", "scrollbarState": "hidden", "suppressApplicationTitle": true, "intenseTextStyle": "bold",
        "experimental.pixelShaderPath": SHADER});
    assert_eq!(f["profiles"][0], want);
    let keys: Vec<&String> = f["profiles"][0].as_object().unwrap().keys().collect();
    let want_keys: Vec<&String> = want.as_object().unwrap().keys().collect();
    assert_eq!(keys, want_keys, "auch die Reihenfolge");
}

#[test]
fn without_pwsh_icon_or_shader_the_profile_still_works() {
    let f = plain();
    let p = &f["profiles"][0];
    assert_eq!(p["commandline"], "powershell.exe -NoLogo");
    assert!(p.get("icon").is_none());
    assert!(p.get("experimental.pixelShaderPath").is_none(), "ein fehlender Shader wäre eine Fehlermeldung im Terminal");
    assert!(p.get("guid").is_none(), "Windows Terminal leitet die guid aus Name und Quelle ab");
    assert_eq!(p["closeOnExit"], "always", "sonst bleibt „[process exited …]“ stehen");
    assert_eq!(p["hidden"], true);
}

#[test]
fn both_schemes_exist_and_the_profile_uses_them() {
    let f = fragment(Some(CLAUDE), shell_commandline(true), None);
    let names: Vec<&str> = f["schemes"].as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap()).collect();
    assert_eq!(names, [SCHEME_LIGHT, SCHEME_DARK]);
    assert_eq!(f["profiles"][0]["colorScheme"]["light"], SCHEME_LIGHT);
    assert_eq!(f["profiles"][0]["colorScheme"]["dark"], SCHEME_DARK);
    for s in f["schemes"].as_array().unwrap() {
        assert_eq!(s.as_object().unwrap().len(), 21, "Name, 4 Grundfarben, 16 ANSI-Farben: {}", s["name"]);
    }
}

#[test]
fn the_light_paper_matches_the_border_the_deck_draws() {
    let f = plain();
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
fn install_writes_once_records_it_and_remove_takes_the_folder_with_it() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("Fragments").join(SOURCE).join(FILE);
    let wt = d.path().join("wt-settings.json");
    let mut m = Installed::default();
    let r = install(&path, &wt, &plain(), &mut m);
    assert!(r.is_ok() && r.changed, "{r:?}");
    let written: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(written, plain());
    assert_eq!(m.wt_fragment.as_deref(), Some(path.to_string_lossy().as_ref()), "verzeichnet");
    assert!(!install(&path, &wt, &plain(), &mut m).changed, "zweiter Lauf: nichts zu tun");
    let r = remove(&path, &mut m);
    assert!(r.changed && m.wt_fragment.is_none());
    assert!(!path.parent().unwrap().exists());
    assert!(remove(&path, &mut m).is_ok());
}

#[test]
fn with_an_own_profile_no_fragment_is_written() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join(SOURCE).join(FILE);
    let wt = d.path().join("wt-settings.json");
    std::fs::write(&wt, r#"{"profiles": {"list": [{"name": "Agent Deck"}]}}"#).unwrap();
    let mut m = Installed::default();
    let r = install(&path, &wt, &plain(), &mut m);
    assert!(r.is_ok() && !r.changed);
    assert!(!path.exists() && m.wt_fragment.is_none());
}

#[test]
fn the_real_fragment_is_locked_in_tests() {
    let d = tempfile::tempdir().unwrap();
    let mut m = Installed::default();
    assert_eq!(install(&fragment_path(), &d.path().join("none.json"), &plain(), &mut m).fails(), 1);
    assert_eq!(remove(&fragment_path(), &mut m).fails(), 1);
}
