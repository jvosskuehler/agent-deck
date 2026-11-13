//! Die Tastenkürzel, wie das Fenster „Tastenkürzel“ sie auflistet: die globalen des Decks
//! und Alt+V aus Claude Code (Bild einfügen; Strg+V fügt in Windows Terminal nur Text ein).
//! Tasten ohne Modifier (F13 bis F24) kommen nur vom Makropad und fehlen in der Liste. Es
//! sind dieselben Einstellungen, die `app/src-tauri/src/lib.rs` beim Start liest
//! (`jump_hotkey`, `front_hotkeys`, `glow_hotkey`, `new_agent_hotkey`); eine leere Einstellung schaltet ein
//! Kürzel ab, es fehlt dann auch in der Liste.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use ts_rs::TS;

use crate::config::{FRONT_HOTKEYS, GLOW_HOTKEY, JUMP_HOTKEY, NEW_AGENT_HOTKEY};
use crate::i18n::Lang;

/// Eine Zeile der Liste.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ShortcutRow {
    /// Die Tasten zum Anzeigen, etwa „Strg+Shift+<“.
    pub keys: String,
    /// Was sie tut.
    pub action: String,
}

/// Eine einzelne Kombination aus den Einstellungen: fehlt sie, gilt `default`.
pub fn single(settings: &Map<String, Value>, key: &str, default: &str) -> String {
    match settings.get(key) {
        Some(Value::String(s)) => s.trim().to_string(),
        _ => default.to_string(),
    }
}

/// Alle wirksamen Kürzel in fester Reihenfolge.
pub fn rows(settings: &Map<String, Value>, lang: Lang) -> Vec<ShortcutRow> {
    let mut out = Vec::new();
    let mut add = |combo: &str, de: &str, en: &str| {
        if !combo.trim().is_empty() {
            out.push(ShortcutRow { keys: display(combo), action: lang.l(de, en).to_string() });
        }
    };
    add(&single(settings, "jump_hotkey", JUMP_HOTKEY), "Zum Agenten, der am längsten wartet", "Jump to the agent waiting longest");
    for c in crate::front::combos(settings.get("front_hotkeys"), &FRONT_HOTKEYS) {
        if !c.contains('+') {
            continue;
        }
        add(&c, "Alle Agenten nach vorn, nochmal: zurück", "All agents to the front, again: back");
    }
    add(&single(settings, "new_agent_hotkey", NEW_AGENT_HOTKEY), "Neuer Agent (im Block des zuletzt benutzten)", "New agent (in the block of the last one used)");
    add(&single(settings, "glow_hotkey", GLOW_HOTKEY), "Glow einstellen", "Adjust glow");
    add("alt+v", "Bild aus der Zwischenablage einfügen", "Paste image from clipboard");
    out
}

/// `ctrl+shift+<` → `Strg+Shift+<`, `f20` → `F20`.
pub fn display(combo: &str) -> String {
    combo
        .split('+')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| match p.to_lowercase().as_str() {
            "ctrl" | "strg" => "Strg".to_string(),
            "alt" => "Alt".to_string(),
            "shift" => "Shift".to_string(),
            "win" => "Win".to_string(),
            "space" | "leertaste" => "Leertaste".to_string(),
            "kleiner" => "<".to_string(),
            other => other.to_uppercase(),
        })
        .collect::<Vec<_>>()
        .join("+")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn without_settings_all_default_shortcuts_are_listed() {
        let r = rows(&Map::new(), Lang::German);
        let keys: Vec<&str> = r.iter().map(|x| x.keys.as_str()).collect();
        assert_eq!(keys, ["Strg+<", "Strg+Shift+<", "Strg+Alt+N", "Strg+Alt+G", "Alt+V"], "F20 nur vom Makropad, nicht gelistet");
    }

    #[test]
    fn an_empty_setting_switches_a_shortcut_off_and_a_changed_one_shows_up() {
        let s = json!({"jump_hotkey": "", "glow_hotkey": "win+shift+g", "front_hotkeys": "ctrl+f19", "new_agent_hotkey": ""});
        let keys: Vec<String> = rows(s.as_object().unwrap(), Lang::German).into_iter().map(|x| x.keys).collect();
        assert_eq!(keys, ["Strg+F19", "Win+Shift+G", "Alt+V"]);
    }
}
