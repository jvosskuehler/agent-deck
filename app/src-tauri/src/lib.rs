//! Die App des Agent Decks: Fenster, Andocken, Windows-Terminal-Host, Poll-Takt,
//! Befehle der Oberfläche.
//!
//! Rechnung in `deck-core`, Win32 in `deck-win`, Hintergrunddienste in `deck-services`
//! (Einbaustellen in `services.rs`). Andere Ausgaben bauen über [`extension`] an; diese
//! hier startet ohne Erweiterung (`main.rs`).

pub mod actions;
pub mod agent_env;
pub mod appearance;
pub mod auto_compact;
pub mod app_paths;
pub mod applog;
pub mod crash_resume;
pub mod commands;
pub mod deck;
pub mod dialog_fit;
pub mod dock;
pub mod dock_drag;
pub mod extension;
pub mod focus_frame;
pub mod group_shadow;
pub mod glow;
pub mod hotkey;
pub mod instance_guard;
pub mod launcher;
pub mod menu;
pub mod native;
pub mod orphans;
pub mod poll;
pub mod refs_files;
pub mod reserve_agent;
pub mod services;
pub mod settings;
pub mod state;
pub mod ticket;
pub mod ticket_dialog;
pub mod tooltip;
pub mod ui_watch;
pub mod ui_windows;
pub mod window_sync;
pub mod worker;
pub mod wt_host;
pub mod wt_setup;
pub mod wt_tiles;
pub mod wt_gap;

#[cfg(test)]
mod testing;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_appearance;
#[cfg(test)]
mod tests_compact;
#[cfg(test)]
mod tests_extension;
#[cfg(test)]
mod tests_head;
#[cfg(test)]
mod tests_query;
#[cfg(test)]
mod tests_refs;
#[cfg(test)]
mod tests_reserve;
#[cfg(test)]
mod tests_resume;
#[cfg(test)]
mod tests_tiles;

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

