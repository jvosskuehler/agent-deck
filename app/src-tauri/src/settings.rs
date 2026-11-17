//! Das Einstellungsfenster (`deck/ui/settings_dialog.py`): lesen und sofort schreiben.
//!
//! Zwei Ablagen wie in Python:
//! - `~/.claude/settings.json` für die Vorgaben neuer Agenten (Modus, Effort, Sprache);
//!   die Sprache steuert zugleich die Oberfläche des Decks.
//! - `deck_settings.json` für alles Deck-Eigene: Modell (dort mit `[1m]`, als
//!   `--model` mitgegeben; leer = ohne), Andockkante, Repos-Ordner, Jira-Projekt und die
//!   Schalter (`deck_core::deck_settings`). Repos-Ordner und Jira-Projekt waren vom
//!   09.11. bis 17.11.2026 bewusst nicht im Fenster; seit Kolleginnen und Kollegen das
//!   Deck installieren, gehören sie wieder hinein (docs/DESIGN.md).

use deck_core::auto_compact;
use deck_core::claude::settings::{WriteValues, effort_label, effort_spec, read_values, write_values};
use deck_core::deck_settings::{self as ds, Switch};
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
        // Genau das, was beim Start als `--model` mitgeht: ohne Deck-Modell keins.
        let model = self.chosen_model().unwrap_or_default();
        let s = &self.store.settings;
        let repos = ds::repos_root(s);
        // Ohne Vorgabe in settings.json startet claude im Standardmodus; nur der Schalter
        // `auto_permission_mode` treibt neue Agenten danach in Auto.
        let unset = if ds::on(s, Switch::AutoPermissionMode) { "auto" } else { "default" };
        let mode = cur.mode.as_ref().and_then(Value::as_str).filter(|m| !m.is_empty()).unwrap_or(unset).to_string();
        let effort = effort_label(cur.effort.as_ref().and_then(Value::as_str), cur.ultracode).to_string();
        SettingsView {
            model,
            mode,
            effort,
            language: Lang::normalize(cur.language.as_ref()).setting_value().to_string(),
            dock_edge: self.dock_edge().as_str().to_string(),
            auto_compact: auto_compact::enabled(s),
            dashboard: deck_core::dash::enabled(s),
            repos_root: repos.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
            repos_state: ds::repos_state(repos.as_deref()),
            jira_prefix: ds::jira_prefix(s),
            auto_permission_mode: ds::on(s, Switch::AutoPermissionMode),
            titan: ds::on(s, Switch::Titan),
            store_update: ds::on(s, Switch::StoreUpdate),
            speechmike_voice: ds::on(s, Switch::SpeechmikeVoice),
            pr_automation: ds::on(s, Switch::PrAutomation),
            pr_reviewer: ds::text(s, ds::PR_REVIEWER),
            github_org: ds::text(s, ds::GITHUB_ORG),
            shortcuts: deck_core::shortcuts::rows(s, self.lang),
        }
    }

    pub fn dock_edge(&self) -> Edge {
        Edge::parse(self.store.settings.get("dock_edge").and_then(Value::as_str))
    }

    /// Eine Einstellung sofort speichern. `key` ist der Feldname aus `SettingsView`.
    pub fn set_setting(&mut self, key: &str, value: &Value) -> Result<Effect, String> {
        let text = || value.as_str().map(str::trim).map(String::from).ok_or_else(|| format!("{key}: Text erwartet"));
        let flag = || value.as_bool().ok_or_else(|| format!("{key}: true/false erwartet"));
        let path = self.paths.claude_settings.clone();
        let claude = move |v: WriteValues| write_values(&v, &path).map(|_| ()).map_err(|e| e.to_string());
        let effect = match key {
            "model" => {
                // Leer = „Standard“: Schlüssel weg, neue Agenten starten ohne `--model`.
                self.put_text(ds::MODEL, text()?);
                Effect::None
            }
            "reposRoot" => {
                self.put_text(ds::REPOS_ROOT, text()?);
                // Der alte Schlüssel würde sonst als Rückfall weiter gelten.
                if self.store.settings.remove(ds::LEGACY_HOME).is_some() {
                    self.store.save_settings();
                }
                self.ensure_home_terminal();
                Effect::None
            }
            "jiraPrefix" => {
                let prefix = text()?.to_uppercase();
                if !prefix.chars().all(|c| c.is_ascii_alphanumeric()) {
                    return Err("jiraPrefix: nur Buchstaben und Ziffern".into());
                }
                self.put_text(ds::JIRA_PREFIX, prefix);
                Effect::None
            }
            "prReviewer" => {
                // Mehrere Logins mit Komma; gespeichert wird die bereinigte Liste.
                let list = deck_core::dash::automation::reviewer_list(&text()?);
                self.put_text(ds::PR_REVIEWER, list.join(", "));
                Effect::None
            }
            "githubOrg" => {
                self.put_text(ds::GITHUB_ORG, text()?);
                Effect::None
            }
            "autoPermissionMode" => self.put_switch(Switch::AutoPermissionMode, flag()?),
            "titan" => self.put_switch(Switch::Titan, flag()?),
            "storeUpdate" => self.put_switch(Switch::StoreUpdate, flag()?),
            "speechmikeVoice" => self.put_switch(Switch::SpeechmikeVoice, flag()?),
            "prAutomation" => self.put_switch(Switch::PrAutomation, flag()?),
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
            // Wirkt sofort: `Deck::auto_compact` liest den Schalter in jedem Takt.
            "autoCompact" => self.put_switch(Switch::AutoCompact, flag()?),
            // Wirkt mit dem nächsten Takt: `Deck::view` fragt den Schalter jedes Mal.
            "dashboard" => self.put_switch(Switch::Dashboard, flag()?),
            other => return Err(format!("unbekannte Einstellung: {other}")),
        };
        Ok(effect)
    }

    /// Einen Wert in `deck_settings.json` setzen und sofort speichern.
    pub fn put(&mut self, key: &str, v: Value) {
        self.store.settings.insert(key.to_string(), v);
        self.store.save_settings();
    }

    /// Text setzen; leer nimmt den Schlüssel heraus (dann gilt die Vorgabe).
    fn put_text(&mut self, key: &str, v: String) {
        if v.is_empty() {
            self.store.settings.remove(key);
            self.store.save_settings();
        } else {
            self.put(key, v.into());
        }
    }

    /// Schalter ausdrücklich setzen. Titan, Store-Update und PR-Automatik fragen ihn bei
    /// jeder Gelegenheit selbst, der SpeechMike-Hook erst beim nächsten Start.
    fn put_switch(&mut self, s: Switch, on: bool) -> Effect {
        self.put(s.key(), on.into());
        Effect::None
    }
}
