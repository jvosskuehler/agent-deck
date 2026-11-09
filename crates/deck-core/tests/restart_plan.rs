//! Der Neustart-Plan (Python `test_domain_restart_plan.py`). Die Flag-Zeile geht als
//! Text in eine Shell, also muss beweisbar sein, dass nur harmlose Werte hineinkommen;
//! und der Plan ist die einzige Stelle, an der Kachel und Unterhaltung zusammenhängen.

use std::collections::{BTreeMap, HashMap};

use deck_core::restart_plan::*;
use deck_core::slot_state::SlotState;
use serde_json::{Value, json};

const SID: &str = "78cbe9cd-67e4-4330-a2f3-34a9ee6bc20a";
const SID2: &str = "11111111-2222-3333-4444-555555555555";
const REPO: &str = r"C:\Users\jv\Repos\orpheus";
const WT: &str = r"C:\Users\jv\Repos\orpheus.wt\ticket-42";

fn st(v: Value) -> SlotState {
    v.as_object().unwrap().clone()
}

#[test]
fn resume_quotes_the_session_id_and_without_id_there_is_no_resume() {
    assert_eq!(flags(Some(SID), None, None, None), format!("--resume \"{SID}\""));
    assert_eq!(flags(None, None, None, None), "");
    assert_eq!(flags(Some(""), None, None, None), "");
}

#[test]
fn anything_but_a_uuid_is_kept_out_of_the_shell_line() {
    let bad = ["x\" ; rm -rf /", "a b", "$(whoami)", "'", "\"", &format!("{SID} extra"), "78cbe9cd67e443 30a2f334a9ee6bc20a"];
    for b in bad {
        assert_eq!(flags(Some(b), None, None, None), "", "{b}");
        assert!(!valid_session(b), "{b}");
    }
}

#[test]
fn uppercase_and_surrounding_whitespace_in_the_id_are_fine() {
    assert!(valid_session(&SID.to_uppercase()));
    assert_eq!(flags(Some(&format!("  {SID}  ")), None, None, None), format!("--resume \"{SID}\""));
}

#[test]
fn the_model_keeps_its_1m_suffix_in_quotes_and_odd_models_drop_out() {
    assert_eq!(flags(Some(SID), Some("opus[1m]"), None, None), format!("--resume \"{SID}\" --model \"opus[1m]\""));
    assert_eq!(flags(None, Some("opus\"; calc"), None, None), "");
    assert_eq!(flags(None, Some("opus 1m"), None, None), "");
}

#[test]
fn ultracode_falls_back_to_its_effort_level_and_only_valid_levels_pass() {
    assert_eq!(flags(None, None, Some("ultracode"), None), "--effort xhigh");
    assert_eq!(flags(None, None, Some("Ultracode"), None), "--effort xhigh");
    for lvl in ["low", "medium", "high", "xhigh", "max"] {
        assert_eq!(flags(None, None, Some(lvl), None), format!("--effort {lvl}"));
    }
    for junk in ["mittel", "XHIGH ; ls", ""] {
        assert!(!flags(None, None, Some(junk), None).contains("--effort"), "{junk}");
    }
}

#[test]
fn permission_mode_is_translated_and_bypass_is_never_forced() {
    assert_eq!(flags(None, None, None, Some("manual")), "--permission-mode default");
    assert_eq!(flags(None, None, None, Some("accept")), "--permission-mode acceptEdits");
    assert_eq!(flags(None, None, None, Some("auto")), "--permission-mode auto");
    assert_eq!(flags(None, None, None, Some("plan")), "--permission-mode plan");
    assert_eq!(flags(None, None, None, Some("bypass")), "");
    assert_eq!(flags(None, None, None, Some("dontask")), "");
}

#[test]
fn the_flag_order_is_fixed() {
    assert_eq!(
        flags(Some(SID), Some("sonnet"), Some("high"), Some("auto")),
        format!("--resume \"{SID}\" --model \"sonnet\" --effort high --permission-mode auto")
    );
}

#[test]
fn a_slot_carries_its_cwd_and_without_state_it_is_fresh() {
    let e = slot_entry("A2", Some(&st(json!({"session_id": SID, "cwd": WT}))), None, None, None);
    assert_eq!((e.cwd.as_str(), e.slot.as_str(), e.fresh), (WT, "A2", false));
    let f = slot_entry("A3", None, None, None, None);
    assert!(f.fresh && f.args.is_empty() && f.cwd.is_empty());
}

#[test]
fn remembered_effort_beats_the_reported_one() {
    let e = slot_entry("A1", Some(&st(json!({"session_id": SID, "effort": "xhigh"}))), None, Some("ultracode"), None);
    assert!(e.args.contains("--effort xhigh"));
    let e2 = slot_entry("A1", Some(&st(json!({"session_id": SID, "effort": "low"}))), None, None, None);
    assert!(e2.args.contains("--effort low"));
}

