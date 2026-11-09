//! Lesen und Schreiben von Claude Codes globaler `~/.claude/settings.json`.
//!
//! Das Einstellungsfenster des Decks steuert damit die Defaults für NEU gestartete
//! Agenten: `model`, `permissions.defaultMode`, `effortLevel` (+ `ultracode`) und
//! `language`. Es wird immer in die bestehende Datei GEMERGT (Hooks, statusLine,
//! Plugins, `permissions.allow` bleiben unangetastet) und atomar, eingerückt und in der
//! vorhandenen Schlüsselreihenfolge geschrieben: die Datei wird auch von Hand gepflegt.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::{Map, Value};

use crate::paths::{atomic_write, home_dir};
use crate::pyjson::dumps_pretty;
use crate::pystr::{py_str, strip, truthy};

/// `%USERPROFILE%\.claude\settings.json`, dieselbe Datei, in der Hooks und statusLine
/// stehen.
pub fn settings_path() -> PathBuf {
    home_dir().join(".claude").join("settings.json")
}

/// (Anzeige, Wert). Reihenfolge = Dropdown-Reihenfolge; das ERSTE Paar ist zugleich
/// der Rückfall für Unbekanntes. Bewusst nur Aliasse, keine Versionen: `claude --model`
/// löst `opus`/`fable`/`sonnet`/`haiku` immer auf das neueste Modell der Reihe auf.
pub const MODEL_CHOICES: [(&str, &str); 4] =
    [("Opus (1M)", "opus[1m]"), ("Fable", "fable"), ("Sonnet", "sonnet"), ("Haiku", "haiku")];

/// Start-Permission-Modi. Fehlte Bypass, machte das Einstellungsfenster beim nächsten
/// Speichern still „Auto“ daraus.
pub const MODE_CHOICES: [(&str, &str); 5] = [
    ("Auto", "auto"),
    ("Standard", "default"),
    ("Accept Edits", "acceptEdits"),
    ("Plan", "plan"),
    ("Bypass", BYPASS_MODE),
];

/// Bypass = Claude Code fragt nie. Den Warnhinweis beim ersten Start bestätigt der
/// Mensch selbst; `skipDangerousModePermissionPrompt` schreibt das Deck bewusst nicht.
pub const BYPASS_MODE: &str = "bypassPermissions";

pub const LANG_CHOICES: [(&str, &str); 2] = [("Englisch", "english"), ("Deutsch", "german")];

/// Effort-Auswahl. „Ultracode“ ist KEIN effortLevel, sondern xhigh plus der Schalter
/// `ultracode`; die Auswahl bildet also auf zwei Felder ab.
pub const EFFORT_CHOICES: [&str; 6] = ["Ultracode", "max", "xhigh", "high", "medium", "low"];

/// Anzeige → (effortLevel, ultracode). Rückfall: xhigh ohne ultracode.
pub fn effort_spec(label: &str) -> (&'static str, bool) {
    match label {
        "Ultracode" => ("xhigh", true),
        "max" => ("max", false),
        "high" => ("high", false),
        "medium" => ("medium", false),
        "low" => ("low", false),
        _ => ("xhigh", false),
    }
}

/// (effortLevel, ultracode) → Anzeige. `ultracode` gewinnt; nichts passt → `xhigh`.
pub fn effort_label(effort: Option<&str>, ultracode: bool) -> &'static str {
    if ultracode {
        return "Ultracode";
    }
    EFFORT_CHOICES
        .into_iter()
        .find(|l| {
            let (lvl, uc) = effort_spec(l);
            !uc && Some(lvl) == effort
        })
        .unwrap_or("xhigh")
}

