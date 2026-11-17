//! `deck_settings.json`: Schlüssel, Vorgaben und Versionierung der Deck-Einstellungen.
//!
//! **Warum versioniert.** Bis Nov. 2026 lief das Deck nur bei seinem Autor, und jede
//! Automatik mit Wirkung nach außen war ohne Eintrag an (Titan per ssh, Store-Update per
//! winget, SpeechMike-Tastenhook, neue Agenten in Auto, `opus[1m]` erzwungen). Seit
//! Kolleginnen und Kollegen das Deck installieren, ist all das für eine neue Installation
//! aus. Eine Datei ohne `settings_version` stammt aber von einer bestehenden Installation:
//! dort schreibt [`migrate`] die bisherigen Werte ausdrücklich hinein, damit sich nichts
//! ändert. Danach gilt nur noch, was in der Datei steht, sonst die Vorgabe hier.
//!
//! Gelesen wird tolerant wie überall: ein Schalter mit falschem Typ gilt als fehlend.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use ts_rs::TS;

/// Schlüssel der Version.
pub const VERSION_KEY: &str = "settings_version";
/// Aktuelle Version der Datei.
pub const VERSION: u64 = 1;

/// Ordner, der alle Repos enthält (Heimat-Block, worktree-Aufräumen, Kosten je Ticket).
pub const REPOS_ROOT: &str = "repos_root";
/// Früherer Name von [`REPOS_ROOT`]; wird weiter gelesen, die Migration kopiert ihn.
pub const LEGACY_HOME: &str = "terminal_home";
/// Jira-Projekt („ORPH“); leer = keine Jira-Verknüpfung, Nummern bleiben Nummern.
pub const JIRA_PREFIX: &str = "jira_prefix";
/// Modell neuer Agenten als `--model`; fehlt = Claude Codes eigene Vorgabe.
pub const MODEL: &str = "model";
/// GitHub-Login, den die PR-Automatik als Reviewer setzt (leer = keiner).
pub const PR_REVIEWER: &str = "pr_reviewer";
/// Anzeigenamen der Reviewer, Login → Name (`{"leo-mxyz": "Leo Müller"}`), geschrieben vom
/// Installer und vom Einstellungsfenster, wenn sie Namen aufgelöst haben. Nur zum Zeigen.
pub const PR_REVIEWER_NAMES: &str = "pr_reviewer_names";
/// GitHub-Organisation, auf die sich die PR-Automatik beschränkt (leer = keine).
pub const GITHUB_ORG: &str = "github_org";

/// Modell, das bestehende Installationen bisher erzwungen haben.
pub const LEGACY_MODEL: &str = "opus[1m]";
/// Jira-Projekt, mit dem bestehende Installationen bisher gelaufen sind.
pub const LEGACY_JIRA_PREFIX: &str = "ORPH";
/// Ordnername, den bestehende Installationen fest unter dem Heimatordner annahmen.
pub const LEGACY_REPOS_DIR: &str = "Repos";

/// Ein- und Ausschalter des Decks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Switch {
    /// Titan-Kachel im Info-Fenster samt ssh-Abfrage alle 60 s.
    Titan,
    /// PowerShell und Windows Terminal per winget nachziehen.
    StoreUpdate,
    /// SpeechMike-Record als Sprechtaste, samt globalem Tastatur-Hook (`paste_guard`).
    SpeechmikeVoice,
    /// Neue Agenten per Shift+Tab in den Auto-Modus schalten.
    AutoPermissionMode,
    /// `/compact` kurz vor Ablauf des Prompt-Caches (kostet Kontingent).
    AutoCompact,
    /// Info-Fenster in der freien Fensterkachel (nur lesend).
    Dashboard,
    /// PR-Automatik: mergt, setzt Reviewer und Assignee, startet Hintergrund-Agenten.
    PrAutomation,
}

impl Switch {
    pub const ALL: [Switch; 7] = [
        Switch::Titan,
        Switch::StoreUpdate,
        Switch::SpeechmikeVoice,
        Switch::AutoPermissionMode,
        Switch::AutoCompact,
        Switch::Dashboard,
        Switch::PrAutomation,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Switch::Titan => "titan",
            Switch::StoreUpdate => "store_update",
            Switch::SpeechmikeVoice => "speechmike_voice",
            Switch::AutoPermissionMode => "auto_permission_mode",
            Switch::AutoCompact => "auto_compact",
            Switch::Dashboard => "dashboard",
            Switch::PrAutomation => "pr_automation",
        }
    }

    /// Vorgabe einer neuen Installation: alles mit Wirkung nach außen aus, das nur
    /// lesende Info-Fenster an. Auto-Compact ist auch an (Jorrits Wahl vom 17.11.2026):
    /// es schreibt nur in den eigenen wartenden Agenten und hält dessen Kontext klein.
    pub fn default_on(self) -> bool {
        matches!(self, Switch::Dashboard | Switch::AutoCompact)
    }

    /// Wert, mit dem eine bestehende Installation ohne Eintrag bisher lief: alles an.
    pub fn legacy_on(self) -> bool {
        true
    }
}

