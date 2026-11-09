//! Die Fenster der Oberfläche (docs/IPC.md). Alle laden dieselbe Seite und wählen ihre
//! Ansicht über den Hash.
//!
//! | Label | Eigenschaften |
//! |---|---|
//! | `panel` | rahmenlos, transparent, immer oben, nicht in der Taskleiste, nie aktiv beim Zeigen |
//! | `handle` | dasselbe, klein, nie aktiv |
//! | `tooltip` | dasselbe, klickt durch |
//! | `settings` | normales Fenster, Titelleiste folgt dem System-Thema, Schließen versteckt nur |
//! | `ticket` | kleiner Dialog „Ticket zuweisen“, Schließen versteckt nur |

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

pub const PANEL: &str = "panel";
pub const HANDLE: &str = "handle";
pub const TOOLTIP: &str = "tooltip";
pub const SETTINGS: &str = "settings";
pub const TICKET: &str = "ticket";

/// Rahmenlose Überlagerung (Panel, Griff, Tooltip): versteckt angelegt, gezeigt wird
/// sie über `native::show_at`.
fn overlay(app: &AppHandle, label: &str, title: &str, size: (f64, f64)) -> tauri::Result<WebviewWindow> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::App(format!("index.html#/{label}").into()))
        .title(title)
        .inner_size(size.0, size.1)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focusable(false)
        .focused(false)
        .visible(false)
        // Kein festes Thema: die Seite folgt dem System (hell = Rosé Pine Dawn, dunkel =
        // Rosé Pine), wie das Terminal-Profil „Agent Deck“.
        .theme(None)
        .build()
}

/// Normales Fenster, das beim Schließen nur verschwindet (der nächste Aufruf zeigt es
/// sofort wieder, ohne die Seite neu zu laden).
fn dialog(app: &AppHandle, label: &str, title: &str, size: (f64, f64)) -> tauri::Result<WebviewWindow> {
    let w = WebviewWindowBuilder::new(app, label, WebviewUrl::App(format!("index.html#/{label}").into()))
        .title(title)
        .inner_size(size.0, size.1)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .always_on_top(true)
        .visible(false)
        // Kein festes Thema: die Seite folgt dem System (hell = Rosé Pine Dawn, dunkel =
        // Rosé Pine), wie das Terminal-Profil „Agent Deck“.
        .theme(None)
        .build()?;
    let me = w.clone();
    w.on_window_event(move |e| {
        if let WindowEvent::CloseRequested { api, .. } = e {
            api.prevent_close();
            let _ = me.hide();
        }
    });
    Ok(w)
}

/// Alle Fenster anlegen (unsichtbar). Fehlschläge landen im Log; ohne Panel startet die
/// App nicht.
pub fn create_all(app: &AppHandle, english: bool) -> tauri::Result<()> {
    overlay(app, PANEL, "Agent Deck", (900.0, 300.0))?;
    overlay(app, HANDLE, "Agent Deck Griff", (152.0, 30.0))?;
    let tip = overlay(app, TOOLTIP, "Agent Deck Tooltip", (360.0, 120.0))?;
    tip.set_ignore_cursor_events(true)?;
    // Höhe = gemessene Inhaltshöhe (489 px seit dem Ausdünnen am 09.11.2026) plus etwas Luft.
    dialog(app, SETTINGS, if english { "Settings" } else { "Einstellungen" }, (480.0, 496.0))?;
    dialog(app, TICKET, "Ticket", (380.0, 150.0))?;
    Ok(())
}

pub fn get(app: &AppHandle, label: &str) -> Option<WebviewWindow> {
    app.get_webview_window(label)
}

static HWNDS: OnceLock<Mutex<HashMap<String, isize>>> = OnceLock::new();

/// HWND eines eigenen Fensters als Zahl (0 = keins). Gemerkt: die Abfrage geht sonst
/// jedes Mal über den Hauptthread.
pub fn raw_hwnd(app: &AppHandle, label: &str) -> isize {
    let cache = HWNDS.get_or_init(Mutex::default);
    if let Some(h) = cache.lock().ok().and_then(|c| c.get(label).copied()) {
        return h;
    }
    let h = get(app, label).and_then(|w| w.hwnd().ok()).map_or(0, |h| h.0 as isize);
    if h != 0
        && let Ok(mut c) = cache.lock()
    {
        c.insert(label.to_string(), h);
    }
    h
}

/// Einen Dialog zeigen und nach vorn holen.
pub fn present(app: &AppHandle, label: &str) {
    if let Some(w) = get(app, label) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}
