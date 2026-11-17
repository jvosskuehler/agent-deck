//! Hooks und statusLine in Claudes `settings.json` eintragen, prüfen, entfernen
//! (Python `claude/hook_setup.py`, `main`). Die Regeln selbst (welche Einträge, wie sie
//! aussehen, was ein Befund ist) stehen in `deck_core::claude::hook_setup`; hier sind
//! Lesen, Schreiben, Sicherung und Bilanz.
//!
//! Eine settings.json, die da ist, aber nicht als JSON-Objekt liest, wird nicht
//! angefasst: sie wird auch von Hand gepflegt, ein Zurückschreiben nähme alles andere mit.
//!
//! **Die Infozeile (`statusLine`) des Decks kommt immer hinein, ohne Rückfrage.** Eine
//! fremde wird vorher nach `%LOCALAPPDATA%\claude-agent-deck\statusline.before.json`
//! gesichert ([`crate::statusline_backup`]); `deck-statusline` ruft sie weiter auf und
//! hängt ihre Ausgabe an, `uninstall` legt sie zurück. Nur wer ausdrücklich
//! `--statusline keep` sagt, behält die fremde allein.
//!
//! Welche Vorgaben (Modus, Effort) neu eingetragen wurden, steht danach in
//! `installed.json` ([`crate::installed`]); `uninstall` nimmt sie heraus, solange sie
//! noch den geschriebenen Wert haben.

use std::path::{Path, PathBuf};

use deck_core::claude::hook_setup::{self, MergeOptions};
use deck_core::claude::settings;
use serde_json::{Map, Value};

use crate::fsutil::{backup, normcase, real_path};
use crate::guard;
use crate::installed::Installed;
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
    /// Stehen lassen (nur auf ausdrücklichen Wunsch). Modell, Effort und Kontext-%
    /// fehlen dann auf den Kacheln.
    Keep,
    /// Durch die des Decks ersetzen (Vorgabe); die bisherige wird gesichert, läuft hinter
    /// der des Decks weiter und kommt bei `uninstall` zurück.
    #[default]
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
            r.push(Grade::Fail, format!("Die Claude-Einstellungen ({}) sind beschädigt und bleiben unverändert. Bitte von Hand prüfen.", path.display()));
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
            r.push(Grade::Fail, format!("Die Claude-Einstellungen ({}) ließen sich nicht speichern: {e}", path.display()));
            false
        }
    }
}

fn bin(bin_dir: &Path) -> String {
    bin_dir.to_string_lossy().into_owned()
}

/// Die Vorgaben, die `settings::fill_defaults` eintragen kann, als Punkt-Pfade.
const DEFAULT_KEYS: [&str; 2] = ["permissions.defaultMode", "effortLevel"];

fn get_path<'a>(m: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    match key.split_once('.') {
        Some((outer, inner)) => m.get(outer)?.get(inner),
        None => m.get(key),
    }
}

fn remove_path(m: &mut Map<String, Value>, key: &str) {
    match key.split_once('.') {
        Some((outer, inner)) => {
            let Some(Value::Object(o)) = m.get_mut(outer) else { return };
            o.shift_remove(inner);
            // Ein Behälter, den nur unsere Vorgabe gefüllt hat, geht mit.
            if o.is_empty() {
                m.shift_remove(outer);
            }
        }
        None => {
            m.shift_remove(key);
        }
    }
}

fn is_deck_statusline(m: &Map<String, Value>) -> bool {
    let cmd = m.get("statusLine").and_then(|s| s.get("command")).and_then(Value::as_str);
    cmd.is_some_and(|c| hook_setup::owner(c) == hook_setup::Owner::Deck)
}

