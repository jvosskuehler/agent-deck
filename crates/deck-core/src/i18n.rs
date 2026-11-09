//! Sprache der Deck-Oberfläche und die Statustexte der Kacheln.
//!
//! Quelle ist DIESELBE Einstellung wie die Antwortsprache der Agenten (`language` in
//! `~/.claude/settings.json`): ein Regler schaltet die Agenten, die Oberfläche und die
//! Hover-Zusammenfassung gemeinsam um. Fehlt sie oder ist unbekannt: Deutsch, die
//! Herkunftssprache des Decks.
//!
//! Jeder sichtbare Text steht zweisprachig direkt am Aufrufort (`lang.l(de, en)`), kein
//! Schlüsselkatalog, keine fehlenden Schlüssel. Anders als in Python gibt es keinen
//! globalen Zwischenspeicher: wer zeichnet, reicht die Sprache herein.

use std::path::Path;

use serde_json::Value;


#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    #[default]
    German,
    English,
}

impl Lang {
    /// Rohwert aus settings.json → Sprache. Alles, was nicht mit „en“ beginnt, ist
    /// Deutsch (auch ein Nicht-Text: Python fiele dort in seinen Fehlerzweig).
    pub fn normalize(value: Option<&Value>) -> Lang {
        match value {
            Some(Value::String(s)) if s.trim().to_lowercase().starts_with("en") => Lang::English,
            _ => Lang::German,
        }
    }

    /// Sprache aus einer settings.json; fehlt oder kaputt → Deutsch.
    pub fn from_settings(path: &Path) -> Lang {
        Lang::normalize(crate::claude::settings::read_values(path).language.as_ref())
    }

    /// Wert, wie settings.json ihn kennt (`german` / `english`).
    pub fn setting_value(self) -> &'static str {
        match self {
            Lang::German => "german",
            Lang::English => "english",
        }
    }

    /// Kürzel für die Oberfläche (`de` / `en`).
    pub fn code(self) -> &'static str {
        match self {
            Lang::German => "de",
            Lang::English => "en",
        }
    }

    pub fn is_english(self) -> bool {
        self == Lang::English
    }

    /// Zweisprachiger Inline-Text.
    pub fn l<'a>(self, de: &'a str, en: &'a str) -> &'a str {
        if self.is_english() { en } else { de }
    }
}

/// Anzeigetext eines Kachel-Status (Python `ui/theme.STATUS_LABEL`). `running` und
/// `thinking` teilen sich „denkt“, `done` ist die ungelesene Antwort, `unknown` das
/// ehrliche Eingeständnis eines Remote-Fensters, `lost` der im Panel errechnete
/// Verbindungsverlust. Unbekanntes gilt als idle.
pub fn status_label(lang: Lang, status: &str) -> &'static str {
    let (de, en) = match status {
        "done" => ("ungelesen", "unread"),
        "thinking" | "running" => ("denkt", "thinking"),
        "waiting" => ("Rückfrage", "needs input"),
        "none" => ("—", "—"),
        "unknown" => ("unbekannt", "unknown"),
        "lost" => ("getrennt", "disconnected"),
        _ => ("idle", "idle"),
    };
    lang.l(de, en)
}