/// Das Deck starten. `context` kommt aus `tauri::generate_context!()` der Binärdatei
/// (ihr `tauri.conf.json`, ihre Oberfläche), `extensions` sind die Anbauten dieser
/// Ausgabe (`extension.rs`); die Team-Ausgabe übergibt keine.
pub fn run_with(context: tauri::Context<tauri::Wry>, extensions: Vec<Box<dyn extension::DeckExtension>>) {
    // Wurde das Deck aus einem Agentenfenster gestartet, trägt es dessen Herkunftsmarke;
    // alles, was es selbst startet (auch Windows Terminal), gälte sonst als Prozess dieses
    // Agenten („Läuft noch ohne Fenster“, `deck_core::orphans::marker`).
    // SAFETY: ganz am Anfang, bevor irgendein anderer Thread läuft.
    unsafe { std::env::remove_var(deck_core::orphans::MARKER_VAR) };
    let paths = Paths::from_env();
    let log = applog::install(&paths.app_dir);
    // Ein Absturz steht im Herzschlag, damit er sich von einem Beenden unterscheiden lässt
    // (auch im Release mit panic=abort läuft der Hook vorher).
    let dir = paths.app_dir.clone();
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        mark_ended(&dir, EndReason::Panic, now());
        prev(info);
    }));
    note(format!("--- Start agent-deck {} (pid {}), Log {} ---", env!("CARGO_PKG_VERSION"), std::process::id(), log.display()));
    // Agenten in WSL: Claudes Ordner (Einstellungen, Transcripts) liegt in der Distro.
    let env = agent_env::resolve(&paths.app_dir, &deck_services::wsl_cli::WslExe);
    let paths = paths.with_claude_dir(agent_env::claude_dir(&env));
    // Hängt ein laufendes Deck, übernimmt dieser Start, statt sein Signal an die tote
    // Instanz weiterzugeben (Einzelinstanz-Sperre) und still zu enden.
    instance_guard::take_over_if_stuck(&paths.app_dir);
    // Gleiche Kennung wie die Startmenü-Verknüpfung, damit Windows beides zusammenführt.
    if let Err(e) = deck_win::taskbar::set_process_app_id(deck_services::shortcut::APP_ID) {
        note(format!("App-Kennung nicht gesetzt: {e}"));
    }
    app_paths::prepare(&paths);
    let desktop: Arc<dyn Desktop> = Arc::new(Win32Desktop);
    let procs: Arc<dyn Processes> = Arc::new(Win32Processes);
    let fx = WtEffects {
        desktop: desktop.clone(),
        procs,
        console: Arc::new(HelperConsole::beside_exe()),
        launcher: Arc::new(launcher::ProcessLauncher),
    };
    let setup = wt_setup::real_setup(&paths.app_dir, &paths.state_dir, paths.wt_agent_script(), &env, (&paths.wsl_launcher(), &paths.wt_agent_sh()));
    let wt = WtHost::new(setup, fx, Worker::spawn("wt-host"), now);
    let host_map = wt.host_map();
    let frame_hold = wt.frame_hold();
    let agent_group = deck_win::taskbar::TaskbarGroup {
        app_id: deck_core::wt::TASKBAR_APP_ID.into(),
        icon: paths.agent_icon(),
        name: deck_core::wt::TASKBAR_NAME.into(),
    };
    let lang = deck_core::i18n::Lang::from_settings(&paths.claude_settings);
    let services = Services::system(&paths, lang, Arc::new(poll::wake), &env);
    let ext = extension::Extensions::new(extensions);
    for e in ext.iter() {
        note(format!("Erweiterung {} geladen", e.name()));
    }
    let mut deck = Deck::new(paths.clone(), wt, desktop.clone(), services);
    deck.ext = ext.clone();
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
    let frame_season = deck.store.settings.get("frame_season").cloned();
    let glow_combo = deck_core::shortcuts::single(&deck.store.settings, "glow_hotkey", deck_core::config::GLOW_HOTKEY);
    let new_agent_combo =
        deck_core::shortcuts::single(&deck.store.settings, "new_agent_hotkey", deck_core::config::NEW_AGENT_HOTKEY);
    let fullscreen_combo =
        deck_core::shortcuts::single(&deck.store.settings, "fullscreen_hotkey", deck_core::config::FULLSCREEN_HOTKEY);
    let glow = glow::GlowState::new(deck_core::glow::GlowSettings::from_settings(&deck.store.settings));
    // Gemeinsamer Schatten für bündig liegende Agenten; `false` lässt jedem seinen eigenen.
    let group_shadow = deck.store.settings.get("group_shadow").and_then(serde_json::Value::as_bool).unwrap_or(true);
    let repos_problem = {
        let state = deck_core::deck_settings::repos_state(deck_core::deck_settings::repos_root_local(&deck.store.settings).as_deref());
        (state != deck_core::deck_settings::ReposState::Ok).then_some(state)
    };
    let english = deck.lang.is_english();
    let watch_dir = paths.app_dir.clone();
    let state = Arc::new(AppState {
        deck: Mutex::new(deck),
        dock: Mutex::new(dock_ctl),
        tip: Mutex::default(),
        last_view: Mutex::new(None),
        menu_open: AtomicBool::new(false),
        restarting: AtomicBool::new(false),
        ticket_slot: Mutex::new(None),
        desktop: desktop.clone(),
        glow: glow.clone(),
    });

    let core_handler: extension::InvokeHandler = Box::new(tauri::generate_handler![
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
        commands::dialog_content_size,
        commands::open_usage_page,
        commands::open_settings,
        commands::set_setting,
        commands::pick_folder,
        commands::restart_panel,
        commands::focus_block,
        commands::get_ticket,
        commands::submit_ticket,
        commands::get_glow,
        commands::set_glow,
        commands::open_glow,
        commands::open_keys,
        commands::show_block_menu,
        commands::panel_hold,
        commands::panel_drag,
        orphans::kill_orphan,
        orphans::kill_all_orphans,
        ui_watch::ui_pong,
    ]);
    let ext_handlers = ext.handlers();
    let setup_ext = ext.clone();

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
        .invoke_handler(move |invoke| {
            // Befehle einer Erweiterung zuerst an sie, alle anderen an den Kern.
            let cmd = invoke.message.command();
            if let Some((_, handler)) = ext_handlers.iter().find(|(names, _)| names.contains(&cmd)) {
                return handler(invoke);
            }
            core_handler(invoke)
        })
        .setup(move |app| {
            let handle = app.handle();
            ui_windows::create_all(handle, english)?;
            // Gleich nach den Fenstern: sie gehen schon im eingestellten Thema auf.
            appearance::start(handle);
            if let Some(why) = repos_problem {
                // Ohne brauchbaren Repos-Ordner zeigt das Deck nicht still ins Leere: das
                // Einstellungsfenster geht auf, mit Hinweis und hervorgehobener Zeile.
                note(format!("Repos-Ordner {why:?}: Einstellungen geöffnet"));
                ui_windows::present(handle, ui_windows::SETTINGS);
            }
            dock::start(handle);
            poll::start(handle);
            hotkey::start(handle, combo);
            hotkey::start_front(handle, front_combos);
            hotkey::start_glow(handle, glow_combo);
            hotkey::start_new_agent(handle, new_agent_combo);
            hotkey::start_fullscreen(handle, fullscreen_combo);
            focus_frame::start(glow, desktop, host_map, frame_hold, Some(agent_group), frame_season, group_shadow);
            ui_watch::start(handle, watch_dir);
            for e in setup_ext.iter() {
                if let Err(why) = e.setup(handle) {
                    note(format!("Erweiterung {}: Start gescheitert, läuft ohne sie weiter: {why}", e.name()));
                }
            }
            Ok(())
        })
        .build(context);
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
