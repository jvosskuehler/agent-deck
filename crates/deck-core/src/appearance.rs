//! Hell oder dunkel: die Einstellung `appearance` in `deck_settings.json`.
//!
//! | Wert | Deck und Agentenfenster | Windows |
//! |---|---|---|
//! | `system` | folgen Windows | bleibt, wie es ist |
//! | `light`, `dark` | fest hell bzw. dunkel | bleibt, wie es ist |
//! | `sun` | folgen Windows | das Deck schaltet es bei Sonnenauf- und -untergang um |
//!
//! „Fest“ heißt nur für das Deck selbst: seine Fenster bekommen das Thema vorgegeben
//! (Tauri, `prefers-color-scheme` der Webviews), Rahmen und Glow rechnen damit, und neue
//! Agentenfenster gehen mit dem passenden Schema auf ([`Appearance::wt_scheme`]).
//! Windows und andere Programme bleiben unberührt.
//!
//! Neue Installationen: `system`. Eine bestehende Installation ohne Version (der Autor,
//! der bisher eine geplante Aufgabe „DarkModeZeitplan“ dafür hatte) bekommt `sun`.

use serde_json::{Map, Value};

use crate::wt::{SCHEME_DARK, SCHEME_LIGHT};

/// Schlüssel in `deck_settings.json`.
pub const KEY: &str = "appearance";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
    Sun,
}

impl Appearance {
    pub const ALL: [Appearance; 4] = [Appearance::System, Appearance::Light, Appearance::Dark, Appearance::Sun];

    pub fn as_str(self) -> &'static str {
        match self {
            Appearance::System => "system",
            Appearance::Light => "light",
            Appearance::Dark => "dark",
            Appearance::Sun => "sun",
        }
    }

    /// Genau einer der vier Werte (ohne Groß/klein); sonst `None`.
    pub fn parse(s: &str) -> Option<Appearance> {
        let s = s.trim();
        Appearance::ALL.into_iter().find(|a| a.as_str().eq_ignore_ascii_case(s))
    }

    /// Aus den Einstellungen; fehlt oder unbekannt → `system`.
    pub fn from_settings(settings: &Map<String, Value>) -> Appearance {
        settings.get(KEY).and_then(Value::as_str).and_then(Appearance::parse).unwrap_or_default()
    }

    /// Festes Thema des Decks: `Some(true)` hell, `Some(false)` dunkel, `None` = wie Windows.
    pub fn forced_light(self) -> Option<bool> {
        match self {
            Appearance::Light => Some(true),
            Appearance::Dark => Some(false),
            Appearance::System | Appearance::Sun => None,
        }
    }

    /// Farbschema, mit dem ein neues Agentenfenster aufgeht; `None` = das Paar des Profils,
    /// Windows Terminal wählt nach seinem Thema.
    pub fn wt_scheme(self) -> Option<&'static str> {
        self.forced_light().map(|light| if light { SCHEME_LIGHT } else { SCHEME_DARK })
    }

    /// Schaltet das Deck Windows nach dem Sonnenstand um?
    pub fn follows_sun(self) -> bool {
        self == Appearance::Sun
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn map(v: Value) -> Map<String, Value> {
        match v {
            Value::Object(m) => m,
            _ => panic!("kein Objekt"),
        }
    }

    #[test]
    fn a_missing_or_unknown_value_follows_windows() {
        assert_eq!(Appearance::from_settings(&Map::new()), Appearance::System);
        assert_eq!(Appearance::from_settings(&map(json!({"appearance": "lila"}))), Appearance::System);
        assert_eq!(Appearance::from_settings(&map(json!({"appearance": true}))), Appearance::System);
        assert_eq!(Appearance::from_settings(&map(json!({"appearance": " Dark "}))), Appearance::Dark);
    }

    #[test]
    fn only_light_and_dark_fix_the_deck_and_name_a_terminal_scheme() {
        assert_eq!(Appearance::Light.forced_light(), Some(true));
        assert_eq!(Appearance::Dark.forced_light(), Some(false));
        assert_eq!(Appearance::System.forced_light(), None);
        assert_eq!(Appearance::Sun.forced_light(), None, "Sonnenstand stellt Windows um, das Deck folgt");
        assert_eq!(Appearance::Light.wt_scheme(), Some(SCHEME_LIGHT));
        assert_eq!(Appearance::Dark.wt_scheme(), Some(SCHEME_DARK));
        assert_eq!(Appearance::Sun.wt_scheme(), None);
        assert!(Appearance::Sun.follows_sun() && !Appearance::Dark.follows_sun());
    }

    #[test]
    fn every_value_reads_back_as_itself() {
        for a in Appearance::ALL {
            assert_eq!(Appearance::parse(a.as_str()), Some(a));
        }
    }
}
