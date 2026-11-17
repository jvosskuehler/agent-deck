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
//! „[process exited …]“ stehen) und für Farben, an die Rand und Nähte passen. Die
//! Kommandozeile gibt das Deck bei jedem Start selbst mit (`deck_core::wt::wt_argv`).
//!
//! Hat der Nutzer schon ein eigenes Profil „Agent Deck“ in seiner `settings.json` (der
//! Autor des Decks), schreibt `install` kein Fragment: zwei Profile gleichen Namens
//! wären ein Rätsel, und seins geht vor.

use std::path::{Path, PathBuf};

use deck_core::wt::PROFILE;
use serde_json::{Value, json};

use crate::fsutil::normcase;
use crate::guard;
use crate::report::{Grade, Report};

/// Ordnername unter `Fragments`: zugleich die Quelle (`source`), unter der Windows
/// Terminal das Profil führt.
pub const SOURCE: &str = "AgentDeck";
pub const FILE: &str = "agent-deck.json";
pub const SCHEME_LIGHT: &str = "papier-ebenen";
pub const SCHEME_DARK: &str = "papier-ebenen-nacht";

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

/// Das Fragment. `icon`: Pfad zu `claude.exe`, falls bekannt (das Profil-Symbol zeigt
/// sich nur im Tab, den der Fokus-Modus ausblendet; ohne ist auch gut).
pub fn fragment(icon: Option<&str>) -> Value {
    let mut profile = json!({
        "name": PROFILE,
        "hidden": true,
        "closeOnExit": "always",
        "commandline": "powershell.exe -NoLogo",
        "colorScheme": {"light": SCHEME_LIGHT, "dark": SCHEME_DARK},
        "font": {"size": 10.5},
        "scrollbarState": "hidden",
        "suppressApplicationTitle": true,
        "intenseTextStyle": "bold",
    });
    if let Some(i) = icon.filter(|i| !i.is_empty()) {
        profile["icon"] = i.into();
    }
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
pub fn install(path: &Path, wt_settings: &Path, icon: Option<&str>) -> Report {
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
    let want = serde_json::to_vec_pretty(&fragment(icon)).expect("JSON");
    if std::fs::read(path).is_ok_and(|have| have == want) {
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
            r.push(Grade::Ok, format!("Windows-Terminal-Profil „{PROFILE}“: {}", path.display()));
        }
        Err(e) => r.push(Grade::Fail, format!("{} nicht geschrieben: {e}", path.display())),
    }
    r
}

/// Fragment und seinen Ordner entfernen; fehlt es, ist das gut.
pub fn remove(path: &Path) -> Report {
    let mut r = Report::default();
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
