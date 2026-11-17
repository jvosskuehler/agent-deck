use serde_json::{Map, Value, json};

use super::*;

fn map(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => panic!("kein Objekt"),
    }
}

#[test]
fn a_missing_or_empty_file_is_a_fresh_install() {
    assert_eq!(origin(None), Origin::Fresh);
    assert_eq!(origin(Some(b"")), Origin::Fresh);
    assert_eq!(origin(Some(b"  \r\n")), Origin::Fresh);
    assert_eq!(origin(Some(b"{ }")), Origin::Fresh);
    assert_eq!(origin(Some(br#"{"dock_edge":"top"}"#)), Origin::Existing);
    assert_eq!(origin(Some(b"{kaputt")), Origin::Existing, "unlesbar, aber da: lieber wie bisher");
}

#[test]
fn a_fresh_install_gets_only_the_version_and_everything_with_side_effects_stays_off() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir(home.path().join("Repos")).unwrap();
    let mut s = Map::new();
    assert!(migrate(&mut s, Origin::Fresh, home.path()));
    assert_eq!(Value::Object(s.clone()), json!({"settings_version": 1}));
    for sw in Switch::ALL {
        assert_eq!(on(&s, sw), sw == Switch::Dashboard, "{}", sw.key());
    }
    assert_eq!(model(&s), None, "ohne --model");
    assert_eq!(jira_prefix(&s), "");
    assert_eq!(repos_root(&s), None, "kein still angenommenes ~/Repos");
}

#[test]
fn an_existing_install_without_version_keeps_todays_behaviour() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir(home.path().join("Repos")).unwrap();
    let mut s = map(json!({"dock_edge": "left"}));
    assert!(migrate(&mut s, Origin::Existing, home.path()));
    for sw in Switch::ALL {
        assert_eq!(s.get(sw.key()), Some(&json!(true)), "{} ausdrücklich an", sw.key());
    }
    assert_eq!(s["model"], "opus[1m]");
    assert_eq!(s["jira_prefix"], "ORPH");
    assert_eq!(s["repos_root"], json!(home.path().join("Repos").to_string_lossy()));
    assert_eq!(s["settings_version"], 1);
    assert_eq!(s["dock_edge"], "left");
}

#[test]
fn the_migration_keeps_values_the_owner_already_chose() {
    let home = tempfile::tempdir().unwrap();
    let mut s = map(json!({
        "auto_compact": false, "speechmike_voice": false, "model": "fable",
        "jira_prefix": "ABC", "terminal_home": "D:/Code"
    }));
    migrate(&mut s, Origin::Existing, home.path());
    assert_eq!(s["auto_compact"], false);
    assert_eq!(s["speechmike_voice"], false);
    assert_eq!(s["titan"], true);
    assert_eq!(s["model"], "fable");
    assert_eq!(s["jira_prefix"], "ABC");
    assert_eq!(s["repos_root"], "D:/Code", "der alte Schlüssel wird übernommen");
}

#[test]
fn without_a_repos_folder_the_migration_sets_none() {
    let home = tempfile::tempdir().unwrap();
    let mut s = map(json!({"dock_edge": "left"}));
    migrate(&mut s, Origin::Existing, home.path());
    assert!(!s.contains_key("repos_root"));
}

#[test]
fn a_versioned_file_is_left_alone_even_with_missing_switches() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir(home.path().join("Repos")).unwrap();
    let installer = json!({"settings_version": 1, "repos_root": "C:/src", "titan": false});
    let mut s = map(installer.clone());
    assert!(!migrate(&mut s, Origin::Existing, home.path()));
    assert_eq!(Value::Object(s.clone()), installer);
    assert!(!on(&s, Switch::StoreUpdate), "fehlt = Vorgabe aus");
}

#[test]
fn a_switch_with_the_wrong_type_counts_as_missing() {
    let s = map(json!({"titan": "ja", "dashboard": 0}));
    assert!(!on(&s, Switch::Titan));
    assert!(on(&s, Switch::Dashboard));
}

#[test]
fn the_old_home_key_still_names_the_repos_folder() {
    assert_eq!(repos_root(&map(json!({"terminal_home": " C:/R "}))), Some("C:/R".into()));
    assert_eq!(repos_root(&map(json!({"repos_root": "C:/A", "terminal_home": "C:/B"}))), Some("C:/A".into()));
    assert_eq!(repos_root(&map(json!({"repos_root": "  "}))), None);
}

#[test]
fn a_repos_folder_counts_only_with_a_git_repo_inside() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(repos_state(None), ReposState::Unset);
    assert_eq!(repos_state(Some(&dir.path().join("fehlt"))), ReposState::Missing);
    std::fs::create_dir(dir.path().join("notizen")).unwrap();
    assert_eq!(repos_state(Some(dir.path())), ReposState::NoRepos);
    std::fs::create_dir_all(dir.path().join("api").join(".git")).unwrap();
    assert_eq!(repos_state(Some(dir.path())), ReposState::Ok);
    let s = map(json!({"repos_root": dir.path().to_string_lossy()}));
    assert_eq!(usable_repos_root(&s).as_deref(), Some(dir.path()));
}

#[test]
fn the_jira_prefix_is_upper_case_and_empty_means_none() {
    assert_eq!(jira_prefix(&map(json!({"jira_prefix": " orph "}))), "ORPH");
    assert_eq!(jira_prefix(&map(json!({"jira_prefix": 5}))), "");
}
