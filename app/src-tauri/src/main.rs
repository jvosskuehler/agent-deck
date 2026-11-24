// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// Die Team-Ausgabe: der Kern ohne Erweiterung (`extension.rs`).
fn main() {
    agent_deck_lib::run_with(tauri::generate_context!(), Vec::new())
}
