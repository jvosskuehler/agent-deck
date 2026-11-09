//! Zeichen statt Bytes (`text`), tolerantes Lesen fremder Felder (`field`) und das
//! Format, in dem `save_json` die Zustandsdateien schreibt.

use deck_core::field::{flag, number, raw_number, text};
use deck_core::paths::{load_json, save_json};
use deck_core::text::*;
use serde_json::{Value, json};

#[test]
fn slices_count_characters_not_bytes() {
    assert_eq!(take_chars("Müßig", 3), "Müß");
    assert_eq!(take_chars("ab", 5), "ab");
    assert_eq!(skip_chars("Müßig", 3), "ig");
    assert_eq!(skip_chars("ab", 5), "");
    assert_eq!(char_len("für"), 3);
    let s = "abcÄÖÜdef";
    assert_eq!(chars_before(s, s.find('d').unwrap(), 2), "ÖÜ");
    assert_eq!(chars_before(s, 2, 24), "ab");
    assert_eq!(chars_before(s, 2, 0), "");
}

#[test]
fn whitespace_is_squashed_and_the_first_line_is_trimmed() {
    assert_eq!(squash_ws("  a \n\t b  "), "a b");
    assert_eq!(squash_ws(" "), "");
    assert_eq!(first_line("  ORPH-1 \r\nRest"), "ORPH-1");
    assert_eq!(first_line(""), "");
}

#[test]
fn a_field_of_the_wrong_type_counts_as_missing() {
    assert_eq!(text(Some(&json!("x"))), Some("x"));
    for v in [json!(""), json!(5), json!(null), json!(["x"])] {
        assert_eq!(text(Some(&v)), None, "{v}");
    }
    assert_eq!(number(Some(&json!(2))), Some(2.0));
    assert_eq!(number(Some(&json!(1.5))), Some(1.5));
    for v in [json!("1.5"), json!(true), json!(null)] {
        assert_eq!(number(Some(&v)), None, "keine Zahl aus {v}");
    }
    assert!(raw_number(Some(&json!(41))).is_some_and(Value::is_u64), "eine ganze Zahl bleibt ganz");
    assert_eq!(raw_number(Some(&json!("41"))), None);
    assert!(flag(Some(&json!(true))));
    for v in [json!(false), json!(1), json!("true"), json!(null)] {
        assert!(!flag(Some(&v)), "{v}");
    }
    assert_eq!((text(None), number(None), flag(None)), (None, None, false));
}

#[test]
fn state_files_are_compact_utf8_json_with_key_order_and_number_types_kept() {
    // Die App und die Hooks lesen diese Dateien, das Format bleibt stabil.
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("A1.json");
    let v = json!({"slot": "A1", "status": "done", "ts": 1_762_467_890.0, "n": 3, "prompt": "für 😀 \"x\"\n"});
    save_json(&p, &v).unwrap();
    let raw = std::fs::read_to_string(&p).unwrap();
    assert_eq!(raw, "{\"slot\":\"A1\",\"status\":\"done\",\"ts\":1762467890.0,\"n\":3,\"prompt\":\"für 😀 \\\"x\\\"\\n\"}");
    let back = load_json(&p).unwrap();
    assert_eq!(back, v);
    assert!(back["ts"].is_f64() && back["n"].is_u64());
    assert!(!dir.path().join("A1.json.tmp").exists(), "atomar: keine Reste");
}
