//! Die App des Agent Decks: Fenster, Andocken, Broker zur VS-Code-Extension,
//! Windows-Terminal-Host, Poll-Takt, Befehle der Oberfläche.
//!
//! Verhalten wie das Python-Deck (`deck/ui/panel.py` und Nachbarn), Rechnung in
//! `deck-core`, Win32 in `deck-win`, Hintergrunddienste in `deck-services`
//! (Einbaustellen in `services.rs`).
//!
//! Probebetrieb neben dem laufenden Python-Deck: `DECK_NO_BROKER=1` (kein Broker) oder
//! `DECK_BROKER_PORT=<port>`, dazu `DECK_DOCK_EDGE=off|left|…` für eine Kante nur in
//! diesem Lauf. Ohne eigenen Broker auf 8765 räumt das Deck nichts ab
//! (keine Bindungen vergessen, keine worktrees löschen), kachelt keine Terminal-Fenster,
//! belegt nicht die Sprung-Taste und zeichnet keinen Fokus-Rahmen: das tut noch das
//! Python-Deck.

pub mod actions;
pub mod app_paths;
pub mod applog;
pub mod block_head;
pub mod broker;
pub mod commands;
pub mod deck;
pub mod dock;
pub mod dock_drag;
pub mod focus_frame;
pub mod hosts;
pub mod hotkey;
pub mod launcher;
pub mod menu;
pub mod native;
pub mod poll;
pub mod services;
pub mod settings;
pub mod state;
pub mod ticket;
pub mod ticket_dialog;
pub mod tools;
pub mod tooltip;
pub mod ui_windows;
pub mod window_sync;
pub mod worker;
pub mod wt_host;
pub mod wt_tiles;

#[cfg(test)]
mod testing;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_head;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use deck_core::config::{BROKER_HOST, BROKER_PORT, FRONT_HOTKEYS, JUMP_HOTKEY};
use deck_services::heartbeat::{EndReason, mark_ended};
use deck_win::effects::{Desktop, HelperConsole, Processes, Win32Desktop, Win32Processes};
use tauri::{Manager, RunEvent};

use crate::app_paths::Paths;
use crate::applog::note;
use crate::broker::Broker;
use crate::deck::Deck;
use crate::dock::DockCtl;
use crate::hosts::Hosts;
use crate::services::Services;
use crate::state::AppState;
use crate::worker::Worker;
use crate::wt_host::{WtEffects, WtHost};

/// Unix-Sekunden als Float, wie `ts` in den Zustandsdateien.
pub fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// Broker nach Umgebung: aus, anderer Port oder der echte auf 8765.
fn broker_from_env() -> (Arc<Broker>, bool) {
    if std::env::var("DECK_NO_BROKER").is_ok_and(|v| v.trim() == "1") {
        note("DECK_NO_BROKER=1: ohne Broker (Probebetrieb)");
        return (Arc::new(Broker::offline()), true);
    }
    let port = std::env::var("DECK_BROKER_PORT").ok().and_then(|p| p.trim().parse::<u16>().ok()).unwrap_or(BROKER_PORT);
    let broker = Broker::start(BROKER_HOST, port);
    let trial = port != BROKER_PORT || !broker.listening();
    (broker, trial)
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
    let (broker, trial) = broker_from_env();
    app_paths::prepare(&paths, &app_paths::legacy_roots(), !trial);
    let desktop: Arc<dyn Desktop> = Arc::new(Win32Desktop);
    let procs: Arc<dyn Processes> = Arc::new(Win32Processes);
    let fx = WtEffects {
        desktop: desktop.clone(),
        procs: procs.clone(),
        console: Arc::new(HelperConsole::beside_exe()),
        launcher: Arc::new(launcher::ProcessLauncher),
    };
    let setup = wt_host::real_setup(&paths.app_dir, &paths.state_dir, paths.wt_agent_script());
    let mut wt = WtHost::new(setup, fx, Worker::spawn("wt-host"), now);
    wt.retile = !trial;
    let host_map = wt.host_map();
    let agent_group = deck_win::taskbar::TaskbarGroup {
        app_id: deck_core::wt::TASKBAR_APP_ID.into(),
        icon: paths.agent_icon(),
        name: deck_core::wt::TASKBAR_NAME.into(),
    };
    let hosts = Hosts { vscode: broker.clone(), wt };
    let lang = deck_core::i18n::Lang::from_settings(&paths.claude_settings);
    let services = Services::system(&paths, broker.clone(), desktop.clone(), procs, lang, trial, Arc::new(poll::wake));
    let mut deck = Deck::new(paths.clone(), hosts, desktop.clone(), services);
    deck.cleanup_allowed = !trial;
    if trial {
        note("Probebetrieb: kein Abräumen, kein Kacheln, keine Sprung-Taste, kein Fokus-Rahmen");
    }
    deck.ensure_home_terminal();
    let glow = deck.store.settings.get("glow").is_some_and(deck_core::pystr::truthy);
    deck.services.glow_self_heal(glow, deck.lang);
    // Probebetrieb: Kante nur für diesen Lauf, ohne die Einstellung zu ändern.
    let edge = match std::env::var("DECK_DOCK_EDGE") {
        Ok(e) if !e.trim().is_empty() => deck_core::dock::Edge::parse(Some(e.trim())),
        _ => deck.dock_edge(),
    };
    let along = dock::saved_along(&deck.store.settings);
    let mut dock_ctl = DockCtl::new(edge, along);
    dock_ctl.free = dock::saved_free(&deck.store.settings);
    dock_ctl.zoom_user = dock::saved_zoom(&deck.store.settings);
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
        broker: broker.clone(),
    });

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Ein zweiter Start öffnet kein zweites Deck, er klappt dieses auf.
            let app = app.clone();
            std::thread::spawn(move || dock::reveal_for_request(&app));
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
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
            commands::run_tool,
            commands::focus_block,
            commands::get_ticket,
            commands::submit_ticket,
            commands::show_block_menu,
            commands::confirm_close_block,
            commands::panel_hold,
            commands::panel_drag,
            commands::foreign_vscode_windows,
        ])
        .setup(move |app| {
            let handle = app.handle();
            ui_windows::create_all(handle, english)?;
            dock::start(handle);
            poll::start(handle);
            if !trial {
                hotkey::start(handle, combo);
                hotkey::start_front(handle, front_combos);
                focus_frame::start(desktop, host_map, Some(agent_group));
            }
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
            // Port sofort freigeben: ein Neustart soll ihn wieder binden können.
            s.broker.stop();
            if s.restarting.load(std::sync::atomic::Ordering::SeqCst) {
                note("--- Panel-Ende (Neustart, Kind übernimmt) ---");
            } else {
                mark_ended(&app_dir, EndReason::Exit, now());
                note("--- Ende agent-deck ---");
            }
        }
    });
}
