//! Der Takt: alle 400 ms das Deck einmal durchrechnen (`Deck::tick`) und die Ansicht
//! nur bei Änderung als `deck://view` schicken. Eine Aktion (Klick, Menü) stößt den
//! nächsten Takt sofort an (`nudge`), damit die Kachel nicht bis zu 400 ms nachhinkt.
//!
//! Regel für alle Hintergrund-Threads: nie eine blockierende Tauri-Abfrage stellen,
//! während ein Mutex des Decks gehalten wird; der Hauptthread wartet nie auf unsere
//! Mutexe (alle Befehle laufen asynchron), so kann sich nichts im Kreis blockieren.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use deck_core::config::POLL_MS;
use tauri::{AppHandle, Emitter};

use deck_services::heartbeat::Beater;

use crate::state::{lock, shared};
use crate::tooltip;
use crate::ui_windows::{SETTINGS, TICKET, get};

static NUDGE: OnceLock<Mutex<Sender<()>>> = OnceLock::new();

/// Den nächsten Takt sofort auslösen.
pub fn nudge(_app: &AppHandle) {
    wake();
}

/// Dasselbe ohne App-Handle (für die Rückrufe der Dienste).
pub fn wake() {
    if let Some(tx) = NUDGE.get() {
        let _ = lock(tx).send(());
    }
}

/// Hat ein Dialog des Decks den Fokus? Dann fokussiert das Deck keinen neuen Agenten.
fn modal(app: &AppHandle) -> bool {
    let visible = |label| get(app, label).is_some_and(|w| w.is_visible().unwrap_or(false));
    let focused = |label| get(app, label).is_some_and(|w| w.is_focused().unwrap_or(false));
    visible(TICKET) || (visible(SETTINGS) && focused(SETTINGS))
}

/// Einen Takt rechnen und senden. Auch direkt aufrufbar (Start, nach Befehlen).
pub fn tick(app: &AppHandle, beater: &mut Beater) {
    let s = shared(app);
    let modal = modal(app);
    let now = crate::now();
    let (view, app_dir) = {
        let mut d = lock(&s.deck);
        d.modal = modal;
        let v = d.tick(now);
        tooltip::refresh(app, &d);
        (v, d.paths.app_dir.clone())
    };
    let changed = {
        let mut last = lock(&s.last_view);
        if last.as_ref() == Some(&view) {
            false
        } else {
            *last = Some(view.clone());
            true
        }
    };
    if changed {
        let _ = app.emit("deck://view", &view);
    }
    // Lebenszeichen für den Wächter (deck-watchdog): bezeugt, dass der Takt läuft.
    beater.tick(&app_dir, now);
}

pub fn start(app: &AppHandle) {
    let mut beater = Beater::default();
    let (tx, rx): (Sender<()>, Receiver<()>) = channel();
    let _ = NUDGE.set(Mutex::new(tx));
    let app = app.clone();
    std::thread::Builder::new()
        .name("poll".into())
        .spawn(move || {
            loop {
                tick(&app, &mut beater);
                // Mehrere Anstöße auf einmal zählen als einer.
                if rx.recv_timeout(Duration::from_millis(POLL_MS)).is_ok() {
                    while rx.try_recv().is_ok() {}
                }
            }
        })
        .ok();
}
