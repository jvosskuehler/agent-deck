//! Die VS-Code-Extension (Brücke zum Deck): Dateien kopieren und VS Codes Registratur
//! prüfen und reparieren (Python `ops/vscode_ext.py` und der Kopierschritt aus
//! `install.ps1`). Quelle der Extension ist `extension/` im Repo bzw. in den Ressourcen.
//!
//! Der Ordner `~/.vscode/extensions/agent-deck-bridge` GENÜGT NICHT: geladen wird, was in
//! der Registratur `extensions.json` daneben steht. Zeigt deren Eintrag ins Leere (ein
//! umbenannter Ordner), lädt VS Code die Extension gar nicht, während der richtige
//! Ordner unregistriert daneben liegt (2026-07-30). Und die Extension ist mehr als eine
//! Datei: fehlt ein Nebenmodul, wirft `require` beim Aktivieren und es lädt GAR NICHTS.
//!
//! CHIRURGISCH: in der Registratur stehen ALLE Extensions des Nutzers. Sie wird ergänzt,
//! nie neu geschrieben, und eine Datei, die nicht als JSON-Array liest, wird nicht
//! angefasst. Eine Reparatur wirkt erst nach „Developer: Reload Window“, je Fenster.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::fsutil::{atomic_write_with, backup, normcase};
use crate::guard;
use crate::report::{Grade, Report};

/// So nennt VS Code uns (publisher „local“ aus package.json), und so heißt der Ordner.
pub const EXT_ID: &str = "local.agent-deck-bridge";
pub const EXT_DIR: &str = "agent-deck-bridge";
pub const REGISTRY: &str = "extensions.json";

/// `~/.vscode/extensions`.
pub fn extensions_dir() -> PathBuf {
    deck_core::paths::home_dir().join(".vscode").join("extensions")
}

pub fn registry_path(ext_dir: &Path) -> PathBuf {
    ext_dir.join(REGISTRY)
}

/// Die Registratur lesen. `None` = nicht lesbar oder kein JSON-Array, und das ist NICHT
/// dasselbe wie leer: auf `None` wird nie geschrieben, sonst deinstalliert ein Tippfehler
/// in der Datei alle Extensions des Nutzers.
pub fn load(path: &Path) -> Option<Vec<Value>> {
    let raw = std::fs::read(path).ok()?;
    match serde_json::from_slice(&raw).ok()? {
        Value::Array(a) => Some(a),
        _ => None,
    }
}

/// Atomar, im Format von VS Code: eine Zeile, kompakt, UTF-8 ohne BOM, Umlaute unescaped
/// (ein `\u00df` wäre für VS Code ein anderer Pfad).
pub fn save(entries: &[Value], path: &Path) -> std::io::Result<()> {
    let raw = serde_json::to_string(entries).map_err(std::io::Error::other)?;
    atomic_write_with(path, raw.as_bytes(), ".deck-tmp")
}

/// `C:\Users\x\.vscode\extensions\y` → `/c:/Users/x/.vscode/extensions/y`: so schreibt
/// VS Code den Wert für eine lokal hineinkopierte Extension (führender Schrägstrich,
/// KLEINER Laufwerksbuchstabe, Umlaute unescaped).
pub fn uri_path(fs_path: &str) -> String {
    let p = fs_path.replace('\\', "/");
    let b = p.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        return format!("/{}{}", p[..1].to_lowercase(), &p[1..]);
    }
    if p.starts_with('/') { p } else { format!("/{p}") }
}

fn join(ext_dir: &Path, rel: &str) -> String {
    format!("{}\\{rel}", ext_dir.to_string_lossy().trim_end_matches(['\\', '/']))
}

/// Der `location`-Wert; `$mid: 1` markiert eine serialisierte URI.
pub fn location(ext_dir: &Path, rel: &str) -> Value {
    json!({"$mid": 1, "path": uri_path(&join(ext_dir, rel)), "scheme": "file"})
}

fn norm(p: &str) -> String {
    p.replace('\\', "/").to_lowercase()
}

/// Gehört ein Eintrag uns? An der ID UND am Ordnernamen erkannt (auch
/// `agent-deck-bridge.irgendwas`): so wird der Eintrag eines umbenannten Ordners
/// umgebogen, statt einen zweiten daneben zu legen.
pub fn is_ours(entry: &Value) -> bool {
    let Some(e) = entry.as_object() else { return false };
    if e.get("identifier").and_then(|i| i.get("id")).and_then(Value::as_str) == Some(EXT_ID) {
        return true;
    }
    match e.get("relativeLocation").and_then(Value::as_str) {
        Some(rel) => {
            let head = rel.replace('\\', "/");
            let head = head.split('/').next().unwrap_or("");
            head == EXT_DIR || head.starts_with(&format!("{EXT_DIR}."))
        }
        None => false,
    }
}

