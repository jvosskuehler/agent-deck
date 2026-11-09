//! Die App des Agent Decks: Fenster, Andocken, Windows-Terminal-Host, Poll-Takt,
//! Befehle der Oberfläche.
//!
//! Rechnung in `deck-core`, Win32 in `deck-win`, Hintergrunddienste in `deck-services`
//! (Einbaustellen in `services.rs`).

pub mod actions;
pub mod auto_compact;
pub mod app_paths;
pub mod applog;
pub mod commands;
pub mod deck;
pub mod dock;
pub mod dock_drag;
pub mod focus_frame;
pub mod hotkey;
pub mod launcher;
pub mod menu;
pub mod native;
pub mod orphans;
pub mod poll;
pub mod services;
pub mod settings;
pub mod state;
pub mod ticket;
pub mod ticket_dialog;
pub mod tooltip;
pub mod ui_windows;
pub mod window_sync;
pub mod worker;
pub mod wt_host;
pub mod wt_setup;
pub mod wt_tiles;

#[cfg(test)]
mod testing;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_compact;
#[cfg(test)]
mod tests_head;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use deck_core::config::{FRONT_HOTKEYS, JUMP_HOTKEY};
use deck_services::heartbeat::{EndReason, mark_ended};
use deck_win::effects::{Desktop, HelperConsole, Processes, Win32Desktop, Win32Processes};
use tauri::{Manager, RunEvent};

use crate::app_paths::Paths;
use crate::applog::note;
use crate::deck::Deck;
use crate::dock::DockCtl;
use crate::services::Services;
use crate::state::AppState;
use crate::worker::Worker;
use crate::wt_host::WtHost;
use crate::wt_setup::WtEffects;

/// Unix-Sekunden als Float, wie `ts` in den Zustandsdateien.
pub fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let paths = Paths::from_env();
    let log = applog::install(&paths.app_dir);
    // Ein Absturz steht im Herzschlag, damit der Wächter ihn von einem Beenden
    // unterscheidet (auch im Release mit panic=abort läuft der Hook vorher).
    let dir = paths.app_dir.clone();
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        mark_ended(&dir, EndReason::Panic, now());
        prev(info);
    }));
    note(format!("--- Start agent-deck {} (pid {}), Log {} ---", env!("CARGO_PKG_VERSION"), std::process::id(), log.display()));
    app_paths::prepare(&paths);
    let desktop: Arc<dyn Desktop> = Arc::new(Win32Desktop);
    let procs: Arc<dyn Processes> = Arc::new(Win32Processes);
    let fx = WtEffects {
        desktop: desktop.clone(),
        procs,
        console: Arc::new(HelperConsole::beside_exe()),
        launcher: Arc::new(launcher::ProcessLauncher),
    };
    let setup = wt_setup::real_setup(&paths.app_dir, &paths.state_dir, paths.wt_agent_script());
    let wt = WtHost::new(setup, fx, Worker::spawn("wt-host"), now);
    let host_map = wt.host_map();
    let agent_group = deck_win::taskbar::TaskbarGroup {
        app_id: deck_core::wt::TASKBAR_APP_ID.into(),
        icon: paths.agent_icon(),
        name: deck_core::wt::TASKBAR_NAME.into(),
    };
    let lang = deck_core::i18n::Lang::from_settings(&paths.claude_settings);
    let services = Services::system(&paths, lang, Arc::new(poll::wake));
    let mut deck = Deck::new(paths.clone(), wt, desktop.clone(), services);
    deck.ensure_home_terminal();
    let edge = deck.dock_edge();
    let along = deck_core::dock::saved_along(&deck.store.settings);
    let mut dock_ctl = DockCtl::new(edge, along);
    dock_ctl.free = deck_core::dock::saved_free(&deck.store.settings);
    dock_ctl.zoom_user = deck_core::dock::saved_zoom(&deck.store.settings);
    // Wie Python: fehlt der Schlüssel, gilt die Vorgabe; ein leerer Wert schaltet ab.
    let combo = match deck.store.settings.get("jump_hotkey") {
        Some(serde_json::Value::String(s)) => s.clone(),
        _ => JUMP_HOTKEY.to_string(),
    };
    let front_combos = deck_core::front::combos(deck.store.settings.get("front_hotkeys"), &FRONT_HOTKEYS);
    let english = deck.lang.is_english();
    let state = Arc::new(AppState {
        deck: Mutex::new(deck),
        dock: Mutex::new(dock_ctl),
        tip: Mutex::default(),
        last_view: Mutex::new(None),
        menu_open: AtomicBool::new(false),
        restarting: AtomicBool::new(false),
        ticket_slot: Mutex::new(None),
        desktop: desktop.clone(),
    });

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Ein zweiter Start öffnet kein zweites Deck, er klappt dieses auf.
            let app = app.clone();
            std::thread::spawn(move || dock::reveal_for_request(&app));
        }))
        .plugin(tauri_plugin_opener::init())
        .manage(state)
        .on_menu_event(|app, event| {
            // Läuft auf dem Hauptthread: der darf nicht auf das Deck warten.
            let (app, id) = (app.clone(), event.id().0.clone());
            std::thread::spawn(move || menu::handle(&app, &id));
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_view,
            commands::get_dock,
            commands::get_settings,
            commands::panel_content_size,
            commands::focus_slot,
            commands::close_slot,
            commands::create_agent,
            commands::show_tile_menu,
            commands::reorder_tiles,
            commands::reorder_blocks,
            commands::hover_tile,
            commands::tooltip_content_size,
            commands::open_usage_page,
            commands::open_settings,
            commands::set_setting,
            commands::restart_panel,
            commands::focus_block,
            commands::get_ticket,
            commands::submit_ticket,
            commands::show_block_menu,
            commands::panel_hold,
            commands::panel_drag,
            orphans::kill_orphan,
            orphans::kill_all_orphans,
        ])
        .setup(move |app| {
            let handle = app.handle();
            ui_windows::create_all(handle, english)?;
            dock::start(handle);
            poll::start(handle);
            hotkey::start(handle, combo);
            hotkey::start_front(handle, front_combos);
            focus_frame::start(desktop, host_map, Some(agent_group));
            Ok(())
        })
        .build(tauri::generate_context!());
    let app_dir = paths.app_dir.clone();
    let app = match app {
        Ok(a) => a,
        Err(e) => {
            note(format!("Start fehlgeschlagen: {e}"));
            return;
        }
    };
    app.run(move |handle, event| {
        if let RunEvent::Exit = event {
            let s = handle.state::<state::Shared>();
            if s.restarting.load(std::sync::atomic::Ordering::SeqCst) {
                note("--- Panel-Ende (Neustart, Kind übernimmt) ---");
            } else {
                mark_ended(&app_dir, EndReason::Exit, now());
                note("--- Ende agent-deck ---");
            }
        }
    });
}
