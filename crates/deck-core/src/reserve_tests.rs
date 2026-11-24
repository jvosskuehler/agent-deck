use serde_json::json;

use super::*;

fn recipe(model: &str) -> Recipe {
    Recipe { win: "B".into(), folder: r"C:\Repos".into(), argv: vec!["--model".into(), model.into()], scheme: None, wsl: None }
}

fn reserve(since: f64, ready: bool) -> Reserve {
    Reserve { slot: "B9".into(), recipe: recipe("opus"), since, ready }
}

fn sit<'a>(want: Option<&'a Recipe>, current: Option<&'a Reserve>) -> Situation<'a> {
    Situation { now: 1000.0, idle_s: 120.0, front_class: Some("Notepad"), want, current, alive: true, spawning: true, retry_after: 0.0 }
}

#[test]
fn a_reserve_is_only_opened_after_a_minute_without_input() {
    let want = recipe("opus");
    assert_eq!(decide(&sit(Some(&want), None)), Step::Spawn);
    let thinking = Situation { idle_s: 30.0, ..sit(Some(&want), None) };
    assert_eq!(decide(&thinking), Step::Keep, "eine Denkpause ist keine Ruhe");
    let minute = Situation { idle_s: IDLE_BEFORE_SPAWN_S, ..sit(Some(&want), None) };
    assert_eq!(decide(&minute), Step::Spawn);
}

#[test]
fn no_reserve_is_opened_while_a_terminal_or_agent_is_in_front() {
    let want = recipe("opus");
    for class in TERMINAL_CLASSES {
        let front = Situation { front_class: Some(class), ..sit(Some(&want), None) };
        assert_eq!(decide(&front), Step::Keep, "{class}");
    }
    let nothing = Situation { front_class: None, ..sit(Some(&want), None) };
    assert_eq!(decide(&nothing), Step::Spawn, "ohne Vordergrundfenster (gesperrt) ist es ruhig");
}

#[test]
fn no_reserve_is_opened_when_none_is_wanted_or_after_a_failure() {
    assert_eq!(decide(&sit(None, None)), Step::Keep);
    let want = recipe("opus");
    let cooling = Situation { retry_after: 1200.0, ..sit(Some(&want), None) };
    assert_eq!(decide(&cooling), Step::Keep);
}

#[test]
fn a_reserve_with_other_settings_is_retired_at_once() {
    let cur = reserve(990.0, true);
    let want = recipe("fable");
    let busy = Situation { idle_s: 0.0, ..sit(Some(&want), Some(&cur)) };
    assert_eq!(decide(&busy), Step::Retire("Einstellungen geändert"));
    assert_eq!(decide(&sit(None, Some(&cur))), Step::Retire("abgeschaltet"));
}

#[test]
fn a_reserve_for_another_environment_does_not_fit() {
    let cur = reserve(990.0, true);
    let mut in_wsl = recipe("opus");
    in_wsl.wsl = Some("Ubuntu".into());
    assert_eq!(decide(&sit(Some(&in_wsl), Some(&cur))), Step::Retire("Einstellungen geändert"));
    assert!(!fits(Some(&cur), &in_wsl));
}

#[test]
fn a_fresh_reserve_gets_time_to_start_its_shell() {
    let want = recipe("opus");
    let cur = reserve(990.0, false);
    assert_eq!(decide(&Situation { alive: false, ..sit(Some(&want), Some(&cur)) }), Step::Keep);
    let late = reserve(900.0, false);
    assert_eq!(decide(&Situation { alive: false, ..sit(Some(&want), Some(&late)) }), Step::Retire("Shell lebt nicht"));
}

#[test]
fn an_unfinished_reserve_left_over_from_an_earlier_run_is_retired() {
    let want = recipe("opus");
    let cur = reserve(995.0, false);
    let left_over = Situation { spawning: false, idle_s: 0.0, ..sit(Some(&want), Some(&cur)) };
    assert_eq!(decide(&left_over), Step::Retire(LEFT_OVER), "auch wenn gerade jemand tippt");
    let ready = reserve(995.0, true);
    assert_eq!(decide(&Situation { spawning: false, ..sit(Some(&want), Some(&ready)) }), Step::Keep, "eine fertige übersteht den Neustart");
}

#[test]
fn a_ready_reserve_without_a_shell_yet_waits_and_is_retired_only_later() {
    let want = recipe("opus");
    let fresh = reserve(995.0, true);
    assert_eq!(decide(&Situation { alive: false, ..sit(Some(&want), Some(&fresh)) }), Step::Keep, "Fenster da, PID-Datei noch nicht");
    let dead = reserve(900.0, true);
    assert_eq!(decide(&Situation { alive: false, ..sit(Some(&want), Some(&dead)) }), Step::Retire("Shell lebt nicht"));
}

#[test]
fn an_old_reserve_is_swapped_only_while_it_is_quiet() {
    let want = recipe("opus");
    let old = reserve(1000.0 - MAX_AGE_S - 1.0, true);
    assert_eq!(decide(&sit(Some(&want), Some(&old))), Step::Retire("zu alt"));
    assert_eq!(decide(&Situation { idle_s: 1.0, ..sit(Some(&want), Some(&old)) }), Step::Keep);
    let agent_in_front = Situation { front_class: Some(crate::wt::WINDOW_CLASS), ..sit(Some(&want), Some(&old)) };
    assert_eq!(decide(&agent_in_front), Step::Keep);
}

#[test]
fn input_during_the_spawn_counts_only_if_it_came_after_the_start() {
    assert!(typed_while_spawning(0.2, 1.5), "vor 0,2 s getippt, Start vor 1,5 s");
    assert!(!typed_while_spawning(61.5, 1.5), "seit dem Start nichts");
}

#[test]
fn only_a_ready_reserve_with_the_same_recipe_fits() {
    let ask = recipe("opus");
    assert!(fits(Some(&reserve(0.0, true)), &ask));
    assert!(!fits(Some(&reserve(0.0, false)), &ask));
    assert!(!fits(Some(&reserve(0.0, true)), &recipe("fable")));
    assert!(!fits(None, &ask));
}

#[test]
fn the_remembered_reserve_survives_a_round_trip_and_garbage_reads_as_none() {
    let r = reserve(12.5, true);
    assert_eq!(load(Some(serde_json::to_value(&r).unwrap())), Some(r));
    assert_eq!(load(Some(json!({"slot": 3}))), None);
    assert_eq!(load(None), None);
}

#[test]
fn a_file_from_before_the_environment_field_still_reads_as_windows() {
    // So steht die Datei seit dem 20.11.2026 auf dem Rechner des Autors.
    let v = json!({"slot": "B99", "recipe": {"win": "B", "folder": r"C:\Repos", "argv": ["--model", "opus[1m]"], "scheme": null},
        "since": 1795504879.37, "ready": true});
    let r = load(Some(v)).expect("lesbar");
    assert_eq!((r.slot.as_str(), r.recipe.wsl.as_deref(), r.ready), ("B99", None, true));
    let back = serde_json::to_value(&r).unwrap();
    assert!(back["recipe"].get("wsl").is_none(), "Windows schreibt das Feld nicht");
}
