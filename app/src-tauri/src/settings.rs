//! Das Einstellungsfenster (`deck/ui/settings_dialog.py`): lesen und sofort schreiben.
//!
//! Zwei Ablagen wie in Python:
//! - `~/.claude/settings.json` für die Vorgaben neuer Agenten (Modus, Effort, Sprache);
//!   die Sprache steuert zugleich die Oberfläche des Decks.
//! - `deck_settings.json` für alles Deck-Eigene: Modell (dort mit `[1m]`, als
//!   `--model` mitgegeben; leer = ohne), Andockkante, Repos-Ordner und die Schalter
//!   (`deck_core::deck_settings`). Der Repos-Ordner war vom 09.11. bis 17.11.2026 bewusst
//!   nicht im Fenster; seit Kolleginnen und Kollegen das Deck installieren, gehört er
//!   wieder hinein (docs/DESIGN.md).
//!
//! Schlüssel, die der Kern nicht kennt, gehen an die Erweiterungen (`extension.rs`); ihre
//! Abschnitte stehen in `SettingsView::extras`.

use deck_core::appearance::{self, Appearance};
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
    /// Hell/Dunkel (oder bei einer Erweiterung der Ort für den Sonnenstand) gewechselt:
    /// Fenster, Rahmen und Sonnenstand neu einstellen (`crate::appearance::apply`).
    Appearance,
}

impl Deck {
    pub fn settings_view(&self) -> SettingsView {
        let cur = read_values(&self.paths.claude_settings);
        // Genau das, was beim Start als `--model` mitgeht: ohne Deck-Modell keins.
        let model = self.chosen_model().unwrap_or_default();
        let s = &self.store.settings;
        let repos = ds::repos_root(s);
        let local = ds::repos_root_local(s);
        // Ohne Vorgabe in settings.json startet claude im Standardmodus; nur der Schalter
        // `auto_permission_mode` treibt neue Agenten danach in Auto.
        let unset = if ds::on(s, Switch::AutoPermissionMode) { "auto" } else { "default" };
        let mode = cur.mode.as_ref().and_then(Value::as_str).filter(|m| !m.is_empty()).unwrap_or(unset).to_string();
        let effort = effort_label(cur.effort.as_ref().and_then(Value::as_str), cur.ultracode).to_string();
        let mut shortcuts = deck_core::shortcuts::rows(s, self.lang);
        // Kürzel einer Erweiterung vor Alt+V, das immer zuletzt steht (Claude Code selbst).
        shortcuts.splice(shortcuts.len().saturating_sub(1)..shortcuts.len().saturating_sub(1), self.ext.shortcuts(s, self.lang));
        let place = crate::appearance::sun_place(self);
        SettingsView {
            model,
            mode,
            effort,
            language: Lang::normalize(cur.language.as_ref()).setting_value().to_string(),
            dock_edge: self.dock_edge().as_str().to_string(),
            auto_compact: auto_compact::enabled(s),
            repos_root: repos.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
            repos_state: ds::repos_state(local.as_deref()),
            agent_env: if ds::text(s, ds::AGENT_ENV) == "wsl" { "wsl".into() } else { "windows".into() },
            wsl_distro: ds::text(s, ds::WSL_DISTRO),
            wsl_distros: self.wsl_distros(),
            auto_permission_mode: ds::on(s, Switch::AutoPermissionMode),
            shortcuts,
            appearance: Appearance::from_settings(s).as_str().to_string(),
            sun_place_name: self.lang.l(place.de, place.en).to_string(),
            extras: self.ext.iter().filter_map(|e| e.settings_section(self)).collect(),
        }
    }

