//! Die Flag-Zeile für `claude` (Python `test_domain_restart_plan.py`, Teil `flags`). Sie
//! geht als Text in eine Shell, also muss beweisbar sein, dass nur harmlose Werte
//! hineinkommen.

use deck_core::agent_flags::*;

const SID: &str = "78cbe9cd-67e4-4330-a2f3-34a9ee6bc20a";

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
