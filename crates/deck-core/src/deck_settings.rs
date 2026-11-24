//! `deck_settings.json`: Schlüssel, Vorgaben und Versionierung der Deck-Einstellungen.
//!
//! **Warum versioniert.** Bis Nov. 2026 lief das Deck nur bei seinem Autor, und jede
//! Automatik mit Wirkung nach außen war ohne Eintrag an (neue Agenten in Auto,
//! `opus[1m]` erzwungen). Seit Kolleginnen und Kollegen das Deck installieren, ist all das
//! für eine neue Installation aus. Eine Datei ohne `settings_version` stammt aber von einer
//! bestehenden Installation: dort schreibt [`migrate`] die bisherigen Werte ausdrücklich
//! hinein, damit sich nichts ändert. Danach gilt nur noch, was in der Datei steht, sonst
//! die Vorgabe hier.
//!
//! Gelesen wird tolerant wie überall: ein Schalter mit falschem Typ gilt als fehlend.
//! Schlüssel, die diese Ausgabe nicht kennt (etwa von einer Erweiterung, `extension.rs`
//! der App), bleiben in der Datei stehen und werden nicht angefasst.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use ts_rs::TS;

use crate::appearance::{self, Appearance};

/// Schlüssel der Version.
pub const VERSION_KEY: &str = "settings_version";
/// Aktuelle Version der Datei.
pub const VERSION: u64 = 1;

/// Ordner, der alle Repos enthält (Heimat-Block, Ticket-Prompt).
pub const REPOS_ROOT: &str = "repos_root";
/// Früherer Name von [`REPOS_ROOT`]; wird weiter gelesen, die Migration kopiert ihn.
pub const LEGACY_HOME: &str = "terminal_home";
/// Modell neuer Agenten als `--model`; fehlt = Claude Codes eigene Vorgabe.
pub const MODEL: &str = "model";

/// Wo neue Agenten laufen: `windows` (PowerShell, Vorgabe) oder `wsl` (`deck_core::wsl`).
/// In WSL ist [`REPOS_ROOT`] ein Linux-Pfad.
pub const AGENT_ENV: &str = "agent_env";
/// Die WSL-Distro der Agenten (`Ubuntu`).
pub const WSL_DISTRO: &str = "wsl_distro";
/// `$HOME` in der Distro, ermittelt vom Installer bzw. beim Wählen der Distro.
pub const WSL_HOME: &str = "wsl_home";
/// Einhängepunkt der Windows-Laufwerke in der Distro (`/mnt/`), falls abweichend.
pub const WSL_MOUNT_ROOT: &str = "wsl_mount_root";

/// Modell, das bestehende Installationen bisher erzwungen haben.
pub const LEGACY_MODEL: &str = "opus[1m]";
/// Hell/Dunkel bestehender Installationen: nach dem Sonnenstand, wie die frühere
/// geplante Aufgabe „DarkModeZeitplan“ des Autors.
pub const LEGACY_APPEARANCE: Appearance = Appearance::Sun;
/// Ordnername, den bestehende Installationen fest unter dem Heimatordner annahmen.
pub const LEGACY_REPOS_DIR: &str = "Repos";

/// Ein- und Ausschalter des Decks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Switch {
    /// Neue Agenten per Shift+Tab in den Auto-Modus schalten.
    AutoPermissionMode,
    /// `/compact` kurz vor Ablauf des Prompt-Caches (kostet Kontingent).
    AutoCompact,
}

impl Switch {
    pub const ALL: [Switch; 2] = [Switch::AutoPermissionMode, Switch::AutoCompact];

    pub fn key(self) -> &'static str {
        match self {
            Switch::AutoPermissionMode => "auto_permission_mode",
            Switch::AutoCompact => "auto_compact",
        }
    }

    /// Vorgabe einer neuen Installation: alles mit Wirkung nach außen aus. Auto-Compact
    /// ist an (Wahl vom 17.11.2026): es schreibt nur in den eigenen wartenden Agenten und
    /// hält dessen Kontext klein.
    pub fn default_on(self) -> bool {
        matches!(self, Switch::AutoCompact)
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

/// Modell neuer Agenten; `None` = ohne `--model` starten.
pub fn model(settings: &Map<String, Value>) -> Option<String> {
    some_text(settings, MODEL)
}

/// Der eingestellte Repos-Ordner ([`REPOS_ROOT`], sonst noch [`LEGACY_HOME`]).
pub fn repos_root(settings: &Map<String, Value>) -> Option<PathBuf> {
    some_text(settings, REPOS_ROOT).or_else(|| some_text(settings, LEGACY_HOME)).map(PathBuf::from)
}

/// Der Repos-Ordner so, wie Windows ihn liest: in WSL steht ein Linux-Pfad in der Datei
/// (`/home/dev/repos`), gelesen wird über `\\wsl.localhost\<Distro>\…` (`deck_core::wsl`).
pub fn repos_root_local(settings: &Map<String, Value>) -> Option<PathBuf> {
    let root = repos_root(settings)?;
    Some(PathBuf::from(crate::wsl::AgentEnv::from_settings(settings).to_windows(&root.to_string_lossy())))
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
    let root = repos_root_local(settings)?;
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
/// jeden fehlenden Schalter mit dem bisherigen Wert eintragen, dazu Modell, Hell/Dunkel
/// (Sonnenstand) und Repos-Ordner wie bisher. Dann die Version setzen.
/// `home` = Heimatordner (für den früher fest angenommenen `~/Repos`). Gibt zurück, ob sich etwas geändert hat.
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
        // Der Autor schaltete Windows bisher per geplanter Aufgabe nach dem Sonnenstand
        // um; das übernimmt seitdem das Deck.
        if settings.get(appearance::KEY).and_then(Value::as_str).and_then(Appearance::parse).is_none() {
            settings.insert(appearance::KEY.into(), LEGACY_APPEARANCE.as_str().into());
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
