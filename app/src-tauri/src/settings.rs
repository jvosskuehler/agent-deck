//! Das Einstellungsfenster (`deck/ui/settings_dialog.py`): lesen und sofort schreiben.
//!
//! Zwei Ablagen wie in Python:
//! - `~/.claude/settings.json` für die Vorgaben neuer Agenten (Modus, Effort, Sprache);
//!   die Sprache steuert zugleich die Oberfläche des Decks.
//! - `deck_settings.json` für alles Deck-Eigene: Modell (dort mit `[1m]`, als
//!   `--model` erzwungen) und Andockkante. Jira-Präfix (`jira_prefix`) und Heimatordner
//!   (`terminal_home`) werden dort weiter gelesen, aber nicht mehr im Fenster angeboten.

use deck_core::auto_compact;
use deck_core::claude::settings::{MODEL_CHOICES, WriteValues, effort_label, effort_spec, read_values, write_values};
use deck_core::dock::Edge;
use deck_core::i18n::Lang;
use deck_core::view::SettingsView;
use serde_json::Value;

use crate::deck::Deck;

/// Was eine Änderung über die gespeicherte Datei hinaus auslöst.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    None,
    /// Sprache gewechselt: neue Ansicht senden.
    Language,
    /// Andockkante gewechselt.
    Edge(Edge),
}

impl Deck {
    pub fn settings_view(&self) -> SettingsView {
        let cur = read_values(&self.paths.claude_settings);
        let model = self
            .setting_str("model")
            .or_else(|| cur.model.as_ref().and_then(Value::as_str).filter(|m| !m.is_empty()).map(String::from))
            .unwrap_or_else(|| MODEL_CHOICES[0].1.to_string());
        let mode = cur.mode.as_ref().and_then(Value::as_str).filter(|m| !m.is_empty()).unwrap_or("auto").to_string();
        let effort = effort_label(cur.effort.as_ref().and_then(Value::as_str), cur.ultracode).to_string();
        SettingsView {
            model,
            mode,
            effort,
            language: Lang::normalize(cur.language.as_ref()).setting_value().to_string(),
            dock_edge: self.dock_edge().as_str().to_string(),
            auto_compact: auto_compact::enabled(&self.store.settings),
        }
    }

    pub fn dock_edge(&self) -> Edge {
        Edge::parse(self.store.settings.get("dock_edge").and_then(Value::as_str))
    }

    /// Eine Einstellung sofort speichern. `key` ist der Feldname aus `SettingsView`.
    pub fn set_setting(&mut self, key: &str, value: &Value) -> Result<Effect, String> {
        let text = || value.as_str().map(str::trim).map(String::from).ok_or_else(|| format!("{key}: Text erwartet"));
        let path = self.paths.claude_settings.clone();
        let claude = move |v: WriteValues| write_values(&v, &path).map(|_| ()).map_err(|e| e.to_string());
        let effect = match key {
            "model" => {
                self.put("model", text()?.into());
                Effect::None
            }
            "mode" => {
                claude(WriteValues { mode: Some(&text()?), ..Default::default() })?;
                Effect::None
            }
            "effort" => {
                let (level, ultracode) = effort_spec(&text()?);
                claude(WriteValues { effort: Some(level), ultracode: Some(ultracode), ..Default::default() })?;
                Effect::None
            }
            "language" => {
                let lang = Lang::normalize(Some(&Value::String(text()?)));
                claude(WriteValues { language: Some(lang.setting_value()), ..Default::default() })?;
                self.lang = lang;
                self.services.set_lang(lang);
                Effect::Language
            }
            "dockEdge" => {
                let edge = Edge::parse(Some(&text()?));
                self.put("dock_edge", edge.as_str().into());
                Effect::Edge(edge)
            }
            "autoCompact" => {
                // Wirkt sofort: `Deck::auto_compact` liest den Schalter in jedem Takt.
                let on = value.as_bool().ok_or("autoCompact: true/false erwartet")?;
                self.put(auto_compact::SETTING, on.into());
                Effect::None
            }
            other => return Err(format!("unbekannte Einstellung: {other}")),
        };
        Ok(effect)
    }

    /// Einen Wert in `deck_settings.json` setzen und sofort speichern.
    pub fn put(&mut self, key: &str, v: Value) {
        self.store.settings.insert(key.to_string(), v);
        self.store.save_settings();
    }
}
