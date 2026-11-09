//! JSON so schreiben, wie Pythons `json.dump` es tut.
//!
//! Warum nicht einfach `serde_json::to_string`: Python- und Rust-Fassung schreiben
//! während der Umstellung dieselben Zustandsdateien. Gleiche Bytes machen jeden
//! Vergleich trivial (ein `diff` statt eines JSON-Vergleichs) und lassen keine Frage
//! offen, wer eine Datei zuletzt geschrieben hat. Die Unterschiede sind klein, aber
//! sichtbar: Python trennt mit `", "` und `": "`, maskiert ohne `ensure_ascii=False`
//! alles jenseits von ASCII als `\uXXXX` und schreibt Fließkommazahlen als `repr`
//! (`1e+16` statt `1e16`).

use serde_json::Value;

use crate::pystr::{PyNum, float_repr};

/// `json.dumps(v)`: kompakt mit Pythons Trennern, `ensure_ascii=True`.
/// So schreiben `paths.save_json` und damit alle Zustandsdateien.
pub fn dumps(v: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, v, None, 0, true);
    out
}

/// `json.dumps(v, indent=2, ensure_ascii=False)`: so schreibt das Deck
/// `~/.claude/settings.json` (lesbar, Umlaute in Hook-Pfaden bleiben Umlaute).
pub fn dumps_pretty(v: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, v, Some(2), 0, false);
    out
}

fn newline(out: &mut String, indent: Option<usize>, level: usize) {
    if let Some(n) = indent {
        out.push('\n');
        out.push_str(&" ".repeat(n * level));
    }
}

fn write_value(out: &mut String, v: &Value, indent: Option<usize>, level: usize, ascii: bool) {
    // Mit Einrückung trennt Python Elemente mit "," (das Leerzeichen ersetzt der
    // Zeilenumbruch), ohne mit ", ".
    let item_sep = if indent.is_some() { "," } else { ", " };
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => match PyNum::from_value(v) {
            Some(PyNum::Float(f)) => out.push_str(&float_repr(f)),
            _ => out.push_str(&n.to_string()),
        },
        Value::String(s) => write_str(out, s, ascii),
        Value::Array(a) => {
            if a.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, item) in a.iter().enumerate() {
                if i > 0 {
                    out.push_str(item_sep);
                }
                newline(out, indent, level + 1);
                write_value(out, item, indent, level + 1, ascii);
            }
            newline(out, indent, level);
            out.push(']');
        }
        Value::Object(o) => {
            if o.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (i, (k, item)) in o.iter().enumerate() {
                if i > 0 {
                    out.push_str(item_sep);
                }
                newline(out, indent, level + 1);
                write_str(out, k, ascii);
                out.push_str(": ");
                write_value(out, item, indent, level + 1, ascii);
            }
            newline(out, indent, level);
            out.push('}');
        }
    }
}

/// Text maskieren wie `json.encoder.(py_)encode_basestring(_ascii)`.
fn write_str(out: &mut String, s: &str, ascii: bool) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if ascii && (c as u32) > 0x7e => {
                // Außerhalb der Grundebene als Ersatzpaar, wie Python.
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{:04x}", unit));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}
