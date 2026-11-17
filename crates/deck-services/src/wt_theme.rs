//! Windows Terminal soll hell und dunkel mit Windows wechseln.
//!
//! Das Profil „Agent Deck“ hat ein helles und ein dunkles Schema (`colorScheme` mit
//! `light`/`dark`). Welches gilt, entscheidet das App-Thema des Terminals, und das ist
//! ohne Eintrag „dark“, nicht „system“ (gemessen 17.11.2026 auf der Test-VM: Windows
//! hell, Agenten trotzdem im Nacht-Schema). Ein Fragment darf nur Profile und Schemata
//! bringen, keine globalen Einstellungen. Darum trägt `install` in die `settings.json`
//! des Terminals `"theme": "system"` ein, aber nur, wenn dort noch kein `theme` steht:
//! eine eigene Wahl des Nutzers bleibt. `remove` nimmt genau diese Zeile wieder heraus,
//! solange sie unverändert ist.
//!
//! Die Datei wird als Text geändert, nicht neu geschrieben: sie darf Kommentare haben,
//! und Reihenfolge und Einrückung des Nutzers bleiben.

use std::path::Path;

use serde_json::Value;

use crate::fsutil::normcase;
use crate::guard;
use crate::installed::Installed;
use crate::report::{Grade, Report};
use crate::wt_fragment::wt_settings_path;

/// Die Zeile, die `install` einfügt; `remove` sucht genau sie.
const LINE: &str = "\"theme\": \"system\",";

/// Oberste Ebene der Datei als JSON; Zeilen, die mit `//` beginnen, zählen nicht. `None`,
/// wenn sie so nicht lesbar ist: dann bleibt sie unberührt.
fn top_level(text: &str) -> Option<serde_json::Map<String, Value>> {
    let plain: String = text.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
    match serde_json::from_str::<Value>(deck_core::paths::strip_bom(&plain)).ok()? {
        Value::Object(m) => Some(m),
        _ => None,
    }
}

/// Text mit `"theme": "system"` direkt nach der ersten `{`, oder `None`, wenn schon ein
/// `theme` steht oder die Datei nicht lesbar ist.
pub fn with_system_theme(text: &str) -> Option<String> {
    let top = top_level(text)?;
    if top.contains_key("theme") {
        return None;
    }
    let brace = text.find('{')?;
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    // Leere Datei `{}`: ohne Komma, sonst wäre das JSON kaputt.
    let empty = top.is_empty();
    let line = if empty { LINE.trim_end_matches(',') } else { LINE };
    Some(format!("{}{nl}    {line}{}", &text[..=brace], &text[brace + 1..]))
}

/// Text ohne die eingefügte Zeile, oder `None`, wenn sie nicht (mehr) so dasteht.
pub fn without_system_theme(text: &str) -> Option<String> {
    let top = top_level(text)?;
    if top.get("theme").and_then(Value::as_str) != Some("system") {
        return None;
    }
    let mut out = Vec::new();
    let mut removed = false;
    for line in text.split_inclusive('\n') {
        let t = line.trim();
        if !removed && (t == LINE || t == LINE.trim_end_matches(',')) {
            removed = true;
            continue;
        }
        out.push(line);
    }
    removed.then(|| out.concat())
}

/// Die echte Datei ist in Tests gesperrt; Wegwerf-Dateien sind frei.
fn check_target(path: &Path) -> std::io::Result<()> {
    if normcase(&path.to_string_lossy()) == normcase(&wt_settings_path().to_string_lossy()) {
        guard::check("Einstellungen von Windows Terminal ändern")?;
    }
    Ok(())
}

/// `"theme": "system"` eintragen, falls der Nutzer kein Thema gewählt hat. Fehlt die
/// Datei (Terminal noch nie gestartet), gibt es nichts zu tun: dann legt das Terminal
/// sie an, und der nächste Lauf des Installers holt es nach.
pub fn install(path: &Path, m: &mut Installed) -> Report {
    let mut r = Report::default();
    let Ok(text) = std::fs::read_to_string(path) else {
        r.push(Grade::Info, format!("{} fehlt: Hell/Dunkel des Terminals bleibt, wie es ist", path.display()));
        return r;
    };
    let Some(new) = with_system_theme(&text) else {
        r.push(Grade::Ok, "Windows Terminal: Hell/Dunkel ist schon eingestellt");
        return r;
    };
    if let Err(e) = check_target(path) {
        r.push(Grade::Fail, e.to_string());
        return r;
    }
    match deck_core::paths::atomic_write(path, new.as_bytes()) {
        Ok(()) => {
            r.changed = true;
            m.wt_theme_added = true;
            r.push(Grade::Ok, "Windows Terminal wechselt jetzt hell und dunkel mit Windows");
        }
        Err(e) => r.push(Grade::Warn, format!("{} nicht geändert: {e}", path.display())),
    }
    r
}

/// Die eingefügte Zeile wieder herausnehmen, wenn `install` sie gesetzt hat und sie noch
/// so dasteht; eine spätere eigene Wahl bleibt.
pub fn remove(path: &Path, m: &mut Installed) -> Report {
    let mut r = Report::default();
    if !std::mem::take(&mut m.wt_theme_added) {
        return r;
    }
    let Some(new) = std::fs::read_to_string(path).ok().and_then(|t| without_system_theme(&t)) else {
        r.push(Grade::Info, "Windows Terminal: Hell/Dunkel wurde inzwischen geändert und bleibt");
        return r;
    };
    if let Err(e) = check_target(path) {
        r.push(Grade::Fail, e.to_string());
        return r;
    }
    match deck_core::paths::atomic_write(path, new.as_bytes()) {
        Ok(()) => {
            r.changed = true;
            r.push(Grade::Ok, "Windows Terminal: Hell/Dunkel wie vor der Installation");
        }
        Err(e) => r.push(Grade::Warn, format!("{} nicht geändert: {e}", path.display())),
    }
    r
}

#[cfg(test)]
#[path = "wt_theme_tests.rs"]
mod tests;
