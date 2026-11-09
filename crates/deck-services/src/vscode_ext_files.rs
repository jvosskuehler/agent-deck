//! Die Dateien der Extension (Teil von `vscode_ext.rs`): vergleichen, kopieren, und die
//! drei Abläufe Prüfen, Einrichten, Entfernen (wie `install.ps1` Schritt 3).

use std::path::Path;

use super::{EXT_DIR, audit, check_target, load, readable_or_new, register, registry_path, remove, write_if_changed};
use crate::report::{Grade, Line, Report};

/// Ergebnis des Dateivergleichs Quelle ↔ installierter Ordner.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileCheck {
    pub missing: Vec<String>,
    pub differing: Vec<String>,
}

fn source_files(source: &Path) -> Vec<(String, std::path::PathBuf)> {
    let Ok(rd) = std::fs::read_dir(source) else { return Vec::new() };
    let mut out: Vec<_> =
        rd.flatten().filter(|e| e.path().is_file()).map(|e| (e.file_name().to_string_lossy().into_owned(), e.path())).collect();
    out.sort();
    out
}

/// JEDE Datei der Quelle vergleichen, nicht nur `extension.js`: ein fehlendes
/// Nebenmodul ist kein Teilausfall, VS Code lädt dann gar nichts.
pub fn compare_files(source: &Path, installed: &Path) -> FileCheck {
    let mut fc = FileCheck::default();
    for (name, src) in source_files(source) {
        let dst = installed.join(&name);
        match (std::fs::read(&src), std::fs::read(&dst)) {
            (_, Err(_)) => fc.missing.push(name),
            (Ok(a), Ok(b)) if a != b => fc.differing.push(name),
            _ => {}
        }
    }
    fc
}

/// Quelle in den installierten Ordner kopieren; Rückgabe: was sich geändert hat (die
/// Bilanz zählt über ALLE Dateien, sonst hieße es „war schon aktuell“, weil
/// `extension.js` gleich blieb, während daneben ein neues Modul fehlt).
pub fn copy_files(source: &Path, installed: &Path) -> std::io::Result<Vec<String>> {
    if let Some(parent) = installed.parent() {
        check_target(parent)?;
    }
    std::fs::create_dir_all(installed)?;
    let fc = compare_files(source, installed);
    let changed: Vec<String> = fc.missing.into_iter().chain(fc.differing).collect();
    for name in &changed {
        std::fs::copy(source.join(name), installed.join(name))?;
    }
    Ok(changed)
}

/// Prüfen (`install.ps1 -Check`): Dateien gegen die Quelle, dann die Registratur.
pub fn check(ext_dir: &Path, source: &Path) -> Report {
    let mut r = Report::default();
    let installed = ext_dir.join(EXT_DIR);
    if !installed.join("extension.js").is_file() {
        r.push(Grade::Fail, format!("Extension nicht installiert ({})", installed.display()));
    } else {
        let fc = compare_files(source, &installed);
        if !fc.missing.is_empty() {
            r.push(Grade::Fail, format!("Extension unvollständig, es fehlt: {}. Neu einrichten, dann Reload Window.", fc.missing.join(", ")));
        } else if !fc.differing.is_empty() {
            r.push(Grade::Fail, format!("Installierte Extension weicht ab ({}). Neu einrichten, dann Reload Window.", fc.differing.join(", ")));
        } else {
            r.push(Grade::Ok, "Extension-Ordner ist da und aktuell (alle Dateien)");
        }
    }
    let entries = load(&registry_path(ext_dir));
    let findings = audit(entries.as_deref(), ext_dir, &|p| p.is_file());
    r.lines.extend(findings.into_iter().map(|(grade, text)| Line { grade, text }));
    if r.fails() > 0 {
        r.push(Grade::Info, "Geradebiegen: Extension neu einrichten (danach in JEDEM Fenster \"Developer: Reload Window\")");
    }
    r
}

/// Einrichten: erst kopieren, DANN registrieren (umgekehrt entstünde ein Eintrag auf
/// einen Ordner, den es noch nicht gibt, und VS Code bräche beim Laden ab).
pub fn install(ext_dir: &Path, source: &Path) -> Report {
    let mut r = Report::default();
    let installed = ext_dir.join(EXT_DIR);
    match copy_files(source, &installed) {
        Ok(changed) if changed.is_empty() => r.push(Grade::Ok, "Extension war schon aktuell"),
        Ok(changed) => {
            r.changed = true;
            r.push(Grade::Ok, format!("Extension kopiert -> {} ({})", installed.display(), changed.join(", ")));
        }
        Err(e) => {
            r.push(Grade::Fail, format!("Extension nicht kopiert: {e}"));
            return r;
        }
    }
    r.append(register(ext_dir, Some(source)));
    if r.changed {
        r.push(Grade::Info, "In JEDEM offenen VS-Code-Fenster: \"Developer: Reload Window\"");
    }
    r
}

/// Entfernen: unseren Registratur-Eintrag, und mit `delete_folder` auch den Ordner. Ein
/// Eintrag ohne Ordner ist genau der Zustand, gegen den dieses Modul existiert.
pub fn uninstall(ext_dir: &Path, delete_folder: bool) -> Report {
    let mut r = Report::default();
    let path = registry_path(ext_dir);
    if let Some(before) = readable_or_new(&mut r, ext_dir, &path) {
        let (after, notes) = remove(&before);
        write_if_changed(&mut r, ext_dir, &path, &before, &after);
        r.lines.extend(notes.into_iter().map(|(grade, text)| Line { grade, text }));
    }
    let installed = ext_dir.join(EXT_DIR);
    if delete_folder && installed.is_dir() {
        match check_target(ext_dir).and_then(|_| crate::fsutil::force_remove_dir(&installed)) {
            Ok(()) => {
                r.changed = true;
                r.push(Grade::Ok, "Extension entfernt");
            }
            Err(e) => r.push(Grade::Fail, format!("Extension-Ordner nicht entfernt: {e}")),
        }
    }
    r
}