/// Die sechs Hooks und die statusLine auf die Programme in `bin_dir` richten (dort liegen
/// `deck-report.exe` und `deck-statusline.exe`). Fremde Hooks bleiben. Eine fremde
/// statusLine kommt bei [`StatusLine::Replace`] (Vorgabe) vorher nach `saved_file`;
/// bei [`StatusLine::Keep`] bleibt sie. Zeigt die statusLine schon auf das Deck, ändert
/// sich an ihr nichts. Fehlen Modus oder Effort, kommen die Vorgaben des Decks hinein
/// (`settings::fill_defaults`) und werden in `m` verzeichnet; was dasteht, bleibt.
///
/// Merker für den Installer: `statusline` (die Infozeile des Decks ist neu da),
/// `statusline-chained` (eine fremde wurde gesichert und läuft dahinter weiter),
/// `default <schlüssel>=<wert>` je neu eingetragener Vorgabe.
pub fn install(settings_file: &Path, bin_dir: &Path, mode: StatusLine, saved_file: &Path, m: &mut Installed) -> Report {
    let mut r = Report::default();
    let Some(before) = read(settings_file, &mut r) else { return r };
    let opts = MergeOptions { force_statusline: mode == StatusLine::Replace };
    let (merged, notes) = hook_setup::merge(&before, &bin(bin_dir), opts);
    let (after, _) = settings::fill_defaults(&merged);
    let mut chained = false;
    if let Some(old) = saved::replaced(&before, &after) {
        // Erst sichern, dann ersetzen: misslingt die Sicherung, bleibt alles, wie es ist.
        if let Err(e) = saved::save(saved_file, old) {
            r.push(Grade::Fail, format!("Deine bisherige Infozeile ließ sich nicht sichern ({}): {e}. Nichts geändert.", saved_file.display()));
            return r;
        }
        r.push(Grade::Info, format!("Bisherige Infozeile gesichert, sie läuft hinter der des Decks weiter: {}", saved_file.display()));
        chained = true;
    }
    if !write(settings_file, &before, after.clone(), &mut r) {
        return r;
    }
    if chained {
        m.statusline_backup = Some(saved_file.to_string_lossy().into_owned());
        r.tag("statusline-chained");
    }
    if !is_deck_statusline(&before) && is_deck_statusline(&after) {
        r.tag("statusline");
    }
    for key in DEFAULT_KEYS {
        if let Some(v) = get_path(&after, key).filter(|v| get_path(&merged, key) != Some(*v)) {
            m.record_default(key, v.clone());
            r.tag(format!("default {key}={}", v.as_str().unwrap_or_default()));
            r.push(Grade::Info, format!("Neue Vorgabe in den Claude-Einstellungen: {key} = {v}"));
        }
    }
    let notes: Vec<(&str, String)> = notes
        .into_iter()
        .map(|(g, t)| {
            if t.contains("anderen Werkzeug") {
                (g, "Die Infozeile eines anderen Werkzeugs bleibt allein stehen (--statusline keep); Modell, Effort und Kontext fehlen dann auf den Kacheln.".to_string())
            } else {
                (g, t)
            }
        })
        .collect();
    r.extend_notes(&notes);
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
/// andere eingetragen wurde. Vorgaben aus `m` gehen nur, solange sie noch den Wert
/// haben, den `install` geschrieben hat; eine geänderte gehört dem Menschen.
pub fn uninstall(settings_file: &Path, saved_file: &Path, m: &mut Installed) -> Report {
    let mut r = Report::default();
    let Some(before) = read(settings_file, &mut r) else { return r };
    let (mut after, notes) = hook_setup::remove(&before);
    r.extend_notes(&notes);
    for d in &m.claude_defaults {
        if get_path(&after, &d.key) == Some(&d.value) {
            remove_path(&mut after, &d.key);
            r.push(Grade::Info, format!("Vorgabe {} entfernt", d.key));
        } else {
            r.push(Grade::Info, format!("{} wurde geändert und bleibt", d.key));
        }
    }
    let restored = saved::restore_into(&mut after, saved_file, &mut r);
    if restored {
        r.tag("statusline-restored");
    }
    let written = write(settings_file, &before, after, &mut r);
    if written {
        m.claude_defaults.clear();
    }
    if restored && written {
        saved::forget(saved_file, &mut r);
        m.statusline_backup = None;
    }
    r
}

#[cfg(test)]
#[path = "install_tests.rs"]
mod tests;
