//! SpeechMike-Record als Sprechtaste für Claude Code (`deck_core::speechmike`): ein
//! eigener Thread liest die Tasten des Mikros und schickt beim Druck auf Record Alt+K,
//! wenn Windows Terminal vorn ist. Andere Fenster bleiben unberührt, dort gehört die Taste
//! weiter ORPHEUS.
//!
//! ORPHEUS diktiert auf denselben Druck mit und würde seinen Text per Strg+V ins Terminal
//! einfügen. Jeder Druck führt darum auch die ORPHEUS-Runde nach und stellt die Sperre
//! (`deck_win::paste_guard`) entsprechend.
//!
//! Ohne Mikro sucht der Thread alle paar Sekunden neu, Abziehen und Einstecken braucht
//! also keinen Neustart des Decks. Ins Log kommen nur Wechsel: verbunden, verloren,
//! verworfene ORPHEUS-Eingaben je Runde, und ein Fehler beim Senden einmal bis zum
//! nächsten Erfolg.

use std::time::Duration;

use deck_core::speechmike::{OrpheusDictation, PHILIPS_VENDOR_ID, RecordButton, VENDOR_USAGE_PAGE};
use deck_win::paste_guard;
use deck_win::speechmike::{HidReader, send_alt_k, terminal_in_front};

use crate::applog::note;

const RESCAN: Duration = Duration::from_secs(3);

pub fn start() {
    if let Err(e) = paste_guard::start() {
        note(format!("SpeechMike: ORPHEUS-Sperre fehlt, Text kann doppelt ankommen: {e}"));
    }
    std::thread::Builder::new()
        .name("speechmike".into())
        .spawn(run)
        .ok();
}

fn run() {
    let mut announced_missing = false;
    let mut orpheus = OrpheusDictation::default();
    loop {
        let Some(reader) = HidReader::open(PHILIPS_VENDOR_ID, VENDOR_USAGE_PAGE) else {
            if !announced_missing {
                note("SpeechMike: keins angeschlossen, suche weiter");
                announced_missing = true;
            }
            std::thread::sleep(RESCAN);
            continue;
        };
        announced_missing = false;
        note("SpeechMike verbunden, Record = Claude-Sprechtaste");
        let err = listen(&reader, &mut orpheus);
        note(format!("SpeechMike getrennt: {err}"));
        drop(reader);
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// Liest, bis das Gerät verschwindet, und gibt den Grund zurück.
fn listen(reader: &HidReader, orpheus: &mut OrpheusDictation) -> deck_win::Error {
    let mut record = RecordButton::default();
    let mut send_failed = false;
    let mut buf = [0u8; 64];
    loop {
        let n = match reader.read(&mut buf) {
            Ok(n) => n,
            Err(e) => return e,
        };
        if !record.pressed(&buf[..n]) {
            continue;
        }
        let toggle = orpheus.press(paste_guard::now_ms());
        if toggle.started {
            let blocked = paste_guard::take_blocked();
            if blocked > 0 {
                note(format!("SpeechMike: vorige Runde {blocked} ORPHEUS-Tasten im Terminal verworfen"));
            }
        }
        paste_guard::arm_until(toggle.guard_until);
        if !terminal_in_front() {
            continue;
        }
        match send_alt_k() {
            Ok(()) if send_failed => {
                note("SpeechMike: Senden klappt wieder");
                send_failed = false;
            }
            Ok(()) => {}
            Err(e) if !send_failed => {
                note(format!("SpeechMike: Alt+K nicht gesendet: {e}"));
                send_failed = true;
            }
            Err(_) => {}
        }
    }
}
