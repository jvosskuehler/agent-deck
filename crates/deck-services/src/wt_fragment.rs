//! Windows-Terminal-Fragment: das versteckte Profil „Agent Deck“ und die beiden
//! Farbschemata, auf die die Fensterränder des Decks abgestimmt sind.
//!
//! Warum ein Fragment und kein Eintrag in die `settings.json` des Terminals: Fragmente
//! liegen in einem eigenen Ordner, Windows Terminal liest sie dazu, und die Datei des
//! Nutzers bleibt unberührt. Weg damit heißt: die Datei löschen.
//!
//! `%LOCALAPPDATA%\Microsoft\Windows Terminal\Fragments\AgentDeck\agent-deck.json`
//!
//! Das Profil braucht das Deck für `closeOnExit: always` (sonst bleibt nach `/exit`
//! „[process exited …]“ stehen) und für Farben, an die Rand und Nähte passen; Schrift
//! und Shader kommen aus dem Papier-Look ([`crate::look`]). Die
//! Kommandozeile gibt das Deck bei jedem Start selbst mit (`deck_core::wt::wt_argv`).
//!
//! Hat der Nutzer schon ein eigenes Profil „Agent Deck“ in seiner `settings.json` (der
//! Autor des Decks), schreibt `install` kein Fragment: zwei Profile gleichen Namens
//! wären ein Rätsel, und seins geht vor.

use std::path::{Path, PathBuf};

use deck_core::wt::PROFILE;
use serde_json::{Map, Value, json};

use crate::fsutil::normcase;
use crate::guard;
use crate::installed::Installed;
use crate::report::{Grade, Report};

/// Ordnername unter `Fragments`: zugleich die Quelle (`source`), unter der Windows
/// Terminal das Profil führt.
pub const SOURCE: &str = "AgentDeck";
pub const FILE: &str = "agent-deck.json";
pub use deck_core::wt::{SCHEME_DARK, SCHEME_LIGHT};

/// `%LOCALAPPDATA%\Microsoft\Windows Terminal\Fragments\AgentDeck\agent-deck.json`.
pub fn fragment_path() -> PathBuf {
    deck_core::paths::local_app_data().join("Microsoft").join("Windows Terminal").join("Fragments").join(SOURCE).join(FILE)
}

/// Die `settings.json` des Terminals aus dem Store (nur gelesen).
pub fn wt_settings_path() -> PathBuf {
    deck_core::paths::local_app_data()
        .join("Packages")
        .join("Microsoft.WindowsTerminal_8wekyb3d8bbwe")
        .join("LocalState")
        .join("settings.json")
}

/// Kommandozeile der Shell im Profil: PowerShell 7, wenn es sie gibt.
pub fn shell_commandline(pwsh_found: bool) -> &'static str {
    if pwsh_found { "pwsh.exe -NoLogo" } else { "powershell.exe -NoLogo" }
}

/// Das Fragment, genau wie das Profil des Autors: Schrift IBM Plex Mono (installiert
/// `deck-setup look`), Papier-Farben, `shader` macht den blassen Vorschlagstext von
/// Claude im hellen Schema lesbar. `icon`: Pfad zu `claude.exe`, falls bekannt (zeigt
/// sich nur im Tab, den der Fokus-Modus ausblendet); `shader` nur, wenn die Datei da ist,
/// sonst meldet das Terminal einen Fehler.
pub fn fragment(icon: Option<&str>, shell: &str, shader: Option<&str>) -> Value {
    let mut p = Map::new();
    p.insert("closeOnExit".into(), "always".into());
    p.insert("colorScheme".into(), json!({"light": SCHEME_LIGHT, "dark": SCHEME_DARK}));
    p.insert("commandline".into(), shell.into());
    p.insert("font".into(), json!({"face": crate::look::FONT_FACE, "size": 10.5}));
    p.insert("hidden".into(), true.into());
    if let Some(i) = icon.filter(|i| !i.is_empty()) {
        p.insert("icon".into(), i.into());
    }
    p.insert("name".into(), PROFILE.into());
    p.insert("scrollbarState".into(), "hidden".into());
    p.insert("suppressApplicationTitle".into(), true.into());
    p.insert("intenseTextStyle".into(), "bold".into());
    if let Some(sh) = shader.filter(|s| !s.is_empty()) {
        p.insert("experimental.pixelShaderPath".into(), sh.into());
    }
    let profile = Value::Object(p);
    json!({
        "profiles": [profile],
        "schemes": [
            {
                "name": SCHEME_LIGHT,
                "background": "#FAF4ED", "foreground": "#575279", "cursorColor": "#9893A5",
                "selectionBackground": "#DFDAD9",
                "black": "#EEE4DA", "red": "#F7E0E3", "green": "#E2EDDB", "yellow": "#EAA9A5",
                "blue": "#C6DFBA", "purple": "#EEBDC7", "cyan": "#D7827E", "white": "#575279",
                "brightBlack": "#797593", "brightRed": "#B4637A", "brightGreen": "#286983",
                "brightYellow": "#B97A1F", "brightBlue": "#3B8592", "brightPurple": "#8A71A8",
                "brightCyan": "#C26D69", "brightWhite": "#575279"
            },
            {
                "name": SCHEME_DARK,
                "background": "#1D1C1B", "foreground": "#E6E0D6", "cursorColor": "#A39E96",
                "selectionBackground": "#3A3734",
                "black": "#2B2927", "red": "#40272A", "green": "#22372A", "yellow": "#F2C3BF",
                "blue": "#335A3E", "purple": "#66353D", "cyan": "#E39B96", "white": "#E6E0D6",
                "brightBlack": "#8E8A84", "brightRed": "#EE7A8F", "brightGreen": "#8CC7B5",
                "brightYellow": "#E8B86A", "brightBlue": "#8AB8D8", "brightPurple": "#BDA2E0",
                "brightCyan": "#E8A39E", "brightWhite": "#E6E0D6"
            }
        ]
    })
}

