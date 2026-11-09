//! Einzelne Felder aus fremdem JSON tolerant lesen.
//!
//! Hook-stdin, Zustandsdateien und API-Antworten schreiben andere Programme, oft in
//! einer neueren Version als der, gegen die dieser Code geschrieben ist. Die Regel ist
//! deshalb überall dieselbe: **ein Feld mit falschem Typ gilt als fehlend**, nie als
//! Fehler und nie umgedeutet (keine Zahl aus einem Text, kein Text aus einer Zahl).

use serde_json::Value;

/// Ein nicht leerer Text.
pub fn text(v: Option<&Value>) -> Option<&str> {
    v?.as_str().filter(|s| !s.is_empty())
}

/// Eine JSON-Zahl als `f64`; ein Wahrheitswert ist keine Zahl.
pub fn number(v: Option<&Value>) -> Option<f64> {
    v?.as_f64()
}

/// Eine JSON-Zahl, unverändert (eine ganze Zahl bleibt ganz, wenn sie weitergeschrieben
/// wird).
pub fn raw_number(v: Option<&Value>) -> Option<&Value> {
    v.filter(|v| v.is_number())
}

/// Ein Schalter: nur `true` ist an.
pub fn flag(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Bool(true)))
}