    /// Die Distros zur Wahl, einmal je Lauf gefragt (`wsl.exe -l -v` startet keine Distro).
    fn wsl_distros(&self) -> Vec<String> {
        self.wsl_distros
            .get_or_init(|| deck_services::wsl_cli::distros(&deck_services::wsl_cli::WslExe).into_iter().map(|(n, _)| n).collect())
            .clone()
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
                // Der Ordner-Dialog liefert einen Windows-Pfad; bei Agenten in WSL steht der
                // Linux-Pfad in der Datei (der Agent arbeitet dort, `wsl.exe --cd`).
                let picked = text()?;
                let stored = match deck_core::wsl::AgentEnv::from_settings(&self.store.settings).wsl() {
                    Some(w) if !picked.is_empty() => w.to_linux(&picked).ok_or_else(|| {
                        format!("{picked}: {}", self.lang.l("liegt nicht in WSL", "is not inside WSL"))
                    })?,
                    _ => picked,
                };
                self.put_text(ds::REPOS_ROOT, stored);
                // Der alte Schlüssel würde sonst als Rückfall weiter gelten.
                if self.store.settings.remove(ds::LEGACY_HOME).is_some() {
                    self.store.save_settings();
                }
                self.ensure_home_terminal();
                Effect::None
            }
            "agentEnv" => {
                let v = text()?;
                if v != "wsl" && v != "windows" {
                    return Err(format!("{key}: windows oder wsl"));
                }
                self.put(ds::AGENT_ENV, v.into());
                Effect::None
            }
            "wslDistro" => {
                let d = text()?;
                self.put_text(ds::WSL_DISTRO, d.clone());
                // Home und Einhängepunkt gehören zur Distro: neu ermitteln (beim nächsten
                // Start fragt das Deck sonst selbst, `agent_env::resolve`).
                self.store.settings.remove(ds::WSL_HOME);
                self.store.settings.remove(ds::WSL_MOUNT_ROOT);
                if let Some(w) = (!d.is_empty()).then(|| deck_services::wsl_cli::probe(&deck_services::wsl_cli::WslExe, &d)).flatten() {
                    self.store.settings.insert(ds::WSL_HOME.into(), w.home.into());
                    self.store.settings.insert(ds::WSL_MOUNT_ROOT.into(), w.mount_root.into());
                }
                self.store.save_settings();
                Effect::None
            }
            "autoPermissionMode" => self.put_switch(Switch::AutoPermissionMode, flag()?),
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
            "appearance" => {
                let a = Appearance::parse(&text()?).ok_or_else(|| format!("{key}: system, light, dark oder sun"))?;
                self.put(appearance::KEY, a.as_str().into());
                // Neue Agenten gleich im passenden Schema; offene wechseln beim nächsten Start.
                self.wt.set_color_scheme(a.wt_scheme());
                Effect::Appearance
            }
            "dockEdge" => {
                let edge = Edge::parse(Some(&text()?));
                self.put("dock_edge", edge.as_str().into());
                Effect::Edge(edge)
            }
            // Wirkt sofort: `Deck::auto_compact` liest den Schalter in jedem Takt.
            "autoCompact" => self.put_switch(Switch::AutoCompact, flag()?),
            other => {
                // Nicht vom Kern: vielleicht von einer Erweiterung.
                for e in self.ext.clone().iter() {
                    if let Some(r) = e.set_setting(self, other, value) {
                        return r;
                    }
                }
                return Err(format!("unbekannte Einstellung: {other}"));
            }
        };
        Ok(effect)
    }

    /// Einen Wert in `deck_settings.json` setzen und sofort speichern.
    pub fn put(&mut self, key: &str, v: Value) {
        self.store.settings.insert(key.to_string(), v);
        self.store.save_settings();
    }

    /// Text setzen; leer nimmt den Schlüssel heraus (dann gilt die Vorgabe).
    pub fn put_text(&mut self, key: &str, v: String) {
        if v.is_empty() {
            self.store.settings.remove(key);
            self.store.save_settings();
        } else {
            self.put(key, v.into());
        }
    }

    /// Schalter ausdrücklich setzen; wer ihn braucht, fragt ihn bei jeder Gelegenheit selbst.
    fn put_switch(&mut self, s: Switch, on: bool) -> Effect {
        self.put(s.key(), on.into());
        Effect::None
    }
}
