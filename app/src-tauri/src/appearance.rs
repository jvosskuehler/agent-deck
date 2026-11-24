//! Hell/Dunkel in der App umsetzen (`deck_core::appearance`).
//!
//! - **Webviews:** `AppHandle::set_theme` gibt allen Fenstern des Decks das Thema vor; Tauri
//!   reicht es an WebView2 weiter (`PreferredColorScheme`), also schaltet
//!   `prefers-color-scheme` in jeder Ansicht um. `None` = wieder wie Windows.
//! - **Rust:** Rahmen, Gruppenschatten und Glow fragen `Desktop::light_theme`; die echte
//!   Fassung nimmt das feste Thema aus `deck_win::theme::force`.
//! - **Agentenfenster:** neue gehen mit `--colorScheme` auf (`WtHost::set_color_scheme`,
//!   gesetzt in `Deck::new` und `Deck::set_setting`).
//! - **Sonnenstand:** ein Thread schaltet Windows selbst (`deck_services::sun_switch`);
//!   nach dem Aufwachen sieht er sofort nach. Den Ort gibt eine Erweiterung vor, sonst
//!   kommt er grob aus der Zeitzone von Windows ([`sun_place`]).

use std::sync::{Arc, OnceLock};

use deck_core::appearance::Appearance;
use deck_core::sun::Place;
use deck_services::sun_switch::SunSwitch;
use deck_services::sun_theme::RegistryTheme;
use tauri::{AppHandle, Theme};

use crate::applog::note;
use crate::deck::Deck;
use crate::state::{lock, shared};

static SUN: OnceLock<SunSwitch> = OnceLock::new();

fn sun() -> &'static SunSwitch {
    SUN.get_or_init(|| SunSwitch::start(Arc::new(RegistryTheme::real()), crate::now, Arc::new(|line: String| note(line))))
}

/// Beim Start: Thread anlegen, fürs Aufwachen anmelden, dann wie eingestellt setzen.
pub fn start(app: &AppHandle) {
    if let Err(e) = deck_win::theme::on_resume(|| sun().check_now()) {
        note(format!("Hell/Dunkel: Aufwachen nicht abonniert ({e}), der Takt holt es nach"));
    }
    apply(app);
}

/// Die Einstellung umsetzen (beim Start und nach jeder Änderung im Fenster).
pub fn apply(app: &AppHandle) {
    let (a, place) = {
        let st = shared(app);
        let d = lock(&st.deck);
        (Appearance::from_settings(&d.store.settings), sun_place(&d))
    };
    deck_win::theme::force(a.forced_light());
    app.set_theme(a.forced_light().map(|light| if light { Theme::Light } else { Theme::Dark }));
    sun().set(a.follows_sun().then_some(place));
    let where_ = if a.follows_sun() { format!(" ({})", place.de) } else { String::new() };
    note(format!("Hell/Dunkel: {}{where_}", a.as_str()));
}

/// Der Ort für den Sonnenstand: von einer Erweiterung, sonst aus der Zeitzone von
/// Windows (`deck_core::sun_zone`). Ohne lesbare Zeitzone: UTC.
pub fn sun_place(deck: &Deck) -> Place {
    deck.ext.sun_place(&deck.store.settings).unwrap_or_else(|| {
        let (key, bias) = deck_win::time_zone::current().unwrap_or_default();
        deck_core::sun_zone::place_for_zone(&key, bias)
    })
}
