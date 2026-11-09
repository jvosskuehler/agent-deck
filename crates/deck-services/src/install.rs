//! Hooks und statusLine in `~/.claude/settings.json` eintragen, prüfen, entfernen
//! (Python `claude/hook_setup.py`, `main`). Die Regeln selbst (welche Einträge, wie sie
//! aussehen, was ein Befund ist) stehen in `deck_core::claude::hook_setup`; hier sind
//! Lesen, Schreiben, Sicherung und Bilanz.
//!
//! Umstellung von der Python-Fassung: beide Melder schreiben dieselben Zustandsdateien.
//! Darum lässt [`install`] die alten Python-Einträge STEHEN; erst wenn der neue Weg
//! bewiesen ist (eine Datei in `state\` wird frisch), nimmt [`remove_legacy`] sie auf
//! ausdrücklichen Aufruf heraus. Nie umgekehrt.
//!
//! Eine settings.json, die da ist, aber nicht als JSON-Objekt liest, wird nicht
//! angefasst: sie wird auch von Hand gepflegt, ein Zurückschreiben nähme alles andere mit.

use std::path::{Path, PathBuf};

use deck_core::claude::hook_setup::{self, MergeOptions};
use deck_core::claude::settings;
use serde_json::{Map, Value};

use crate::fsutil::{backup, normcase};
use crate::guard;
use crate::report::{Grade, Report};
use crate::vscode_install::real_path;

/// `~/.claude/settings.json`.
pub fn settings_path() -> PathBuf {
    settings::settings_path()
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
        guard::check("~/.claude/settings.json ändern")?;
    }
    Ok(())
}

fn write(path: &Path, before: &Map<String, Value>, after: Map<String, Value>, r: &mut Report) {
    if &after == before {
        r.push(Grade::Ok, format!("Nichts zu tun, {} ist schon auf Stand.", path.display()));
        return;
    }
    if let Err(e) = check_target(path) {
        r.push(Grade::Fail, e.to_string());
        return;
    }
    // Sicherung nur bei echter Änderung, sonst sammelt jeder Lauf eine an.
    r.backup = backup(path).map(|p| p.to_string_lossy().into_owned());
    match settings::save(&after, path) {
        Ok(()) => {
            r.changed = true;
            r.push(Grade::Ok, path.to_string_lossy());
        }
        Err(e) => r.push(Grade::Fail, format!("{} nicht geschrieben: {e}", path.display())),
    }
}

fn bin(bin_dir: &Path) -> String {
    bin_dir.to_string_lossy().into_owned()
}

/// Die sechs Hooks und die statusLine auf die Programme in `bin_dir` richten (dort liegen
/// `deck-report.exe` und `deck-statusline.exe`). Fremde Einträge bleiben, Python-Einträge
/// auch. `force_statusline` ersetzt auch eine fremde statusLine.
pub fn install(settings_file: &Path, bin_dir: &Path, force_statusline: bool) -> Report {
    let mut r = Report::default();
    let Some(before) = read(settings_file, &mut r) else { return r };
    let opts = MergeOptions { force_statusline, keep_legacy: true };
    let (after, notes) = hook_setup::merge(&before, &bin(bin_dir), opts);
    write(settings_file, &before, after, &mut r);
    r.extend_notes(&notes);
    r
}

/// Prüfen, was am Exit-Code nicht auffällt: fehlende Einträge, `cmd /c`, fehlendes
/// `|| exit 0`, Pfade ins Leere, Python-Einträge daneben.
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

/// Stehen noch Python-Einträge in der Datei?
pub fn has_legacy(settings_file: &Path) -> bool {
    hook_setup::has_legacy(&settings::load(settings_file))
}

/// Die Python-Einträge herausnehmen. Nur auf ausdrücklichen Aufruf, nachdem der neue
/// Weg bewiesen ist.
pub fn remove_legacy(settings_file: &Path) -> Report {
    let mut r = Report::default();
    let Some(before) = read(settings_file, &mut r) else { return r };
    let (after, notes) = hook_setup::remove_legacy(&before);
    write(settings_file, &before, after, &mut r);
    r.extend_notes(&notes);
    r
}