/// settings.json als Objekt; fehlt, kaputt oder kein Objekt → leer (nie ein Fehler).
pub fn load(path: &Path) -> Map<String, Value> {
    match crate::paths::load_json(path) {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

/// Atomar und eingerückt schreiben, Umlaute bleiben lesbar (wie `json.dump(indent=2,
/// ensure_ascii=False)`).
pub fn save(data: &Map<String, Value>, path: &Path) -> std::io::Result<()> {
    atomic_write(path, dumps_pretty(&Value::Object(data.clone())).as_bytes())
}

/// Die gesteuerten Werte, roh wie in der Datei (nicht gesetzt → `None`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClaudeValues {
    pub model: Option<Value>,
    pub mode: Option<Value>,
    pub effort: Option<Value>,
    pub ultracode: bool,
    pub language: Option<Value>,
}

fn non_null(v: Option<&Value>) -> Option<Value> {
    v.filter(|v| !v.is_null()).cloned()
}

pub fn read_values(path: &Path) -> ClaudeValues {
    let d = load(path);
    let perms = d.get("permissions").and_then(Value::as_object);
    ClaudeValues {
        model: non_null(d.get("model")),
        mode: non_null(perms.and_then(|p| p.get("defaultMode"))),
        effort: non_null(d.get("effortLevel")),
        ultracode: d.get("ultracode").is_some_and(truthy),
        language: non_null(d.get("language")),
    }
}

/// Was `write_values` setzen soll; `None` = diesen Schlüssel nicht anfassen.
#[derive(Debug, Clone, Default)]
pub struct WriteValues<'a> {
    pub model: Option<&'a str>,
    pub mode: Option<&'a str>,
    pub effort: Option<&'a str>,
    pub ultracode: Option<bool>,
    pub language: Option<&'a str>,
}

/// Nur die gesteuerten Schlüssel in die BESTEHENDE Datei mergen; alles andere bleibt.
/// Gibt das geschriebene Objekt zurück.
pub fn write_values(v: &WriteValues, path: &Path) -> std::io::Result<Map<String, Value>> {
    let mut d = load(path);
    if let Some(m) = v.model {
        d.insert("model".into(), m.into());
    }
    if let Some(e) = v.effort {
        d.insert("effortLevel".into(), e.into());
    }
    if let Some(u) = v.ultracode {
        d.insert("ultracode".into(), u.into());
    }
    if let Some(l) = v.language {
        d.insert("language".into(), l.into());
    }
    if let Some(mode) = v.mode {
        let mut perms = match d.get("permissions") {
            Some(Value::Object(p)) => p.clone(),
            _ => Map::new(),
        };
        perms.insert("defaultMode".into(), mode.into());
        d.insert("permissions".into(), Value::Object(perms));
    }
    save(&d, path)?;
    Ok(d)
}

/// `permissions.defaultMode` für den Panel-Takt: neu gelesen wird nur, wenn sich die
/// Datei geändert hat. Ist sie gerade halb geschrieben (Claude Code und das Deck
/// schreiben beide hinein), gilt der letzte gute Stand weiter, statt für einen Takt auf
/// `None` zu fallen.
#[derive(Debug, Default)]
pub struct DefaultModeCache {
    hits: HashMap<PathBuf, (SystemTime, Option<Value>)>,
}

impl DefaultModeCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&mut self, path: &Path) -> Option<Value> {
        let stamp = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
        let hit = self.hits.get(path);
        if let Some((s, mode)) = hit
            && *s == stamp
        {
            return mode.clone();
        }
        let parsed = std::fs::read(path)
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
            .and_then(|t| serde_json::from_str::<Value>(&t).ok());
        let Some(data) = parsed else {
            return hit.and_then(|(_, m)| m.clone());
        };
        let mode = data
            .get("permissions")
            .and_then(Value::as_object)
            .and_then(|p| p.get("defaultMode"))
            .filter(|v| !v.is_null())
            .cloned();
        self.hits.insert(path.to_path_buf(), (stamp, mode.clone()));
        mode
    }
}

/// Anzeige → Wert (Rückfall: erster Eintrag).
pub fn label_to_value(choices: &[(&str, &'static str)], label: &str) -> &'static str {
    choices.iter().find(|(l, _)| *l == label).map_or(choices[0].1, |(_, v)| *v)
}

/// Wert → Anzeige. Erst exakt, dann (mit `contains`) per Teilstring auf dem Basisnamen
/// vor `[` (`opus` aus `opus[1m]`, auch aus `claude-opus-5`). Nichts passt → erster
/// Eintrag.
pub fn value_to_label<'a>(choices: &[(&'a str, &str)], value: Option<&Value>, contains: bool) -> &'a str {
    if let Some(v) = value.filter(|v| !v.is_null()) {
        let raw = py_str(v);
        let s = strip(&raw);
        if let Some((l, _)) = choices.iter().find(|(_, val)| *val == s) {
            return l;
        }
        if contains && !s.is_empty() {
            let sl = s.to_lowercase();
            for (l, val) in choices {
                let base = val.split('[').next().unwrap_or("").to_lowercase();
                if !base.is_empty() && (sl.contains(&base) || base.contains(&sl)) {
                    return l;
                }
            }
        }
    }
    choices[0].0
}
