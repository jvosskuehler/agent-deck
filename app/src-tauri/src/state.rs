//! Der geteilte Zustand der App, wie Tauri ihn verwaltet (`app.state::<Shared>()`).
//!
//! Sperr-Reihenfolge, damit sich nichts gegenseitig blockiert: höchstens EIN Mutex auf
//! einmal halten. Wer Deck und Dock braucht, holt sich erst, was er vom Deck braucht,
//! gibt es frei und sperrt dann das Dock.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, MutexGuard};

use deck_core::view::DeckView;
use deck_win::effects::Desktop;
use tauri::{AppHandle, Manager};

use crate::deck::Deck;
use crate::dock::DockCtl;
use crate::tooltip::TipState;

pub struct AppState {
    pub deck: Mutex<Deck>,
    pub dock: Mutex<DockCtl>,
    pub tip: Mutex<TipState>,
    /// Zuletzt gesendete Ansicht (für `get_view` und den Vergleich im Takt).
    pub last_view: Mutex<Option<DeckView>>,
    /// Das Kontextmenü einer Kachel ist offen: das Panel bleibt offen.
    pub menu_open: AtomicBool,
    /// „Panel neu starten“ läuft: das Ende ist kein normales (der Wächter soll
    /// einspringen, falls das Kind nicht hochkommt).
    pub restarting: AtomicBool,
    /// Slot, für den der Ticket-Dialog gerade offen ist.
    pub ticket_slot: Mutex<Option<String>>,
    pub desktop: Arc<dyn Desktop>,
}

pub type Shared = Arc<AppState>;

pub fn shared(app: &AppHandle) -> Shared {
    app.state::<Shared>().inner().clone()
}

/// Ein Mutex, der nach einem Panic in einem anderen Thread weiter benutzbar bleibt: ein
/// abgestürzter Takt soll nicht jede spätere Aktion mitreißen.
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
