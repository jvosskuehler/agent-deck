//! SpeechMike als Sprechtaste für Claude Code: die Record-Taste eines Philips SpeechMike
//! startet und beendet `/voice` im Agentenfenster, das vorn ist.
//!
//! Das Mikro meldet seine Tasten über eine eigene HID-Schnittstelle (Usage Page 0xFFA0,
//! dieselbe, die ORPHEUS über WebHID liest). Ein Tastenbericht ist 9 Byte lang, unter
//! Windows mit vorangestellter Report-ID 0: Byte 1 ist der Befehl, 0x80 heißt
//! „Tastenzustand“, die Bytes 8 und 9 sind die Maske der gedrückten Tasten
//! (little-endian, Record = 0x0100). Jeder Bericht kommt doppelt an (gemessen 13.11.2026),
//! deshalb zählt nur der Wechsel von „los“ zu „gedrückt“.
//!
//! Was die Taste in Claude Code auslöst, legt `~/.claude/keybindings.json` fest:
//! [`VOICE_KEY`] ist dort an `voice:pushToTalk` gebunden, `/voice tap` macht daraus
//! „einmal drücken = Aufnahme an, nochmal = beenden und senden“.
//!
//! Abschalten über `speechmike_voice: false` in den Deck-Einstellungen.

/// Usage Page der Tasten-Schnittstelle.
pub const VENDOR_USAGE_PAGE: u16 = 0xFFA0;

/// Herstellerkennung Philips Speech Processing Solutions.
pub const PHILIPS_VENDOR_ID: u16 = 0x0911;

/// Was das Deck ins Terminal schickt: Alt+K, in Claude Code `meta+k`.
pub const VOICE_KEY: &str = "alt+k";

const BUTTON_EVENT: u8 = 0x80;
const RECORD_BIT: u16 = 0x0100;

/// Längste Diktat-Runde, die das Deck ORPHEUS glaubt. Danach gilt ein vergessener Stopp
/// als verpasst, und der nächste Druck startet neu.
pub const DICTATION_MAX_MS: u64 = 5 * 60_000;

/// Nach dem Stopp schickt ORPHEUS noch den letzten Abschnitt; so lange bleibt die Sperre.
pub const AFTER_STOP_MS: u64 = 30_000;

/// Ist die Funktion eingeschaltet? Fehlt die Einstellung, ja.
pub fn enabled(settings: &serde_json::Map<String, serde_json::Value>) -> bool {
    settings.get("speechmike_voice").and_then(serde_json::Value::as_bool).unwrap_or(true)
}

/// Merkt sich, ob Record gerade gedrückt ist, und meldet nur das Drücken.
#[derive(Debug, Default)]
pub struct RecordButton {
    down: bool,
}

impl RecordButton {
    /// Einen rohen Bericht (mit Report-ID vorn) auswerten. `true` genau dann, wenn Record
    /// eben gedrückt wurde. Andere Berichte (Abheben, Ablegen, Antworten) lassen den
    /// Zustand stehen.
    pub fn pressed(&mut self, report: &[u8]) -> bool {
        let Some(mask) = button_mask(report) else {
            return false;
        };
        let down = mask & RECORD_BIT != 0;
        let edge = down && !self.down;
        self.down = down;
        edge
    }
}

/// Was ORPHEUS gerade mit Record macht, aus Sicht des Decks.
///
/// ORPHEUS liest dieselben Drücke und diktiert im Umschaltbetrieb (ein Druck an, der
/// nächste aus), egal welches Fenster vorn ist. Den diktierten Text fügt es Abschnitt für
/// Abschnitt per simuliertem Strg+V ins aktive Fenster ein. Ist das Claude im Terminal,
/// käme der Text doppelt an; darum sperrt das Deck simulierte Tasten ins Terminal, solange
/// ORPHEUS diktiert und [`AFTER_STOP_MS`] danach. Gilt nur für ORPHEUS' Umschaltbetrieb
/// („Halten zum Aufnehmen“ aus), der bei Jorrit eingestellt ist.
#[derive(Debug, Default)]
pub struct OrpheusDictation {
    since: Option<u64>,
}