#[test]
fn an_id_without_transcript_counts_as_no_session_and_the_check_gets_id_and_cwd() {
    let s = st(json!({"session_id": SID, "cwd": REPO}));
    let no = slot_entry("A1", Some(&s), None, None, Some(&|_: &str, _: &str| false));
    assert!(no.fresh && !no.args.contains("--resume"));
    let yes = slot_entry("A1", Some(&s), None, None, Some(&|_: &str, _: &str| true));
    assert!(!yes.fresh && yes.args.contains("--resume"));
    let asked = std::cell::RefCell::new(Vec::new());
    slot_entry("A1", Some(&st(json!({"session_id": SID, "cwd": WT}))), None, None, Some(&|sid: &str, cwd: &str| {
        asked.borrow_mut().push((sid.to_string(), cwd.to_string()));
        true
    }));
    assert_eq!(asked.into_inner(), [(SID.to_string(), WT.to_string())]);
}

fn clients() -> Vec<ClientInfo> {
    vec![
        ClientInfo { window: Some("A".into()), workspace: Some("orpheus".into()), folder: Some(REPO.into()), slots: vec!["A1".into(), "A2".into(), "A3".into()] },
        ClientInfo { window: Some("B".into()), workspace: Some("leer".into()), folder: None, slots: vec!["B1".into()] },
        ClientInfo { window: None, workspace: Some("unverbunden".into()), folder: Some(REPO.into()), slots: vec![] },
    ]
}

fn states() -> HashMap<String, SlotState> {
    [
        ("A1".to_string(), st(json!({"session_id": SID, "cwd": REPO, "mode": "auto", "effort": "xhigh"}))),
        ("A2".to_string(), st(json!({"session_id": SID2, "cwd": WT, "mode": "plan"}))),
    ]
    .into()
}

fn plan_with(order: &[(&str, &[&str])], model: Option<&str>) -> Plan {
    let order: BTreeMap<String, Vec<String>> =
        order.iter().map(|(w, s)| (w.to_string(), s.iter().map(|x| x.to_string()).collect())).collect();
    build(&clients(), &states(), &order, model, &BTreeMap::new(), None, 1.0)
}

#[test]
fn a_window_without_folder_is_skipped_and_one_without_letter_does_not_count() {
    let plan = plan_with(&[], None);
    assert_eq!(plan.windows.iter().map(|w| w.window.as_str()).collect::<Vec<_>>(), ["A"]);
    assert_eq!(plan.skipped, ["B"]);
}

#[test]
fn the_dragged_tile_order_decides_the_start_order_and_unknown_slots_append() {
    let slots = |p: Plan| p.windows[0].slots.iter().map(|s| s.slot.clone()).collect::<Vec<_>>();
    assert_eq!(slots(plan_with(&[("A", &["A3", "A1", "A2"])], None)), ["A3", "A1", "A2"]);
    assert_eq!(slots(plan_with(&[("A", &["A2"])], None)), ["A2", "A1", "A3"]);
}

#[test]
fn counts_separate_resumable_from_fresh_and_the_deck_model_applies_to_all() {
    assert_eq!(counts(&plan_with(&[], None)), (1, 2, 1));
    for s in &plan_with(&[], Some("opus[1m]")).windows[0].slots {
        if !s.fresh {
            assert!(s.args.contains("--model \"opus[1m]\""));
        }
    }
}

#[test]
fn a_remote_window_opens_by_folder_uri() {
    assert_eq!(open_args(r"C:\repo"), ["--new-window", r"C:\repo"]);
    assert_eq!(open_args("vscode-remote://ssh-remote+10.8.0.10/home/x"), ["--new-window", "--folder-uri", "vscode-remote://ssh-remote+10.8.0.10/home/x"]);
}

#[test]
fn foreign_windows_are_named_by_repo_and_remote_tags_do_not_make_them_foreign() {
    let titles = ["a - deck - Visual Studio Code", "b - fremd - Visual Studio Code", "c - FREMD - Visual Studio Code",
        "x - jorrit [SSH: h] - Visual Studio Code", "Visual Studio Code"];
    assert_eq!(foreign_repos(&titles, &["deck", "jorrit [SSH: h]"]), ["fremd", "Visual Studio Code"]);
}

fn plan_value() -> Value {
    json!({"windows": [{"window": "A", "workspace": "orpheus", "folder": REPO, "slots": [{"slot": "A1", "args": "", "cwd": REPO}]}]})
}

#[test]
fn a_plan_whose_windows_are_all_closed_is_resumed() {
    assert!(needs_relaunch::<&str>(&plan_value(), &[]));
    assert!(needs_relaunch(&plan_value(), &["C", "D"]));
    assert!(!needs_relaunch(&plan_value(), &["A"]));
    assert!(!needs_relaunch(&plan_value(), &["a"]));
}

#[test]
fn junk_is_no_plan() {
    for x in [json!(null), json!({}), json!({"windows": []}), json!({"windows": [{"window": "A"}]}),
        json!({"windows": [{"folder": REPO}]}), json!("text"), json!(7)]
    {
        assert!(!is_usable(&x), "{x}");
        assert!(!needs_relaunch::<&str>(&x, &[]), "{x}");
    }
}

#[test]
fn saving_and_loading_survives_the_trip_over_the_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("restart_plan.json");
    let plan = plan_with(&[("A", &["A2", "A1"])], None);
    save(&plan, &path);
    let back = load(&path).expect("Plan");
    assert_eq!(back.windows[0].slots.iter().map(|s| s.slot.as_str()).collect::<Vec<_>>(), ["A2", "A1", "A3"]);
    assert_eq!(back, plan);
    clear(&path);
    assert!(load(&path).is_none());
    clear(&path);
}
