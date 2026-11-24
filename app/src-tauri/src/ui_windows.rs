//! Die Fenster der Oberfläche (docs/IPC.md). Alle laden dieselbe Seite und wählen ihre
//! Ansicht über den Hash.
//!
//! | Label | Eigenschaften |
//! |---|---|
//! | `panel` | rahmenlos, transparent, immer oben, nicht in der Taskleiste, nie aktiv beim Zeigen |
//! | `handle` | dasselbe, klein, nie aktiv |
//! | `tooltip` | dasselbe, klickt durch |
//! | `settings` | normales Fenster, Titelleiste folgt dem Thema (Windows oder „Hell/Dunkel“), Schließen versteckt nur |
//! | `ticket` | kleiner Dialog „Ticket zuweisen“, Schließen versteckt nur |
//! | `keys` | „Tastenkürzel“, nur zum Ansehen, Schließen versteckt nur |
//! | `glow` | „Glow einstellen“ (Strg+Alt+G), Schließen versteckt nur |
//!
//! Jeder Dialog (`dialog()`) ist so hoch wie sein Inhalt: die Ansicht misst sich mit
//! `useDialogFit`, Rust setzt die Höhe (`fit_dialog`). Ein Rollbalken erscheint nur, wenn
//! der Inhalt höher als der Bildschirm ist. Wer einen Dialog dazubaut, hängt ihn dort an.
//!
//! Erweiterungen (`extension.rs`) legen ihre Fenster in `setup` mit [`companion`] an:
//! dieselbe Seite, eigener Hash.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

pub const PANEL: &str = "panel";
pub const HANDLE: &str = "handle";
pub const TOOLTIP: &str = "tooltip";
pub const SETTINGS: &str = "settings";
pub const TICKET: &str = "ticket";
pub const GLOW: &str = "glow";
pub const KEYS: &str = "keys";

/// Grundgerüst jedes Fensters. Nur im Messstand (`tools/probe`) gesetzt:
/// `AGENT_DECK_DEVTOOLS_PORT` öffnet den DevTools-Port der WebView2, darüber ruft das
/// Messskript dieselben Befehle wie ein Klick. Die Umgebungsvariable und die Richtlinie
/// der WebView2 für Zusatzargumente greifen nicht, weil Tauri eigene setzt; die Argumente
/// müssen in allen Fenstern gleich sein, sonst lehnt WebView2 das zweite ab.
fn builder<'a>(app: &'a AppHandle, label: &str) -> WebviewWindowBuilder<'a, tauri::Wry, AppHandle> {
    let b = WebviewWindowBuilder::new(app, label, WebviewUrl::App(format!("index.html#/{label}").into()));
    match std::env::var("AGENT_DECK_DEVTOOLS_PORT").ok().and_then(|p| p.parse::<u16>().ok()) {
        Some(port) => b.additional_browser_args(&format!(
            "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --remote-debugging-port={port}"
        )),
        None => b,
    }
}

/// Rahmenlose Überlagerung (Panel, Griff, Tooltip): versteckt angelegt, gezeigt wird
/// sie über `native::show_at`.
fn overlay(app: &AppHandle, label: &str, title: &str, size: (f64, f64)) -> tauri::Result<WebviewWindow> {
    builder(app, label)
        .title(title)
        .inner_size(size.0, size.1)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        // Nicht minimierbar: Win+M nähme sonst den Griff mit, und nichts holt ihn zurück.
        .minimizable(false)
        .maximizable(false)
        .focusable(false)
        .focused(false)
        .visible(false)
        // Kein eigenes Thema je Fenster: es gilt das der App, also Windows oder die feste
        // Wahl unter „Hell/Dunkel“ (`crate::appearance`).
        .theme(None)
        .build()
}

/// Ein Begleiter der Agentenfenster für Erweiterungen: rahmenlos, nicht in der
/// Taskleiste, nie aktiv, aber **nicht** immer oben. Er gehört zu den Agenten und geht mit
/// ihnen nach hinten; wo er liegt, bestimmt die Erweiterung (`native::show_behind`,
/// `native::show_above`). Versteckt angelegt, Seite `index.html#/<label>`.
pub fn companion(app: &AppHandle, label: &str, title: &str, size: (f64, f64)) -> tauri::Result<WebviewWindow> {
    builder(app, label)
        .title(title)
        .inner_size(size.0, size.1)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(false)
        .skip_taskbar(true)
        .resizable(false)
        .minimizable(false)
        .maximizable(false)
        .focusable(false)
        .focused(false)
        .visible(false)
        .theme(None)
        .build()
}

/// Normales Fenster, das beim Schließen nur verschwindet (der nächste Aufruf zeigt es
/// sofort wieder, ohne die Seite neu zu laden).
fn dialog(app: &AppHandle, label: &str, title: &str, size: (f64, f64)) -> tauri::Result<WebviewWindow> {
    let w = builder(app, label)
        .title(title)
        .inner_size(size.0, size.1)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .always_on_top(true)
        .visible(false)
        // Kein eigenes Thema je Fenster: es gilt das der App, also Windows oder die feste
        // Wahl unter „Hell/Dunkel“ (`crate::appearance`).
        .theme(None)
        .build()?;
    let me = w.clone();
    w.on_window_event(move |e| match e {
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            let _ = me.hide();
        }
        // Vom Nutzer gezogen: neuer Anker; selbst gesetzt: ignoriert (`dialog_fit`).
        WindowEvent::Moved(p) => crate::dialog_fit::moved(me.label(), p.x, p.y),
        // Windows hat das Fenster beim Monitorwechsel mitskaliert: Größe neu anlegen.
        WindowEvent::ScaleFactorChanged { .. } => crate::dialog_fit::refit(&me),
        _ => {}
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
    // Kompakt eine Spalte (480); „Erweitert“ zieht es auf zwei Spalten (960) auf. Die
    // Maße hier sind nur der Start, useDialogFit setzt Breite und Inhaltshöhe.
    dialog(app, SETTINGS, if english { "Settings" } else { "Einstellungen" }, (480.0, 572.0))?;
    dialog(app, TICKET, "Ticket", (380.0, 150.0))?;
    dialog(app, KEYS, if english { "Shortcuts" } else { "Tastenkürzel" }, (500.0, 250.0))?;
    dialog(app, GLOW, if english { "Adjust glow" } else { "Glow einstellen" }, (720.0, 760.0))?;
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

/// Einen Dialog auf die gemeldete Inhaltshöhe (CSS-Pixel) bringen, damit er ohne
/// Rollbalken auskommt: er wächst und schrumpft mit dem Inhalt. Die Breite bleibt, außer
/// `width` ist angegeben. Nie größer als der Arbeitsbereich seines Monitors; ragt er unten
/// oder rechts hinaus, rückt er zurück.
pub fn fit_dialog(w: &WebviewWindow, height: f64, width: Option<f64>) -> tauri::Result<()> {
    crate::dialog_fit::fit(w, height, width)
}

/// Einen Dialog zeigen und nach vorn holen.
pub fn present(app: &AppHandle, label: &str) {
    if let Some(w) = get(app, label) {
        let _ = w.unminimize();
        let _ = w.show();
        crate::dialog_fit::shown(&w);
        let _ = w.set_focus();
    }
}
