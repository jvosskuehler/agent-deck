//! Die Befehle der Oberfläche (docs/IPC.md, Tabelle „Befehle“).
//!
//! Alle asynchron: Tauri führt synchrone Befehle auf dem Hauptthread aus, und der darf
//! nie auf einen Mutex des Decks warten (siehe `poll.rs`).

use std::sync::atomic::Ordering;

use deck_core::tooltip::ScreenRect;
use deck_core::view::{DeckView, DockView, SettingsView, TicketView};
use serde_json::Value;
use tauri::{AppHandle, Emitter};
use tauri_plugin_opener::OpenerExt;

use crate::applog::note;
use crate::settings::Effect;
use crate::state::{lock, shared};
use crate::tooltip;
use crate::ui_windows::{SETTINGS, present};
use crate::dock_drag::DragMode;
use crate::{dock, dock_drag, menu, poll, ticket_dialog};

const USAGE_URL: &str = "https://claude.ai/settings/usage";

type Res<T> = Result<T, String>;

#[tauri::command]
pub async fn get_view(app: AppHandle) -> Res<DeckView> {
    let s = shared(&app);
    if let Some(v) = lock(&s.last_view).clone() {
        return Ok(v);
    }
    Ok(lock(&s.deck).view(crate::now()))
}

#[tauri::command]
pub async fn get_dock(app: AppHandle) -> Res<DockView> {
    Ok(lock(&shared(&app).dock).view())
}

#[tauri::command]
pub async fn get_settings(app: AppHandle) -> Res<SettingsView> {
    Ok(lock(&shared(&app).deck).settings_view())
}

#[tauri::command]
pub async fn panel_content_size(app: AppHandle, width: f64, height: f64) -> Res<()> {
    dock::content_size(&app, width, height);
    Ok(())
}

#[tauri::command]
pub async fn focus_slot(app: AppHandle, slot: String) -> Res<()> {
    tooltip::hide(&app);
    lock(&shared(&app).deck).focus_slot(&slot);
    poll::nudge(&app);
    Ok(())
}

#[tauri::command]
pub async fn close_slot(app: AppHandle, slot: String) -> Res<()> {
    tooltip::hide(&app);
    lock(&shared(&app).deck).close_agent(&slot);
    poll::nudge(&app);
    Ok(())
}

#[tauri::command]
pub async fn create_agent(app: AppHandle, letter: String) -> Res<()> {
    lock(&shared(&app).deck).create_agent(&letter, crate::now());
    poll::nudge(&app);
    Ok(())
}

#[tauri::command]
pub async fn show_tile_menu(app: AppHandle, slot: String) -> Res<()> {
    // Das Menü blockiert, bis es zu ist: nicht auf einem Arbeiter der async-Laufzeit.
    tauri::async_runtime::spawn_blocking(move || menu::show(&app, &slot)).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn reorder_tiles(app: AppHandle, letter: String, slots: Vec<String>) -> Res<()> {
    lock(&shared(&app).deck).reorder_tiles(&letter, slots);
    poll::nudge(&app);
    Ok(())
}

#[tauri::command]
pub async fn reorder_blocks(app: AppHandle, letters: Vec<String>) -> Res<()> {
    lock(&shared(&app).deck).reorder_blocks(&letters);
    poll::nudge(&app);
    Ok(())
}

#[tauri::command]
pub async fn hover_tile(app: AppHandle, slot: Option<String>, rect: Option<ScreenRect>) -> Res<()> {
    tooltip::hover(&app, slot, rect);
    Ok(())
}

#[tauri::command]
pub async fn tooltip_content_size(app: AppHandle, width: f64, height: f64) -> Res<()> {
    tooltip::content_size(&app, width, height);
    Ok(())
}

/// Ein Dialog meldet seine Inhaltshöhe; sein Fenster wächst bzw. schrumpft mit.
#[tauri::command]
pub async fn dialog_content_size(window: tauri::WebviewWindow, height: f64) -> Res<()> {
    crate::ui_windows::fit_dialog(&window, height).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn open_usage_page(app: AppHandle) -> Res<()> {
    app.opener().open_url(USAGE_URL, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn open_settings(app: AppHandle) -> Res<()> {
    present(&app, SETTINGS);
    Ok(())
}

#[tauri::command]
pub async fn set_setting(app: AppHandle, key: String, value: Value) -> Res<SettingsView> {
    let s = shared(&app);
    let (effect, view) = {
        let mut d = lock(&s.deck);
        let effect = d.set_setting(&key, &value)?;
        (effect, d.settings_view())
    };
    match effect {
        Effect::Language => {
            // Neue Statustexte: den Vergleich im Takt aushebeln und sofort senden.
            *lock(&s.last_view) = None;
            poll::nudge(&app);
        }
        Effect::Edge(edge) => dock::set_edge(&app, edge),
        Effect::None => {}
    }
    let _ = app.emit("deck://settings", &view);
    Ok(view)
}

/// „Panel neu starten“ (`panel.py: restart`): eine frische Instanz mit denselben
/// Argumenten und derselben Umgebung, dann diese beenden. Tauri gibt vorher Port und
/// Einzelinstanz-Sperre frei. Bewusst ohne „normales Ende“ im Herzschlag: kommt das
/// Kind nicht hoch, soll der Wächter einspringen.
#[tauri::command]
pub async fn restart_panel(app: AppHandle) -> Res<()> {
    shared(&app).restarting.store(true, Ordering::SeqCst);
    note("--- Panel-Neustart angefordert (Kind übernimmt) ---");
    app.request_restart();
    Ok(())
}

#[tauri::command]
pub async fn show_block_menu(app: AppHandle, letter: String) -> Res<()> {
    tauri::async_runtime::spawn_blocking(move || menu::show_block(&app, &letter)).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn panel_hold(app: AppHandle, hold: bool) -> Res<()> {
    dock::set_hold(&app, hold);
    Ok(())
}

#[tauri::command]
pub async fn panel_drag(app: AppHandle, mode: String, x: Option<f64>, y: Option<f64>) -> Res<()> {
    let mode = DragMode::parse(&mode).ok_or_else(|| format!("unbekannter Zug: {mode}"))?;
    dock_drag::begin(&app, mode, x.zip(y));
    Ok(())
}

#[tauri::command]
pub async fn focus_block(app: AppHandle, letter: String) -> Res<()> {
    tooltip::hide(&app);
    lock(&shared(&app).deck).show_window(&letter);
    Ok(())
}

#[tauri::command]
pub async fn open_keys(app: AppHandle) -> Res<()> {
    present(&app, crate::ui_windows::KEYS);
    Ok(())
}

#[tauri::command]
pub async fn get_glow(app: AppHandle) -> Res<deck_core::glow::GlowView> {
    Ok(crate::glow::view(&app))
}

#[tauri::command]
pub async fn set_glow(app: AppHandle, glow: deck_core::glow::GlowSettings) -> Res<deck_core::glow::GlowView> {
    Ok(crate::glow::set(&app, &glow))
}

#[tauri::command]
pub async fn open_glow(app: AppHandle) -> Res<()> {
    crate::glow::open(&app);
    Ok(())
}

#[tauri::command]
pub async fn get_ticket(app: AppHandle) -> Res<Option<TicketView>> {
    Ok(ticket_dialog::view(&app))
}

#[tauri::command]
pub async fn submit_ticket(app: AppHandle, ticket: Option<String>) -> Res<()> {
    ticket_dialog::submit(&app, ticket);
    Ok(())
}
