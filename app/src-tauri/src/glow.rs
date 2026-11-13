//! Das Fenster „Glow einstellen“ (Strg+Alt+G, `deck_core::config::GLOW_HOTKEY`): Farbe,
//! Stärke und Form des Fokus-Rahmens, je für helles und dunkles Terminal. Jede Änderung
//! landet sofort in [`GlowState`], den der Rahmen-Thread bei jedem Bild liest, und in
//! `deck_settings.json` (`glow`).
//!
//! Solange das Fenster offen ist, zeigt der Rahmen den eigenen Glow auch in der Winterzeit:
//! sonst sähe man im Dezember nicht, was man einstellt.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Arc, RwLock};

use deck_core::glow::{GlowSettings, GlowView, SETTING};
use tauri::{AppHandle, Emitter, WindowEvent};

use crate::state::{lock, shared};
use crate::ui_windows::{GLOW, get, present};

#[derive(Default)]
pub struct GlowState {
    settings: RwLock<GlowSettings>,
    previewing: AtomicBool,
    /// HWND des Fensters, sobald es einmal offen war (0 = noch nie).
    window: AtomicIsize,
}

pub type GlowCell = Arc<GlowState>;

impl GlowState {
    pub fn new(settings: GlowSettings) -> GlowCell {
        Arc::new(GlowState { settings: RwLock::new(settings), previewing: AtomicBool::new(false), window: AtomicIsize::new(0) })
    }

    pub fn get(&self) -> GlowSettings {
        self.settings.read().map(|g| g.clone()).unwrap_or_default()
    }

    /// Ist `hwnd` das Fenster „Glow einstellen“?
    pub fn is_window(&self, hwnd: isize) -> bool {
        hwnd != 0 && self.window.load(Ordering::SeqCst) == hwnd
    }

    /// Das Fenster ist offen: der Winter tritt zurück.
    pub fn previewing(&self) -> bool {
        self.previewing.load(Ordering::SeqCst)
    }
}

/// Werte für den Shader, je Bild gelesen.
pub fn params(cell: &GlowState, night: bool) -> [[f32; 4]; 4] {
    match cell.settings.read() {
        Ok(g) => g.for_night(night).shader_params(),
        Err(_) => GlowSettings::default().for_night(night).shader_params(),
    }
}

pub fn view(app: &AppHandle) -> GlowView {
    let s = shared(app);
    let night = s.desktop.light_theme() == Some(false);
    let lang = lock(&s.deck).lang;
    GlowView { glow: s.glow.get(), night, lang: lang.code().into() }
}

/// Neue Werte übernehmen (in die Grenzen geholt), speichern, beiden Seiten melden.
pub fn set(app: &AppHandle, glow: &GlowSettings) -> GlowView {
    let s = shared(app);
    let mut m = serde_json::Map::new();
    m.insert(SETTING.into(), glow.to_value());
    let clean = GlowSettings::from_settings(&m);
    if let Ok(mut g) = s.glow.settings.write() {
        *g = clean.clone();
    }
    lock(&s.deck).put(SETTING, clean.to_value());
    let v = view(app);
    let _ = app.emit("deck://glow", &v);
    v
}

/// Öffnen (oder nach vorn holen). Beim ersten Öffnen lernt das Fenster, beim Schließen
/// die Vorschau zu beenden.
pub fn open(app: &AppHandle) {
    static HOOKED: std::sync::Once = std::sync::Once::new();
    let s = shared(app);
    if let Some(w) = get(app, GLOW) {
        let cell = s.glow.clone();
        HOOKED.call_once(move || {
            w.on_window_event(move |e| {
                if let WindowEvent::CloseRequested { .. } = e {
                    cell.previewing.store(false, Ordering::SeqCst);
                }
            });
        });
    }
    s.glow.window.store(crate::ui_windows::raw_hwnd(app, GLOW), Ordering::SeqCst);
    s.glow.previewing.store(true, Ordering::SeqCst);
    let _ = app.emit("deck://glow", view(app));
    present(app, GLOW);
}
