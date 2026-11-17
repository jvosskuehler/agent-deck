//! Hooks und statusLine in Claudes `settings.json` eintragen, prüfen, entfernen
//! (Python `claude/hook_setup.py`, `main`). Die Regeln selbst (welche Einträge, wie sie
//! aussehen, was ein Befund ist) stehen in `deck_core::claude::hook_setup`; hier sind
//! Lesen, Schreiben, Sicherung und Bilanz.
//!
//! Eine settings.json, die da ist, aber nicht als JSON-Objekt liest, wird nicht
//! angefasst: sie wird auch von Hand gepflegt, ein Zurückschreiben nähme alles andere mit.
//!
//! **Eine fremde statusLine bleibt, solange niemand `--statusline replace` sagt.** Wer
//! sie ersetzen lässt, bekommt sie bei `uninstall` zurück: vor dem Ersetzen landet sie in
//! `%LOCALAPPDATA%\claude-agent-deck\statusline.before.json` ([`crate::statusline_backup`]).

use std::path::{Path, PathBuf};

use deck_core::claude::hook_setup::{self, MergeOptions};
use deck_core::claude::settings;
use serde_json::{Map, Value};

use crate::fsutil::{backup, normcase, real_path};
use crate::guard;
use crate::report::{Grade, Report};
use crate::statusline_backup as saved;

/// `settings.json` in Claudes Konfigurationsordner, so wie `deck_core::claude::settings`
/// ihn bestimmt (`CLAUDE_CONFIG_DIR`, sonst `~/.claude`).
pub fn settings_path() -> PathBuf {
    settings::settings_path()
}

/// Was mit einer statusLine geschieht, die nicht dem Deck gehört.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StatusLine {
    /// Stehen lassen (Vorgabe). Modell, Effort und Kontext-% fehlen dann auf den Kacheln.
    #[default]
    Keep,
    /// Durch die des Decks ersetzen; die bisherige wird gesichert und bei `uninstall`
    /// zurückgelegt.
    Replace,
}

impl StatusLine {
    pub fn parse(s: &str) -> Option<StatusLine> {
        match s {
            "keep" => Some(StatusLine::Keep),
            "replace" => Some(StatusLine::Replace),
            _ => None,
        }
    }
}

/// Gelesen: Inhalt, oder `None` mit Befund (da, aber nicht lesbar).
fn read(path: &Path, r: &mut Report) -> Option<Map<String, Value>> {
    if !path.exists() {
        return Some(Map::new());
    }
    match deck_core::paths::load_json(path) {
        Some(Value::Object(m)) => Some(m),
        _ => {
            r.push(Grade::Fail, format!("{} ist kein lesbares JSON-Objekt, nicht angefasst. Von Hand nachsehen.", path.display()));
            None
        }
    }
}

/// Die echte settings.json ist in Tests gesperrt; Wegwerf-Dateien sind frei.
fn check_target(path: &Path) -> std::io::Result<()> {
    if normcase(&path.to_string_lossy()) == normcase(&settings_path().to_string_lossy()) {
        guard::check("Claudes settings.json ändern")?;
    }
    Ok(())
}

/// Schreiben, wenn sich etwas ändert. `true`, wenn die Datei danach `after` enthält.
fn write(path: &Path, before: &Map<String, Value>, after: Map<String, Value>, r: &mut Report) -> bool {
    if &after == before {
        r.push(Grade::Ok, format!("Nichts zu tun, {} ist schon auf Stand.", path.display()));
        return true;
    }
    if let Err(e) = check_target(path) {
        r.push(Grade::Fail, e.to_string());
        return false;
    }
    // Sicherung nur bei echter Änderung, sonst sammelt jeder Lauf eine an.
    r.backup = backup(path).map(|p| p.to_string_lossy().into_owned());
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match settings::save(&after, path) {
        Ok(()) => {
            r.changed = true;
            r.push(Grade::Ok, path.to_string_lossy());
            true
        }
        Err(e) => {
            r.push(Grade::Fail, format!("{} nicht geschrieben: {e}", path.display()));
            false
        }
    }
}