/// Urteile über die Registratur. Rein bis auf die eine Dateisystem-Frage (`exists`).
pub fn audit(entries: Option<&[Value]>, ext_dir: &Path, exists: &dyn Fn(&Path) -> bool) -> Vec<(Grade, String)> {
    let pkg = |rel: &str| PathBuf::from(join(ext_dir, rel)).join("package.json");
    let installed = exists(&pkg(EXT_DIR));
    let Some(entries) = entries else {
        return vec![(Grade::Warn, format!("{REGISTRY} nicht lesbar (VS Code noch nie gestartet?); ob die Extension geladen wird, ist von hier nicht zu sehen"))];
    };
    let mine: Vec<&Value> = entries.iter().filter(|e| is_ours(e)).collect();
    if mine.is_empty() {
        let text = if installed {
            format!("{EXT_DIR}\\ liegt da, steht aber nicht in {REGISTRY}: VS Code lädt die Extension nicht")
        } else {
            "Extension weder installiert noch registriert".into()
        };
        return vec![(Grade::Fail, text)];
    }
    let mut out = Vec::new();
    if mine.len() > 1 {
        out.push((Grade::Warn, format!("{} Einträge für {EXT_ID}: VS Code nimmt einen davon, und welchen, entscheidet die Reihenfolge", mine.len())));
    }
    for e in mine {
        let Some(rel) = e.get("relativeLocation").and_then(Value::as_str).filter(|r| !r.is_empty()) else {
            out.push((Grade::Fail, "Eintrag ohne relativeLocation".into()));
            continue;
        };
        if !exists(&pkg(rel)) {
            out.push((Grade::Fail, format!("Eintrag zeigt ins Leere ({rel}): VS Code bricht das Laden ab ('Unable to read file ... package.json')")));
        } else if rel != EXT_DIR {
            out.push((Grade::Warn, format!("Eintrag zeigt auf {rel}, erwartet ist {EXT_DIR}")));
        }
        // location und relativeLocation werden beide benutzt; ein Widerspruch ist ein Befund.
        if let Some(got) = e.get("location").and_then(|l| l.get("path")).and_then(Value::as_str)
            && norm(got) != norm(&uri_path(&join(ext_dir, rel)))
        {
            out.push((Grade::Fail, format!("location.path widerspricht relativeLocation ({got})")));
        }
    }
    if out.is_empty() {
        out.push((Grade::Ok, format!("{EXT_ID} ist auf {EXT_DIR}\\ registriert")));
    }
    out
}

/// Genau EINEN korrekten Eintrag hinterlassen. Rein. Fremde Extensions bleiben an ihrem
/// Platz und in ihrer Reihenfolge; unser Eintrag wird ERGÄNZT (was VS Code sonst noch
/// hineingeschrieben hat, bleibt).
pub fn repair(entries: &[Value], ext_dir: &Path, version: Option<&str>) -> (Vec<Value>, Vec<(Grade, String)>) {
    let mut out = Vec::new();
    let mut notes = Vec::new();
    let mut done = false;
    for e in entries {
        if !is_ours(e) {
            out.push(e.clone());
            continue;
        }
        if done {
            notes.push((Grade::Info, "zweiten Eintrag entfernt".to_string()));
            continue;
        }
        done = true;
        let mut fixed: Map<String, Value> = e.as_object().cloned().unwrap_or_default();
        fixed.insert("identifier".into(), json!({"id": EXT_ID}));
        if let Some(v) = version {
            fixed.insert("version".into(), v.into());
        }
        fixed.insert("location".into(), location(ext_dir, EXT_DIR));
        fixed.insert("relativeLocation".into(), EXT_DIR.into());
        let fixed = Value::Object(fixed);
        if &fixed != e {
            let old = e.get("relativeLocation").and_then(Value::as_str).unwrap_or("?");
            notes.push((Grade::Info, format!("Eintrag umgebogen: {old} -> {EXT_DIR}")));
        }
        out.push(fixed);
    }
    if !done {
        out.push(json!({"identifier": {"id": EXT_ID}, "version": version.unwrap_or("0.0.0"),
                        "location": location(ext_dir, EXT_DIR), "relativeLocation": EXT_DIR}));
        notes.push((Grade::Info, format!("Eintrag angelegt: {EXT_ID} -> {EXT_DIR}")));
    }
    (out, notes)
}

