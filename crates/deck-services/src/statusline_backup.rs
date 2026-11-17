//! Die Infozeile (`statusLine`), die vor dem Deck da war: sichern, bevor `deck-setup
//! install` sie ersetzt, und bei `deck-setup uninstall` zurücklegen. Solange sie hier
//! liegt, ruft `deck-statusline` ihr Kommando weiter auf und hängt die Ausgabe an die
//! eigene Zeile (`deck_hooks::chain`): niemand verliert seine Zeile.
//!
//! Warum eine eigene Datei und nicht die `.bak-…` neben settings.json: die Sicherung
//! der ganzen Datei ist ein Stand von damals; wer sie zurückspielt, verliert alles, was
//! seitdem dazukam. Zurück kommt nur der eine Schlüssel.
//!
//! Format: `{"statusLine": <der alte Wert, unverändert>}` in
//! `%LOCALAPPDATA%\claude-agent-deck\statusline.before.json`.

use std::path::{Path, PathBuf};

use deck_core::claude::hook_setup::{Owner, owner};
use serde_json::{Map, Value, json};

use crate::report::{Grade, Report};

pub const FILE: &str = deck_core::paths::STATUSLINE_BACKUP_FILE;
const KEY: &str = "statusLine";

/// `%LOCALAPPDATA%\claude-agent-deck\statusline.before.json`.
pub fn path() -> PathBuf {
    deck_core::paths::app_dir().join(FILE)
}

fn is_ours(v: &Value) -> bool {
    v.get("command").and_then(Value::as_str).is_some_and(|c| owner(c) == Owner::Deck)
}

/// Die statusLine aus `before`, wenn `after` sie ersetzt und sie nicht dem Deck gehörte.
pub fn replaced<'a>(before: &'a Map<String, Value>, after: &Map<String, Value>) -> Option<&'a Value> {
    let old = before.get(KEY)?;
    (!is_ours(old) && after.get(KEY) != Some(old)).then_some(old)
}

/// Sichern (atomar). Eine ältere Sicherung wird überschrieben: zurück soll, was zuletzt
/// vor dem Deck stand.
pub fn save(file: &Path, old: &Value) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    deck_core::paths::save_json(file, &json!({ (KEY): old }))
}

/// Die gesicherte statusLine in `settings` zurücklegen. `true`, wenn sie eingesetzt
/// wurde. Steht dort schon eine (fremde, inzwischen neu eingetragene), gewinnt die, und
/// die Sicherung bleibt liegen.
pub fn restore_into(settings: &mut Map<String, Value>, file: &Path, r: &mut Report) -> bool {
    if !file.exists() {
        return false;
    }
    let Some(old) = deck_core::paths::load_json(file).and_then(|v| v.get(KEY).cloned()) else {
        r.push(Grade::Warn, format!("Deine frühere Infozeile ließ sich nicht zurückholen ({} ist nicht lesbar).", file.display()));
        return false;
    };
    if settings.contains_key(KEY) {
        r.push(Grade::Warn, format!("Es ist inzwischen eine andere Infozeile eingetragen; deine frühere liegt weiter in {}.", file.display()));
        return false;
    }
    settings.insert(KEY.into(), old);
    r.push(Grade::Info, "Frühere Infozeile wiederhergestellt");
    true
}

/// Sicherung löschen, nachdem sie zurückgelegt ist.
pub fn forget(file: &Path, r: &mut Report) {
    if let Err(e) = std::fs::remove_file(file) {
        r.push(Grade::Warn, format!("{} nicht gelöscht: {e}", file.display()));
    }
}