fn bin(bin_dir: &Path) -> String {
    bin_dir.to_string_lossy().into_owned()
}

/// Die sechs Hooks und die statusLine auf die Programme in `bin_dir` richten (dort liegen
/// `deck-report.exe` und `deck-statusline.exe`). Fremde Hooks bleiben. Eine fremde
/// statusLine bleibt bei [`StatusLine::Keep`]; bei [`StatusLine::Replace`] kommt sie
/// vorher nach `saved_file`, und `uninstall` legt sie zurück. Zeigt die statusLine schon
/// auf das Deck, ändert sich an ihr nichts. Fehlen Modus oder Effort, kommen die
/// Vorgaben des Decks hinein (`settings::fill_defaults`); was dasteht, bleibt.
pub fn install(settings_file: &Path, bin_dir: &Path, mode: StatusLine, saved_file: &Path) -> Report {
    let mut r = Report::default();
    let Some(before) = read(settings_file, &mut r) else { return r };
    let opts = MergeOptions { force_statusline: mode == StatusLine::Replace };
    let (merged, notes) = hook_setup::merge(&before, &bin(bin_dir), opts);
    let (after, defaults) = settings::fill_defaults(&merged);
    if let Some(old) = saved::replaced(&before, &after) {
        // Erst sichern, dann ersetzen: misslingt die Sicherung, bleibt alles, wie es ist.
        if let Err(e) = saved::save(saved_file, old) {
            r.push(Grade::Fail, format!("Bisherige statusLine nicht gesichert ({}): {e}. Nichts geändert.", saved_file.display()));
            return r;
        }
        r.push(Grade::Info, format!("Bisherige statusLine gesichert: {}", saved_file.display()));
    }
    write(settings_file, &before, after, &mut r);
    // deck-core nennt den Schalter in seinem Hinweis `--force`; deck-setup heißt er so.
    let notes: Vec<(&str, String)> = notes.into_iter().map(|(g, t)| (g, t.replace("--force", "--statusline replace"))).collect();
    r.extend_notes(&notes);
    r.extend_notes(&defaults);
    r
}

/// Prüfen, was am Exit-Code nicht auffällt: fehlende Einträge, `cmd /c`, fehlendes
/// `|| exit 0`, Pfade ins Leere.
pub fn check(settings_file: &Path, bin_dir: &Path) -> Report {
    let mut r = Report::default();
    let Some(s) = read(settings_file, &mut r) else { return r };
    let exists = |p: &str| Path::new(p).exists();
    // Kurznamen (`JORRIT~1`) auflösen: Text allein macht sie nicht gleich.
    let resolve = |p: &str| real_path(Path::new(p)).map_or_else(|| p.to_string(), |r| r.to_string_lossy().into_owned());
    let findings = hook_setup::audit(&s, &bin(bin_dir), &exists, &resolve);
    r.extend_notes(&findings);
    r
}

/// Die Einträge dieser Fassung herausnehmen; alles andere bleibt. Hat `install` eine
/// fremde statusLine ersetzt, kommt sie aus `saved_file` zurück, sofern inzwischen keine
/// andere eingetragen wurde.
pub fn uninstall(settings_file: &Path, saved_file: &Path) -> Report {
    let mut r = Report::default();
    let Some(before) = read(settings_file, &mut r) else { return r };
    let (mut after, notes) = hook_setup::remove(&before);
    r.extend_notes(&notes);
    let restored = saved::restore_into(&mut after, saved_file, &mut r);
    let written = write(settings_file, &before, after, &mut r);
    if restored && written {
        saved::forget(saved_file, &mut r);
    }
    r
}

#[cfg(test)]
#[path = "install_tests.rs"]
mod tests;