/// Unseren Eintrag herausnehmen; fremde bleiben.
pub fn remove(entries: &[Value]) -> (Vec<Value>, Vec<(Grade, String)>) {
    let out: Vec<Value> = entries.iter().filter(|e| !is_ours(e)).cloned().collect();
    let n = entries.len() - out.len();
    let notes = if n > 0 { vec![(Grade::Info, format!("{n} Eintrag aus {REGISTRY} entfernt"))] } else { Vec::new() };
    (out, notes)
}

/// Version aus der package.json: erst die INSTALLIERTE (die beschreibt die Registratur),
/// sonst die der Quelle.
pub fn installed_version(ext_dir: &Path, source: Option<&Path>) -> Option<String> {
    let installed = ext_dir.join(EXT_DIR).join("package.json");
    let from_source = source.map(|s| s.join("package.json"));
    [Some(installed), from_source].into_iter().flatten().find_map(|p| {
        let v: Value = serde_json::from_slice(&std::fs::read(p).ok()?).ok()?;
        v.get("version").and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from)
    })
}

/// Echte Eingriffe in die Registratur des Nutzers sind in Tests gesperrt; Wegwerf-Ordner
/// sind frei.
fn check_target(ext_dir: &Path) -> std::io::Result<()> {
    if normcase(&ext_dir.to_string_lossy()) == normcase(&extensions_dir().to_string_lossy()) {
        guard::check("VS-Code-Extensions des Nutzers ändern")?;
    }
    Ok(())
}

/// Die Registratur auf Stand bringen (Python `vscode_ext.main` ohne Schalter). Schreibt
/// nur bei echter Änderung, mit Sicherung daneben.
pub fn register(ext_dir: &Path, source: Option<&Path>) -> Report {
    let mut r = Report::default();
    let path = registry_path(ext_dir);
    let Some(before) = readable_or_new(&mut r, ext_dir, &path) else { return r };
    if !ext_dir.join(EXT_DIR).join("package.json").is_file() {
        // Registrieren, was nicht da ist, erzeugt genau den Geistereintrag, gegen den
        // dieses Modul geschrieben wurde.
        r.push(Grade::Fail, format!("{EXT_DIR}\\ ist nicht installiert, nichts zu registrieren."));
        return r;
    }
    let (after, notes) = repair(&before, ext_dir, installed_version(ext_dir, source).as_deref());
    write_if_changed(&mut r, ext_dir, &path, &before, &after);
    r.lines.extend(notes.into_iter().map(|(grade, text)| crate::report::Line { grade, text }));
    r
}

/// Registratur lesen; fehlt sie ganz, beginnt sie leer. `None` (mit Befund) = nicht
/// anfassen: da, aber kein lesbares JSON-Array.
pub(crate) fn readable_or_new(r: &mut Report, ext_dir: &Path, path: &Path) -> Option<Vec<Value>> {
    if let Some(v) = load(path) {
        return Some(v);
    }
    if path.exists() {
        r.push(Grade::Fail, format!("{REGISTRY} ist kein lesbares JSON-Array, nicht angefasst. Von Hand nachsehen: {}", path.display()));
        return None;
    }
    if !ext_dir.is_dir() {
        r.push(Grade::Warn, "Kein ~/.vscode/extensions (VS Code nicht installiert?)");
        return None;
    }
    Some(Vec::new())
}

pub(crate) fn write_if_changed(r: &mut Report, ext_dir: &Path, path: &Path, before: &[Value], after: &[Value]) {
    if after == before {
        r.push(Grade::Ok, "Registratur ist schon auf Stand.");
        return;
    }
    if let Err(e) = check_target(ext_dir) {
        r.push(Grade::Fail, e.to_string());
        return;
    }
    r.backup = backup(path).map(|p| p.to_string_lossy().into_owned());
    match save(after, path) {
        Ok(()) => {
            r.changed = true;
            r.push(Grade::Ok, path.to_string_lossy());
        }
        Err(e) => r.push(Grade::Fail, format!("{} nicht geschrieben: {e}", path.display())),
    }
}

#[path = "vscode_ext_files.rs"]
mod files;
pub use files::{FileCheck, check, compare_files, copy_files, install, uninstall};

#[cfg(test)]
#[path = "vscode_ext_tests.rs"]
mod tests;
