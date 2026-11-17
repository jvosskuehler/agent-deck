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

use crate::paths::{atomic_write, claude_dir};
use crate::field::flag;

/// `settings.json` in Claude Codes Konfigurationsordner (`paths::claude_dir`), dieselbe
/// Datei, in der Hooks und statusLine stehen.
pub fn settings_path() -> PathBuf {
    claude_dir().join("settings.json")
}

/// (Anzeige, Wert). Reihenfolge = Dropdown-Reihenfolge; das ERSTE Paar ist zugleich
/// der Rückfall für Unbekanntes. Bewusst nur Aliasse, keine Versionen: `claude --model`
/// löst `opus`/`fable`/`sonnet`/`haiku` immer auf das neueste Modell der Reihe auf.
pub const MODEL_CHOICES: [(&str, &str); 4] =
    [("Opus (1M)", "opus[1m]"), ("Fable", "fable"), ("Sonnet", "sonnet"), ("Haiku", "haiku")];

/// Bypass = Claude Code fragt nie. Den Warnhinweis beim ersten Start bestätigt der
/// Mensch selbst; `skipDangerousModePermissionPrompt` schreibt das Deck bewusst nicht.
pub const BYPASS_MODE: &str = "bypassPermissions";

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

/// Atomar und eingerückt schreiben (zwei Leerzeichen, Umlaute bleiben lesbar); die
/// Schlüssel behalten ihre Reihenfolge (`preserve_order`).
pub fn save(data: &Map<String, Value>, path: &Path) -> std::io::Result<()> {
    atomic_write(path, &serde_json::to_vec_pretty(data)?)
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
        ultracode: flag(d.get("ultracode")),
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

/// Vorgaben einer frischen Installation (Jorrits Wahl vom 17.11.2026): Bypass und Effort
/// high. Das Modell ist `MODEL_CHOICES[0]`, die Sprache ohne Eintrag Deutsch; beides
/// braucht keinen Eintrag.
pub const DEFAULT_MODE: &str = BYPASS_MODE;
pub const DEFAULT_EFFORT: &str = "high";

/// Fehlende Vorgaben (Modus, Effort) in eine settings.json eintragen. Was schon dasteht,
/// auch ein bewusst anderer Wert oder `ultracode`, bleibt: das ist eine Entscheidung des
/// Menschen, kein Loch. Gibt das neue Objekt und je Eintrag eine Notiz zurück.
pub fn fill_defaults(settings: &Map<String, Value>) -> (Map<String, Value>, Vec<(&'static str, String)>) {
    let mut out = settings.clone();
    let mut notes = Vec::new();
    let has_effort = non_null(out.get("effortLevel")).is_some() || flag(out.get("ultracode"));
    if !has_effort {
        out.insert("effortLevel".into(), DEFAULT_EFFORT.into());
        notes.push(("info", format!("Neu eingetragen: Effort {DEFAULT_EFFORT} (änderbar in den Einstellungen des Decks)")));
    }
    let mut perms = match out.get("permissions") {
        Some(Value::Object(p)) => p.clone(),
        _ => Map::new(),
    };
    if non_null(perms.get("defaultMode")).is_none() {
        perms.insert("defaultMode".into(), DEFAULT_MODE.into());
        out.insert("permissions".into(), Value::Object(perms));
        notes.push(("info", format!("Neu eingetragen: Modus {DEFAULT_MODE}, Agenten fragen nicht nach (änderbar in den Einstellungen des Decks)")));
    }
    (out, notes)
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
