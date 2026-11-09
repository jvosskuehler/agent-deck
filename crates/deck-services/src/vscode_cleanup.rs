//! Aufräumen: die frühere VS-Code-Extension des Decks („agent-deck-bridge“) wieder vom
//! Rechner nehmen. Agenten laufen nur noch in Windows Terminal, die Extension hat
//! nichts mehr zu tun. `tools/install.ps1` ruft bei jeder Installation
//! `deck-setup remove-vscode-extension`; ein zweiter Lauf ändert nichts. Das Modul kann
//! in einer späteren Fassung wieder entfallen, wenn kein Rechner die Extension mehr hat.
//!
//! VORSICHT: In `~/.vscode/extensions/extensions.json` stehen ALLE Extensions des
//! Nutzers. Heraus kommt nur unser Eintrag, alles andere bleibt in seiner Reihenfolge.
//! Eine Datei, die nicht als JSON-Array liest, wird nicht angefasst (sonst
//! deinstallierte ein Tippfehler darin alle Extensions), und dann bleibt auch der Ordner
//! liegen: ein Eintrag ohne Ordner ließe VS Code beim Laden stolpern. Vor dem Schreiben
//! kommt eine Sicherung daneben, geschrieben wird atomar.

use std::path::{Path, PathBuf};

use deck_core::paths::atomic_write_with;
use serde_json::Value;

use crate::fsutil::{backup, force_remove_dir, normcase};
use crate::guard;
use crate::report::{Grade, Report};

/// So nannte VS Code die Extension (publisher „local“), und so hieß ihr Ordner.
pub const EXT_ID: &str = "local.agent-deck-bridge";
pub const EXT_DIR: &str = "agent-deck-bridge";
pub const REGISTRY: &str = "extensions.json";

/// `~/.vscode/extensions`.
pub fn extensions_dir() -> PathBuf {
    deck_core::paths::home_dir().join(".vscode").join("extensions")
}

/// Ordnername, wenn er zu uns gehört: `agent-deck-bridge` oder eine umbenannte Fassung
/// `agent-deck-bridge.<irgendwas>`. Nur solche Ordner werden je gelöscht.
fn our_folder(rel: &str) -> Option<&str> {
    let head = rel.split(['/', '\\']).next().unwrap_or("");
    (head == EXT_DIR || head.starts_with(&format!("{EXT_DIR}."))).then_some(head)
}

/// Gehört ein Registratur-Eintrag uns? An der ID ODER am Ordnernamen erkannt.
pub fn is_ours(entry: &Value) -> bool {
    let Some(e) = entry.as_object() else { return false };
    let id = e.get("identifier").and_then(|i| i.get("id")).and_then(Value::as_str);
    let rel = e.get("relativeLocation").and_then(Value::as_str);
    id == Some(EXT_ID) || rel.is_some_and(|r| our_folder(r).is_some())
}

/// Eintrag und Ordner der Extension aus `ext_dir` entfernen. Fehlt beides, ist das ein
/// gutes Ergebnis: dann war schon aufgeräumt.
pub fn remove_extension(ext_dir: &Path) -> Report {
    let mut r = Report::default();
    // Der echte Ordner des Nutzers ist in Tests gesperrt, Wegwerf-Ordner sind frei.
    if normcase(&ext_dir.to_string_lossy()) == normcase(&extensions_dir().to_string_lossy())
        && let Err(e) = guard::check("VS-Code-Extensions des Nutzers ändern")
    {
        r.push(Grade::Fail, e.to_string());
        return r;
    }
    if !ext_dir.is_dir() {
        r.push(Grade::Ok, format!("Kein {}: nichts aufzuräumen", ext_dir.display()));
        return r;
    }
    let Some(mut folders) = remove_entries(&mut r, &ext_dir.join(REGISTRY)) else { return r };
    if !folders.iter().any(|f| f == EXT_DIR) {
        folders.push(EXT_DIR.to_string());
    }
    for name in folders {
        let dir = ext_dir.join(&name);
        if !dir.is_dir() {
            continue;
        }
        match force_remove_dir(&dir) {
            Ok(()) => {
                r.changed = true;
                r.push(Grade::Ok, format!("Extension-Ordner entfernt: {}", dir.display()));
            }
            Err(e) => r.push(Grade::Fail, format!("{} nicht entfernt: {e}", dir.display())),
        }
    }
    if !r.changed {
        r.push(Grade::Ok, "VS-Code-Extension des Decks ist nicht (mehr) installiert");
    }
    r
}

/// Unsere Einträge aus der Registratur nehmen. Rückgabe: die Ordner, auf die sie
/// zeigten, oder `None`, wenn die Registratur nicht angefasst werden darf.
fn remove_entries(r: &mut Report, path: &Path) -> Option<Vec<String>> {
    let Ok(raw) = std::fs::read(path) else { return Some(Vec::new()) };
    let Ok(Value::Array(before)) = serde_json::from_slice::<Value>(&raw) else {
        r.push(Grade::Warn, format!("{REGISTRY} ist kein lesbares JSON-Array, nicht angefasst. Von Hand nachsehen: {}", path.display()));
        return None;
    };
    let (mine, keep): (Vec<Value>, Vec<Value>) = before.into_iter().partition(is_ours);
    if mine.is_empty() {
        return Some(Vec::new());
    }
    let folders = mine
        .iter()
        .filter_map(|e| e.get("relativeLocation").and_then(Value::as_str).and_then(our_folder).map(String::from))
        .collect();
    r.backup = backup(path).map(|p| p.to_string_lossy().into_owned());
    // Wie VS Code selbst: eine Zeile, kompakt, Umlaute unescaped.
    let written = serde_json::to_string(&keep).map_err(std::io::Error::other).and_then(|s| atomic_write_with(path, s.as_bytes(), ".deck-tmp"));
    match written {
        Ok(()) => {
            r.changed = true;
            r.push(Grade::Ok, format!("{} Eintrag aus {} entfernt", mine.len(), path.display()));
            Some(folders)
        }
        Err(e) => {
            r.push(Grade::Fail, format!("{} nicht geschrieben: {e}", path.display()));
            None
        }
    }
}

#[cfg(test)]
#[path = "vscode_cleanup_tests.rs"]
mod tests;