/// Die Einträge dieser Fassung herausnehmen; alles andere bleibt.
pub fn uninstall(settings_file: &Path) -> Report {
    let mut r = Report::default();
    let Some(before) = read(settings_file, &mut r) else { return r };
    let (after, notes) = hook_setup::remove(&before);
    write(settings_file, &before, after, &mut r);
    r.extend_notes(&notes);
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use deck_core::claude::hook_setup::{HOOKS, Owner, commands_of, owner};
    use serde_json::json;

    fn rig() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let bin = d.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("deck-report.exe"), b"").unwrap();
        std::fs::write(bin.join("deck-statusline.exe"), b"").unwrap();
        let s = d.path().join("settings.json");
        (d, bin, s)
    }

    fn load(p: &Path) -> Map<String, Value> {
        settings::load(p)
    }

    #[test]
    fn a_fresh_file_gets_all_six_hooks_and_the_statusline() {
        let (_d, bin, s) = rig();
        let r = install(&s, &bin, false);
        assert!(r.is_ok() && r.changed, "{r:?}");
        let m = load(&s);
        for (event, _, _) in HOOKS {
            assert_eq!(commands_of(&m, event, Owner::Deck).len(), 1, "{event}");
        }
        assert_eq!(owner(m["statusLine"]["command"].as_str().unwrap()), Owner::Deck);
        assert!(r.backup.is_none(), "neu angelegt: nichts zu sichern");
    }

    #[test]
    fn a_second_run_changes_nothing_and_backs_up_nothing() {
        let (d, bin, s) = rig();
        install(&s, &bin, false);
        let r = install(&s, &bin, false);
        assert!(!r.changed && r.backup.is_none());
        let n = std::fs::read_dir(d.path()).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().contains(".bak-")).count();
        assert_eq!(n, 0);
    }

    fn with_python(s: &Path) {
        let py = |status: &str| json!([{"hooks": [{"type": "command", "command": format!("python \"C:/agent-deck/report.py\" {status} || exit 0")}]}]);
        let mut m = Map::new();
        m.insert("model".into(), "opus[1m]".into());
        m.insert("hooks".into(), json!({"Stop": py("done"), "SessionStart": py("idle")}));
        m.insert("statusLine".into(), json!({"type": "command", "command": "python \"C:/agent-deck/statusline.py\" || exit 0"}));
        settings::save(&m, s).unwrap();
    }

    #[test]
    fn python_entries_stay_by_default_and_go_only_on_explicit_request() {
        let (_d, bin, s) = rig();
        with_python(&s);
        let r = install(&s, &bin, false);
        assert!(r.backup.is_some(), "vorhandene Datei: Sicherung");
        let m = load(&s);
        assert_eq!(commands_of(&m, "Stop", Owner::Legacy).len(), 1, "Python meldet weiter");
        assert_eq!(commands_of(&m, "Stop", Owner::Deck).len(), 1);
        assert_eq!(owner(m["statusLine"]["command"].as_str().unwrap()), Owner::Legacy);
        assert_eq!(m["model"], "opus[1m]", "andere Schlüssel bleiben");
        assert!(has_legacy(&s));
        remove_legacy(&s);
        let m = load(&s);
        assert!(commands_of(&m, "Stop", Owner::Legacy).is_empty());
        assert_eq!(commands_of(&m, "Stop", Owner::Deck).len(), 1);
        assert!(!has_legacy(&s));
    }

    #[test]
    fn an_unreadable_settings_file_is_not_touched() {
        let (_d, bin, s) = rig();
        std::fs::write(&s, "{ kaputt").unwrap();
        let r = install(&s, &bin, false);
        assert_eq!(r.fails(), 1);
        assert_eq!(std::fs::read_to_string(&s).unwrap(), "{ kaputt");
    }

    #[test]
    fn the_check_is_clean_after_install_and_fails_when_the_programs_are_gone() {
        let (_d, bin, s) = rig();
        install(&s, &bin, false);
        let c = check(&s, &bin);
        assert_eq!((c.fails(), c.warns()), (0, 0), "{c:?}");
        std::fs::remove_file(bin.join("deck-report.exe")).unwrap();
        assert_eq!(check(&s, &bin).fails(), HOOKS.len());
    }

    #[test]
    fn uninstall_leaves_python_and_foreign_entries() {
        let (_d, bin, s) = rig();
        with_python(&s);
        install(&s, &bin, false);
        uninstall(&s);
        let m = load(&s);
        assert!(commands_of(&m, "Stop", Owner::Deck).is_empty());
        assert_eq!(commands_of(&m, "Stop", Owner::Legacy).len(), 1);
    }

    #[test]
    fn the_real_settings_file_is_locked_in_tests() {
        assert!(check_target(&settings_path()).is_err());
        assert!(check_target(Path::new(r"C:\tmp\x\settings.json")).is_ok());
    }
}
