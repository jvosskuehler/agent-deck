//! Das freie Panel (Kante `off`) verschieben und an der Ecke zoomen.
//!
//! In Python hatte das schwebende Fenster eine Titelleiste und eine Ecke zum Ziehen
//! (`frameless.py: _undock`, `layout.py: _on_deck_configure`). Das Panel hier ist
//! rahmenlos; die Oberfläche meldet nur, DASS gezogen wird (`panel_drag`), Rust folgt
//! dann dem Zeiger, bis die Taste los ist. So läuft die Bewegung in Bildschirmpixeln,
//! unabhängig von Zoom und Skalierung der Webview, und das Panel wird nie aktiviert.
//! Am Ende werden Lage bzw. Zoom in `deck_settings.json` gemerkt.

use std::time::{Duration, Instant};

use deck_core::dock::{Area, Edge};
use deck_core::zoom::zoom_from_drag;
use serde_json::{Value, json};
use tauri::AppHandle;

use crate::dock::relayout;
use crate::native;
use crate::state::{lock, shared};

/// Takt beim Ziehen (ms): schnell genug, dass das Panel am Zeiger klebt.
const STEP_MS: u64 = 10;
/// Notbremse: länger zieht niemand, dann gilt die Taste als losgelassen.
const MAX_DRAG: Duration = Duration::from_secs(120);

/// Was gezogen wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragMode {
    /// Das ganze Panel verschieben (leere Fläche des Panels).
    Move,
    /// An der Ecke zoomen.
    Zoom,
}

impl DragMode {
    pub fn parse(s: &str) -> Option<DragMode> {
        match s {
            "move" => Some(DragMode::Move),
            "zoom" => Some(DragMode::Zoom),
            _ => None,
        }
    }
}

/// Ziehen beginnen. Nur frei schwebend; läuft schon ein Zug, passiert nichts.
/// `grab` ist die Stelle des Drucks im Panel (CSS-Pixel, `clientX/Y`): daraus wird der
/// Griffpunkt auf dem Bildschirm, nicht aus der Zeigerposition beim Eintreffen des
/// Befehls. Bis dahin ist der Zeiger oft schon ein Stück gewandert, und das Panel hinge
/// sonst um genau dieses Stück hinter ihm.
pub fn begin(app: &AppHandle, mode: DragMode, grab: Option<(f64, f64)>) {
    let s = shared(app);
    let Some(now) = native::cursor() else { return };
    let (rect, zoom0, start) = {
        let mut ctl = lock(&s.dock);
        if ctl.edge != Edge::Off || ctl.dragging {
            return;
        }
        let p = ctl.place(s.desktop.as_ref());
        ctl.dragging = true;
        // Die Lage steht ab jetzt fest, auch wenn sie bisher nur die Vorgabe war.
        ctl.free = Some((p.rect.left, p.rect.top));
        let k = p.scale * p.zoom;
        let start = grab.map_or(now, |(x, y)| (p.rect.left + (x * k).round() as i32, p.rect.top + (y * k).round() as i32));
        (p.rect, p.zoom, start)
    };
    let app = app.clone();
    std::thread::spawn(move || {
        let t0 = Instant::now();
        loop {
            std::thread::sleep(Duration::from_millis(STEP_MS));
            let done = !native::primary_button_down() || t0.elapsed() > MAX_DRAG;
            let pointer = native::cursor().unwrap_or(start);
            let delta = (pointer.0 - start.0, pointer.1 - start.1);
            step(&app, mode, rect, zoom0, delta);
            if done {
                break;
            }
        }
        finish(&app, mode);
    });
}

/// Ein Schritt: Lage bzw. Zoom aus dem Zeigerweg seit dem Zugreifen.
fn step(app: &AppHandle, mode: DragMode, rect: Area, zoom0: f64, delta: (i32, i32)) {
    let s = shared(app);
    let mut ctl = lock(&s.dock);
    match mode {
        DragMode::Move => {
            let (x, y) = (rect.left + delta.0, rect.top + delta.1);
            ctl.free = Some((x, y));
            // Während des Zuges ungeklemmt: das Panel darf über Monitorgrenzen wandern.
            native::move_to(ctl.panel_hwnd(), Area::new(x, y, x + rect.width(), y + rect.height()));
        }
        DragMode::Zoom => {
            ctl.zoom_user = zoom_from_drag(zoom0, (rect.width(), rect.height()), delta);
            relayout(app, &s, &mut ctl);
        }
    }
}

/// Zug zu Ende: auf die Arbeitsfläche klemmen und merken.
fn finish(app: &AppHandle, mode: DragMode) {
    let s = shared(app);
    let (key, value) = {
        let mut ctl = lock(&s.dock);
        ctl.dragging = false;
        let p = relayout(app, &s, &mut ctl);
        ctl.free = Some((p.rect.left, p.rect.top));
        match mode {
            DragMode::Move => ("panel_pos", json!([p.rect.left, p.rect.top])),
            DragMode::Zoom => ("panel_zoom", Value::from((ctl.zoom_user * 1000.0).round() / 1000.0)),
        }
    };
    lock(&s.deck).put(key, value);
}
