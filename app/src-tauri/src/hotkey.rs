//! Die globalen Tasten des Decks, wirksam egal welches Programm vorn ist:
//!
//! - Sprung-Taste (`config::JUMP_HOTKEY`, heute Strg+<): zum Agenten, der am längsten auf
//!   dich wartet (`attention::next_slot`). Überschreibbar über `jump_hotkey`.
//! - Alle nach vorn (`config::FRONT_HOTKEYS`, heute Strg+Shift+< und F20 = Drehregler am
//!   Makropad): jedes Agentenfenster als Raster unter die Maus, nochmal = alle
//!   verstecken und zurück (`deck_core::front`). Überschreibbar über `front_hotkeys` (Text oder Liste).
//!
//! - Glow einstellen (`config::GLOW_HOTKEY`, heute Strg+Alt+G): öffnet das Fenster dafür
//!   (`crate::glow`). Überschreibbar über `glow_hotkey`.
//! - Neuer Agent (`config::NEW_AGENT_HOTKEY`, heute Strg+Alt+N): wie das ＋ im Deck, im
//!   Block des zuletzt benutzten Agenten (`Deck::create_agent_here`). Überschreibbar über
//!   `new_agent_hotkey`.
//!
//! Leer schaltet eine Taste ab. Ist eine Kombination belegt (etwa vom noch laufenden
//! Python-Deck), versucht es ihr Thread eine Weile und gibt dann auf; das Deck läuft ohne
//! sie weiter.

use std::time::Duration;

use deck_win::hotkey::{GlobalHotkey, HotkeyEvent};
use tauri::AppHandle;

use crate::applog::note;
use crate::state::{lock, shared};

/// Sprung-Taste.
pub fn start(app: &AppHandle, combo: String) {
    listen(app, combo, "Sprung-Taste", |app| lock(&shared(app).deck).jump_to_waiting());
}

/// Alle nach vorn, eine Taste je Kombination.
pub fn start_front(app: &AppHandle, combos: Vec<String>) {
    for combo in combos {
        listen(app, combo, "Alle-nach-vorn-Taste", |app| {
            lock(&shared(app).deck).bring_all_front(crate::native::cursor());
        });
    }
}

/// Fenster „Glow einstellen“.
pub fn start_glow(app: &AppHandle, combo: String) {
    listen(app, combo, "Glow-Taste", crate::glow::open);
}

/// Neuer Agent.
pub fn start_new_agent(app: &AppHandle, combo: String) {
    listen(app, combo, "Neuer-Agent-Taste", |app| lock(&shared(app).deck).create_agent_here(crate::now()));
}

fn listen(app: &AppHandle, combo: String, label: &'static str, press: fn(&AppHandle)) {
    if combo.trim().is_empty() {
        return;
    }
    let app = app.clone();
    std::thread::Builder::new()
        .name("hotkey-listen".into())
        .spawn(move || {
            let (_keep, rx) = match GlobalHotkey::start(&combo, 20, Duration::from_millis(500)) {
                Ok(x) => x,
                Err(e) => {
                    note(format!("{label} {combo}: {e}"));
                    return;
                }
            };
            for ev in rx {
                match ev {
                    HotkeyEvent::Registered => note(format!("{label} {combo} aktiv")),
                    HotkeyEvent::Failed => {
                        note(format!("{label} {combo} belegt, ohne Taste weiter"));
                        return;
                    }
                    HotkeyEvent::Pressed => {
                        press(&app);
                        crate::poll::nudge(&app);
                    }
                }
            }
        })
        .ok();
}
