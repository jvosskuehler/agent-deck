//! Python-Semantik für Texte und JSON-Werte, die der Port Zeichen für Zeichen treffen muss.
//!
//! Die Python-Fassung und diese Fassung schreiben während der Umstellung dieselben
//! Dateien und dieselbe Statuszeile. Wo Python eine eigene Regel hat, die Rust anders
//! auslegt, steht sie hier genau einmal: was `str.strip()` als Leerraum zählt (auch
//! `\x1c`–`\x1f`), wo `splitlines()` trennt, welche Werte „falsy“ sind, wie `str()` und
//! `repr()` einen Wert drucken und wie `round()` rundet (Banker's Rounding).
//!
//! Längen und Schnitte zählen wie in Python in Codepunkten, nicht in Bytes.

use serde_json::Value;

/// Leerraum im Sinne von Python `str.isspace()`: Unicode-White-Space plus die vier
/// Trennzeichen `\x1c`–`\x1f`, die Rust nicht dazuzählt.
pub fn is_py_space(c: char) -> bool {
    c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

/// `s.strip()`.
pub fn strip(s: &str) -> &str {
    s.trim_matches(is_py_space)
}

/// `s.rstrip()`.
pub fn rstrip(s: &str) -> &str {
    s.trim_end_matches(is_py_space)
}

/// `s.split()` ohne Argument: an Leerraum-Läufen trennen, leere Stücke fallen weg.
pub fn split_ws(s: &str) -> Vec<&str> {
    s.split(is_py_space).filter(|p| !p.is_empty()).collect()
}

/// `" ".join(s.split())`: Leerraum glätten.
pub fn squash_ws(s: &str) -> String {
    split_ws(s).join(" ")
}

/// Zeilengrenzen von `str.splitlines()`.
fn is_line_break(c: char) -> bool {
    matches!(
        c,
        '\n' | '\r' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

/// `s.splitlines()`: `\r\n` zählt als eine Grenze, ein Umbruch am Ende erzeugt keine
/// leere letzte Zeile.
pub fn splitlines(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut it = s.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        if is_line_break(c) {
            out.push(&s[start..i]);
            let mut end = i + c.len_utf8();
            if c == '\r'
                && let Some(&(j, '\n')) = it.peek()
            {
                end = j + 1;
                it.next();
            }
            start = end;
        }
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

/// Länge in Codepunkten (`len(s)`).
pub fn char_len(s: &str) -> usize {
    s.chars().count()
}

/// `s[:n]` in Codepunkten.
pub fn take_chars(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// `s[n:]` in Codepunkten.
pub fn skip_chars(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[i..],
        None => "",
    }
}

/// Die letzten höchstens `n` Codepunkte vor dem Byte-Index `pos`
/// (`text[max(0, pos - n):pos]` mit `pos` als Codepunkt-Index in Python).
pub fn chars_before(s: &str, pos: usize, n: usize) -> &str {
    let head = &s[..pos];
    match head.char_indices().rev().nth(n.saturating_sub(1)) {
        Some((i, _)) if n > 0 => &head[i..],
        _ if n == 0 => "",
        _ => head,
    }
}

/// Wahrheitswert eines JSON-Werts wie in Python: `None`, `False`, `0`, `0.0`, `""`,
/// `[]` und `{}` sind falsch.
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// `opt or default` für Optionen auf JSON-Werte: der Wert, wenn er wahr ist.
pub fn truthy_opt(v: Option<&Value>) -> Option<&Value> {
    v.filter(|x| truthy(x))
}

/// Python `repr(float)`: kürzeste Darstellung, die zurück zur selben Zahl führt;
/// Exponentschreibweise ab 1e16 bzw. unter 1e-4 (`1e+16`, `1e-05`), sonst mit `.0`.
pub fn float_repr(f: f64) -> String {
    if f.is_nan() {
        return "nan".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "inf".into() } else { "-inf".into() };
    }
    // Rusts `{:e}` liefert ebenfalls die kürzeste Ziffernfolge, nur anders angeordnet.
    let sci = format!("{:e}", f.abs());
    let (mant, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let decpt = exp + 1;
    let sign = if f.is_sign_negative() { "-" } else { "" };
    let n = digits.len() as i32;
    let body = if decpt <= -4 || decpt > 16 {
        let tail = if n > 1 { format!(".{}", &digits[1..]) } else { String::new() };
        let e = decpt - 1;
        format!("{}{}e{}{:02}", &digits[..1], tail, if e < 0 { '-' } else { '+' }, e.abs())
    } else if decpt <= 0 {
        format!("0.{}{}", "0".repeat((-decpt) as usize), digits)
    } else if decpt >= n {
        format!("{}{}.0", digits, "0".repeat((decpt - n) as usize))
    } else {
        format!("{}.{}", &digits[..decpt as usize], &digits[decpt as usize..])
    };
    format!("{sign}{body}")
}

/// Python `round(x)` ohne Stellenangabe: auf die nächste ganze Zahl, bei genau .5 zur
/// geraden (Banker's Rounding).
pub fn round_half_even(f: f64) -> f64 {
    f.round_ties_even()
}

/// Eine Zahl, wie Python sie nach `json.loads` hält: `int` oder `float` (ein `bool`
/// zählt in Python als `int`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PyNum {
    Int(i128),
    Float(f64),
}

impl PyNum {
    /// `isinstance(v, (int, float))` – also auch `True`/`False`.
    pub fn from_value(v: &Value) -> Option<PyNum> {
        match v {
            Value::Bool(b) => Some(PyNum::Int(i128::from(*b))),
            Value::Number(n) => Some(if let Some(i) = n.as_i64() {
                PyNum::Int(i128::from(i))
            } else if let Some(u) = n.as_u64() {
                PyNum::Int(i128::from(u))
            } else {
                PyNum::Float(n.as_f64().unwrap_or(0.0))
            }),
            _ => None,
        }
    }

    pub fn as_f64(self) -> f64 {
        match self {
            PyNum::Int(i) => i as f64,
            PyNum::Float(f) => f,
        }
    }

    /// `round(x)` als Text: eine ganze Zahl ohne Nachkommastellen.
    pub fn round_str(self) -> String {
        match self {
            PyNum::Int(i) => i.to_string(),
            PyNum::Float(f) => format!("{:.0}", round_half_even(f)),
        }
    }

    /// `a + b` mit Pythons Typregel: int + int bleibt int, sonst float.
    pub fn py_add(self, other: PyNum) -> PyNum {
        match (self, other) {
            (PyNum::Int(a), PyNum::Int(b)) => PyNum::Int(a + b),
            (a, b) => PyNum::Float(a.as_f64() + b.as_f64()),
        }
    }

    pub fn to_value(self) -> Value {
        match self {
            PyNum::Int(i) => i64::try_from(i)
                .map(Value::from)
                .or_else(|_| u64::try_from(i).map(Value::from))
                .unwrap_or_else(|_| Value::from(i as f64)),
            PyNum::Float(f) => serde_json::Number::from_f64(f).map_or(Value::Null, Value::Number),
        }
    }
}

/// `float(v)` auf einen JSON-Wert: Zahlen, Wahrheitswerte und Zahl-Texte; sonst `None`
/// (Python würde dort einen Fehler werfen).
pub fn py_float(v: &Value) -> Option<f64> {
    match v {
        Value::String(s) => {
            let t = strip(s).replace('_', "");
            match t.to_ascii_lowercase().as_str() {
                "inf" | "+inf" | "infinity" | "+infinity" => Some(f64::INFINITY),
                "-inf" | "-infinity" => Some(f64::NEG_INFINITY),
                "nan" | "+nan" | "-nan" => Some(f64::NAN),
                _ => t.parse().ok(),
            }
        }
        other => PyNum::from_value(other).map(PyNum::as_f64),
    }
}

/// `str(v)` eines aus JSON geladenen Werts.
pub fn py_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => py_repr(other),
    }
}

/// `repr(v)` eines aus JSON geladenen Werts. Für Texte eine Näherung (Python lässt
/// druckbare Unicode-Zeichen stehen und maskiert nur Steuerzeichen) – Texte in Listen
/// oder Objekten kommen in den Hook-Feldern praktisch nie vor.
pub fn py_repr(v: &Value) -> String {
    match v {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Number(n) => match PyNum::from_value(v) {
            Some(PyNum::Float(f)) => float_repr(f),
            _ => n.to_string(),
        },
        Value::String(s) => str_repr(s),
        Value::Array(a) => format!("[{}]", a.iter().map(py_repr).collect::<Vec<_>>().join(", ")),
        Value::Object(o) => format!(
            "{{{}}}",
            o.iter()
                .map(|(k, v)| format!("{}: {}", str_repr(k), py_repr(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn str_repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}