/// Ist der Schalter an? Nur ein ausdrücklicher Wahrheitswert zählt, sonst die Vorgabe.
pub fn on(settings: &Map<String, Value>, s: Switch) -> bool {
    settings.get(s.key()).and_then(Value::as_bool).unwrap_or_else(|| s.default_on())
}

/// Ein Textwert, getrimmt; fehlt, falscher Typ oder leer → leer.
pub fn text(settings: &Map<String, Value>, key: &str) -> String {
    settings.get(key).and_then(Value::as_str).map(str::trim).unwrap_or("").to_string()
}

fn some_text(settings: &Map<String, Value>, key: &str) -> Option<String> {
    Some(text(settings, key)).filter(|s| !s.is_empty())
}

/// Jira-Projekt, groß geschrieben; leer = keins.
pub fn jira_prefix(settings: &Map<String, Value>) -> String {
    text(settings, JIRA_PREFIX).to_uppercase()
}

/// Modell neuer Agenten; `None` = ohne `--model` starten.
pub fn model(settings: &Map<String, Value>) -> Option<String> {
    some_text(settings, MODEL)
}

/// Der eingestellte Repos-Ordner ([`REPOS_ROOT`], sonst noch [`LEGACY_HOME`]).
pub fn repos_root(settings: &Map<String, Value>) -> Option<PathBuf> {
    some_text(settings, REPOS_ROOT).or_else(|| some_text(settings, LEGACY_HOME)).map(PathBuf::from)
}

/// Taugt der Repos-Ordner? Das Einstellungsfenster zeigt den Befund.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ReposState {
    /// Ordner da, mit mindestens einem git-Repo darin.
    Ok,
    /// Nichts eingestellt.
    Unset,
    /// Eingestellt, aber kein Ordner.
    Missing,
    /// Ordner da, aber kein Unterordner mit `.git`.
    NoRepos,
}

/// Befund für einen Ordner: hat er einen Unterordner mit `.git` (Ordner oder Datei, ein
/// worktree hat eine Datei)?
pub fn repos_state(root: Option<&Path>) -> ReposState {
    let Some(root) = root else { return ReposState::Unset };
    let Ok(entries) = std::fs::read_dir(root) else { return ReposState::Missing };
    let has_repo = entries.flatten().any(|e| e.path().is_dir() && e.path().join(".git").exists());
    if has_repo { ReposState::Ok } else { ReposState::NoRepos }
}

/// Der Repos-Ordner, nur wenn er taugt ([`ReposState::Ok`]). Sonst gibt es keinen
/// Heimat-Block und nichts, was darin sucht oder aufräumt.
pub fn usable_repos_root(settings: &Map<String, Value>) -> Option<PathBuf> {
    let root = repos_root(settings)?;
    (repos_state(Some(&root)) == ReposState::Ok).then_some(root)
}

/// Woher die geladene Datei stammt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Keine Datei, leer oder `{}`: neue Installation.
    Fresh,
    /// Datei mit Inhalt (auch unlesbarer): eine bestehende Installation.
    Existing,
}

/// Herkunft aus dem rohen Dateiinhalt (`None` = Datei fehlt).
pub fn origin(raw: Option<&[u8]>) -> Origin {
    let Some(raw) = raw else { return Origin::Fresh };
    let text = String::from_utf8_lossy(raw);
    let text = text.trim();
    let empty_object = matches!(serde_json::from_str::<Value>(text), Ok(Value::Object(m)) if m.is_empty());
    if text.is_empty() || empty_object { Origin::Fresh } else { Origin::Existing }
}

/// Auf die aktuelle Version bringen. Mit Version: nichts (auch die Antworten des
/// Installers bleiben, wie sie sind). Ohne Version aus einer bestehenden Installation:
/// jeden fehlenden Schalter mit dem bisherigen Wert eintragen, dazu Modell, Jira-Projekt
/// und Repos-Ordner wie bisher. Dann die Version setzen. `home` = Heimatordner (für den
/// früher fest angenommenen `~/Repos`). Gibt zurück, ob sich etwas geändert hat.
pub fn migrate(settings: &mut Map<String, Value>, origin: Origin, home: &Path) -> bool {
    if settings.contains_key(VERSION_KEY) {
        return false;
    }
    if origin == Origin::Existing {
        for s in Switch::ALL {
            if settings.get(s.key()).and_then(Value::as_bool).is_none() {
                settings.insert(s.key().into(), s.legacy_on().into());
            }
        }
        if model(settings).is_none() {
            settings.insert(MODEL.into(), LEGACY_MODEL.into());
        }
        if jira_prefix(settings).is_empty() {
            settings.insert(JIRA_PREFIX.into(), LEGACY_JIRA_PREFIX.into());
        }
        if some_text(settings, REPOS_ROOT).is_none() {
            let legacy = some_text(settings, LEGACY_HOME)
                .or_else(|| Some(home.join(LEGACY_REPOS_DIR)).filter(|p| p.is_dir()).map(|p| p.to_string_lossy().into_owned()));
            if let Some(root) = legacy {
                settings.insert(REPOS_ROOT.into(), root.into());
            }
        }
    }
    settings.insert(VERSION_KEY.into(), VERSION.into());
    true
}

#[cfg(test)]
#[path = "deck_settings_tests.rs"]
mod tests;