/// Ergebnis eines Drucks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Toggle {
    /// Dieser Druck hat ein Diktat gestartet (sonst beendet).
    pub started: bool,
    /// Bis wann (ms, gleiche Uhr wie `now`) simulierte Tasten ins Terminal gesperrt sind.
    pub guard_until: u64,
}

impl OrpheusDictation {
    /// Record wurde zum Zeitpunkt `now` (ms) gedrückt.
    pub fn press(&mut self, now: u64) -> Toggle {
        match self.since.take() {
            Some(since) if now.saturating_sub(since) < DICTATION_MAX_MS => {
                Toggle { started: false, guard_until: now + AFTER_STOP_MS }
            }
            _ => {
                self.since = Some(now);
                Toggle { started: true, guard_until: now + DICTATION_MAX_MS }
            }
        }
    }
}

fn button_mask(report: &[u8]) -> Option<u16> {
    if report.len() < 10 || report[1] != BUTTON_EVENT {
        return None;
    }
    Some(u16::from_le_bytes([report[8], report[9]]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Echte Berichte vom 13.11.2026 (SpeechMike Premium, 0911:0C1C).
    const PICKED_UP: [u8; 10] = [0x00, 0x9e, 0, 0, 0, 0, 0, 0, 0, 0];
    const RECORD_DOWN: [u8; 10] = [0x00, 0x80, 0, 0, 0, 0x24, 0x04, 0, 0, 0x01];
    const RECORD_UP: [u8; 10] = [0x00, 0x80, 0, 0, 0, 0x24, 0x04, 0, 0, 0x00];

    #[test]
    fn a_press_counts_once_although_every_report_arrives_twice() {
        let mut b = RecordButton::default();
        let seen: Vec<bool> = [RECORD_DOWN, RECORD_DOWN, RECORD_UP, RECORD_UP, RECORD_DOWN]
            .iter()
            .map(|r| b.pressed(r))
            .collect();
        assert_eq!(seen, [true, false, false, false, true]);
    }

    #[test]
    fn other_buttons_and_motion_reports_do_not_trigger() {
        let mut b = RecordButton::default();
        let mut play = RECORD_DOWN;
        play[9] = 0x04; // PLAY = 0x0400
        assert!(!b.pressed(&PICKED_UP));
        assert!(!b.pressed(&play));
        assert!(!b.pressed(&[0x00, 0x80]));
    }

    #[test]
    fn record_held_together_with_another_button_still_counts() {
        let mut b = RecordButton::default();
        let mut both = RECORD_DOWN;
        both[8] = 0x02; // F1 dazu
        assert!(b.pressed(&both));
    }

    #[test]
    fn presses_alternate_between_start_and_stop_and_the_lock_outlasts_the_stop() {
        let mut d = OrpheusDictation::default();
        assert_eq!(d.press(1_000), Toggle { started: true, guard_until: 1_000 + DICTATION_MAX_MS });
        assert_eq!(d.press(61_000), Toggle { started: false, guard_until: 61_000 + AFTER_STOP_MS });
        assert!(d.press(70_000).started);
    }

    #[test]
    fn a_missed_stop_does_not_turn_the_next_start_into_a_stop() {
        let mut d = OrpheusDictation::default();
        d.press(0);
        assert!(d.press(DICTATION_MAX_MS + 1).started);
    }

    #[test]
    fn on_unless_switched_off() {
        assert!(enabled(&serde_json::Map::new()));
        assert!(!enabled(json!({"speechmike_voice": false}).as_object().unwrap()));
        assert!(enabled(json!({"speechmike_voice": "kaputt"}).as_object().unwrap()));
    }
}
