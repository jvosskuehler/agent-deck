//! Die Python-Semantik, die beide Fassungen teilen müssen. Die Erwartungen sind mit
//! Python 3.14 erzeugt (`repr`, `'%.2f'`, `json.dumps`, `round`).

use deck_core::pyjson::{dumps, dumps_pretty};
use deck_core::pystr::*;
use serde_json::json;

#[test]
fn float_repr_matches_python_including_exponent_form() {
    let cases = [
        (0.125, "0.125"),
        (1e16, "1e+16"),
        (1e15, "1000000000000000.0"),
        (123456789012345678.0, "1.2345678901234568e+17"),
        (0.0001, "0.0001"),
        (0.00001, "1e-05"),
        (1.5, "1.5"),
        (-0.0, "-0.0"),
        (1762467890.1234567, "1762467890.1234567"),
        (1e-7, "1e-07"),
        (5e-324, "5e-324"),
        (1.7976931348623157e308, "1.7976931348623157e+308"),
        (100.0, "100.0"),
        (0.1 + 0.2, "0.30000000000000004"),
        (0.0, "0.0"),
    ];
    for (v, want) in cases {
        assert_eq!(float_repr(v), want, "{v}");
    }
}

#[test]
fn two_decimals_round_like_python_percent_format() {
    for (v, want) in [(0.125, "0.12"), (0.375, "0.38"), (2.675, "2.67"), (1.005, "1.00"), (0.005, "0.01"), (1e16, "10000000000000000.00")] {
        assert_eq!(format!("{v:.2}"), want, "{v}");
    }
}

#[test]
fn round_uses_bankers_rounding_like_python() {
    for (v, want) in [(0.5, "0"), (1.5, "2"), (2.5, "2"), (41.5, "42"), (1e20, "100000000000000000000"), (1e23, "99999999999999991611392")] {
        assert_eq!(PyNum::Float(v).round_str(), want, "{v}");
    }
    assert_eq!(PyNum::Int(41).round_str(), "41");
}

#[test]
fn strip_and_split_count_python_whitespace_including_separator_controls() {
    assert_eq!(strip("\x1c a \x1f\u{3000}"), "a");
    assert_eq!(squash_ws("  a \n\t b  "), "a b");
    assert_eq!(split_ws(" "), Vec::<&str>::new());
}

#[test]
fn splitlines_knows_all_python_line_boundaries() {
    assert_eq!(splitlines("a\r\nb\rc\nd\u{2028}e\x0bf"), ["a", "b", "c", "d", "e", "f"]);
    assert_eq!(splitlines("a\n"), ["a"]);
    assert_eq!(splitlines("\n\n"), ["", ""]);
    assert_eq!(splitlines(""), Vec::<&str>::new());
}

#[test]
fn slices_count_code_points_not_bytes() {
    assert_eq!(take_chars("Müßig", 3), "Müß");
    assert_eq!(skip_chars("Müßig", 3), "ig");
    assert_eq!(char_len("für"), 3);
    let s = "abcÄÖÜdef";
    assert_eq!(chars_before(s, s.find('d').unwrap(), 2), "ÖÜ");
    assert_eq!(chars_before(s, 2, 24), "ab");
}

#[test]
fn falsy_values_are_the_python_ones() {
    for v in [json!(null), json!(false), json!(0), json!(0.0), json!(""), json!([]), json!({})] {
        assert!(!truthy(&v), "{v}");
    }
    for v in [json!(true), json!(1), json!(-0.5), json!("x"), json!([0]), json!({"a": 0})] {
        assert!(truthy(&v), "{v}");
    }
}

#[test]
fn str_of_json_values_prints_like_python() {
    assert_eq!(py_str(&json!(null)), "None");
    assert_eq!(py_str(&json!(true)), "True");
    assert_eq!(py_str(&json!(5)), "5");
    assert_eq!(py_str(&json!(1e16)), "1e+16");
    assert_eq!(py_str(&json!("x")), "x");
    assert_eq!(py_str(&json!([1, "a", null])), "[1, 'a', None]");
    assert_eq!(py_str(&json!({"k": "it's"})), "{'k': \"it's\"}");
}

#[test]
fn adding_numbers_keeps_int_unless_a_float_takes_part() {
    assert_eq!(PyNum::Int(2).py_add(PyNum::Int(3)), PyNum::Int(5));
    assert_eq!(PyNum::Int(2).py_add(PyNum::Float(0.5)), PyNum::Float(2.5));
    assert_eq!(PyNum::from_value(&json!(true)), Some(PyNum::Int(1)));
}

#[test]
fn float_parses_numeric_text_like_python() {
    assert_eq!(py_float(&json!(" 1.5 ")), Some(1.5));
    assert_eq!(py_float(&json!("kaputt")), None);
    assert_eq!(py_float(&json!(true)), Some(1.0));
    assert_eq!(py_float(&json!(null)), None);
}

#[test]
fn compact_json_is_byte_identical_to_python_dumps() {
    let v = json!({"slot": "A1", "ts": 1762467890.5, "prompt": "für 😀 \"x\"\n\u{7f}", "n": [1, 2.0, null, true]});
    // python -c "import json; print(json.dumps({...}))"
    assert_eq!(
        dumps(&v),
        // Die Maskierungen stückweise, damit kein Werkzeug sie beim Schreiben auflöst.
        concat!(
            "{\"slot\": \"A1\", \"ts\": 1762467890.5, \"prompt\": \"f", "\\", "u00fcr ", "\\", "ud83d", "\\",
            "ude00 \\\"x\\\"\\n", "\\", "u007f\", \"n\": [1, 2.0, null, true]}"
        )
    );
    assert_eq!(dumps(&json!({})), "{}");
}

#[test]
fn pretty_json_matches_python_indent_two_without_ascii_escaping() {
    let v = json!({"a": {"b": [1, {}], "c": []}, "d": "Voßkühler"});
    assert_eq!(dumps_pretty(&v), "{\n  \"a\": {\n    \"b\": [\n      1,\n      {}\n    ],\n    \"c\": []\n  },\n  \"d\": \"Voßkühler\"\n}");
}