/// Steht in der `settings.json` des Terminals ein EIGENES Profil „Agent Deck“? Einträge,
/// die Windows Terminal selbst für unser Fragment anlegt (`source: AgentDeck`), zählen
/// nicht. Die Datei darf Kommentare haben; liest sie nicht als JSON, entscheidet der Text.
pub fn has_own_profile(wt_settings: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(wt_settings) else {
        return wt_settings.contains(&format!("\"{PROFILE}\"")) && !wt_settings.contains(&format!("\"{SOURCE}\""));
    };
    let list = v.get("profiles").and_then(|p| p.get("list").or(Some(p))).and_then(Value::as_array);
    list.into_iter().flatten().any(|p| {
        p.get("name").and_then(Value::as_str) == Some(PROFILE) && p.get("source").and_then(Value::as_str) != Some(SOURCE)
    })
}

/// Die echte Fragment-Datei ist in Tests gesperrt; Wegwerf-Ordner sind frei.
fn check_target(path: &Path) -> std::io::Result<()> {
    if normcase(&path.to_string_lossy()) == normcase(&fragment_path().to_string_lossy()) {
        guard::check("Windows-Terminal-Fragment ändern")?;
    }
    Ok(())
}

/// Fragment schreiben (oder erneuern), außer der Nutzer hat ein eigenes Profil.
/// `profile` ist das fertige Fragment ([`fragment`]).
pub fn install(path: &Path, wt_settings: &Path, profile: &Value, m: &mut Installed) -> Report {
    let mut r = Report::default();
    if let Ok(text) = std::fs::read_to_string(wt_settings)
        && has_own_profile(&text)
    {
        r.push(Grade::Ok, format!("Eigenes Profil „{PROFILE}“ in {} gefunden, kein Fragment nötig", wt_settings.display()));
        return r;
    }
    if let Err(e) = check_target(path) {
        r.push(Grade::Fail, e.to_string());
        return r;
    }
    let want = serde_json::to_vec_pretty(profile).expect("JSON");
    if std::fs::read(path).is_ok_and(|have| have == want) {
        m.wt_fragment = Some(path.to_string_lossy().into_owned());
        r.push(Grade::Ok, format!("Windows-Terminal-Profil „{PROFILE}“ ist auf Stand"));
        return r;
    }
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| deck_core::paths::atomic_write(path, &want));
    match written {
        Ok(()) => {
            r.changed = true;
            m.wt_fragment = Some(path.to_string_lossy().into_owned());
            r.push(Grade::Ok, format!("Windows-Terminal-Profil „{PROFILE}“: {}", path.display()));
        }
        Err(e) => r.push(Grade::Fail, format!("{} nicht geschrieben: {e}", path.display())),
    }
    r
}

/// Fragment und seinen Ordner entfernen; fehlt es, ist das gut. Die Datei gehört dem
/// Deck (eigener Ordner), sie geht immer.
pub fn remove(path: &Path, m: &mut Installed) -> Report {
    let mut r = Report::default();
    m.wt_fragment = None;
    if let Err(e) = check_target(path) {
        r.push(Grade::Fail, e.to_string());
        return r;
    }
    if !path.exists() {
        r.push(Grade::Ok, "Kein Windows-Terminal-Fragment: nichts zu entfernen");
        return r;
    }
    match std::fs::remove_file(path) {
        Ok(()) => {
            r.changed = true;
            r.push(Grade::Ok, format!("Windows-Terminal-Fragment entfernt: {}", path.display()));
            // Der Ordner gehört nur uns; ist er leer, geht er mit.
            if let Some(dir) = path.parent() {
                let _ = std::fs::remove_dir(dir);
            }
        }
        Err(e) => r.push(Grade::Fail, format!("{} nicht entfernt: {e}", path.display())),
    }
    r
}

#[cfg(test)]
#[path = "wt_fragment_tests.rs"]
mod tests;
