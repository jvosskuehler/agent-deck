//! Andocken und Auto-Hide (`deck/dock/`): Panel bündig an der Kante, eingeklappt nur
//! der Griff. Gerechnet wird in `deck_core::dock`; hier stehen die Fenster und der Takt.
//!
//! Ablauf (docs/IPC.md):
//! - Aufklappen: Panel-Fenster zeigen (die Oberfläche steht noch auf `closed`), dann
//!   `deck://dock {phase: open}` (sie gleitet 220 ms herein), dann den Griff verbergen.
//!   So ist die Kante nie einen Moment leer.
//! - Einklappen: `phase: closed` (sie gleitet 180 ms hinaus), Griff zeigen, das Panel
//!   frühestens 200 ms später verbergen. Klappt es vorher wieder auf, bleibt es stehen.
//!
//! Wie in Python liegt das Deck auf dem Hauptmonitor; die Lage entlang der Kante ist
//! `dock_along` aus `deck_settings.json` (sonst mittig). Frei schwebend (`off`) liegt es,
//! wo der Nutzer es hingezogen hat (`panel_pos`), auf dem Monitor dieser Stelle.
//!
//! Größe (`deck/ui/layout.py`): das Fenster folgt dem Inhalt mal Zoom. Der Zoom ist der
//! vom Nutzer an der Ecke gezogene (`panel_zoom`), verkleinert, wenn das Panel sonst
//! nicht auf die Arbeitsfläche passte: lieber alles kleiner als etwas abgeschnitten.
//! Gezeichnet wird er als Zoom der Webview, die Oberfläche misst weiter ihre natürliche
//! Größe.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use deck_core::dock::{
    Area, AutoHide, DockAction, Edge, HIDE_AFTER_CLOSE_MS, INSIDE_MARGIN, POLL_MS, Sight, handle_rect, panel_body,
    panel_rect, reveal_zone,
};
use deck_core::zoom::{ZOOM_MAX, ZOOM_MIN, fit_zoom};
use deck_core::view::DockView;
use deck_win::effects::Desktop;
use tauri::{AppHandle, Emitter};

use crate::native;
use crate::state::{Shared, lock, shared};
use crate::tooltip;
use crate::ui_windows::{HANDLE, PANEL, get, raw_hwnd};

/// Ersatzgröße des Panels, bis die Oberfläche ihre gemessen hat (CSS-Pixel).
const DEFAULT_CONTENT: (f64, f64) = (900.0, 300.0);
/// Ein Halten fürs Ziehen (`panel_hold`) gilt höchstens so lange: stürzt die Oberfläche
/// mitten im Zug ab, bliebe das Panel sonst für immer offen.
const HOLD_MAX: Duration = Duration::from_secs(60);

pub struct DockCtl {
    pub edge: Edge,
    along: Option<i32>,
    auto: AutoHide,
    content: (f64, f64),
    hide_gen: u64,
    t0: Instant,
    panel: isize,
    handle: isize,
    /// Lage des freien Panels (linke obere Ecke, Bildschirmpixel), `panel_pos`.
    pub free: Option<(i32, i32)>,
    /// Vom Nutzer gezogener Zoom (`panel_zoom`).
    pub zoom_user: f64,
    /// Zuletzt an die Webview gegebener, wirksamer Zoom.
    zoom: f64,
    /// Die Oberfläche zieht gerade eine Karte oder einen Block (`panel_hold`).
    drag_hold: Option<Instant>,
    /// Das freie Panel wird gerade verschoben oder gezoomt (`panel_drag`).
    pub dragging: bool,
}

/// Wo das Panel liegt und wie groß es gezeichnet wird.
#[derive(Debug, Clone, Copy)]
pub struct Placement {
    pub rect: Area,
    /// Wirksamer Zoom (Webview).
    pub zoom: f64,
    /// Skalierung des Monitors.
    pub scale: f64,
}

/// Arbeitsfläche und Skalierung des Hauptmonitors.
fn primary(desktop: &dyn Desktop) -> (Area, f64) {
    let mons = desktop.monitors();
    match mons.iter().find(|m| m.primary).or(mons.first()) {
        Some(m) => (Area::new(m.work.left, m.work.top, m.work.right, m.work.bottom), m.scale),
        None => (Area::new(0, 0, 1920, 1040), 1.0),
    }
}

impl DockCtl {
    pub fn new(edge: Edge, along: Option<i32>) -> DockCtl {
        DockCtl {
            edge,
            along,
            auto: AutoHide::collapsed(),
            content: DEFAULT_CONTENT,
            hide_gen: 0,
            t0: Instant::now(),
            panel: 0,
            handle: 0,
            free: None,
            zoom_user: 1.0,
            zoom: 1.0,
            drag_hold: None,
            dragging: false,
        }
    }

    fn now_ms(&self) -> f64 {
        self.t0.elapsed().as_secs_f64() * 1000.0
    }

    /// Offen, wenn frei schwebend oder ausgeklappt.
    pub fn open(&self) -> bool {
        self.edge == Edge::Off || self.auto.is_expanded()
    }

    pub fn view(&self) -> DockView {
        DockView {
            edge: self.edge.as_str().into(),
            phase: if self.open() { "open" } else { "closed" }.into(),
            zoom: self.zoom,
        }
    }

    /// Arbeitsfläche, auf der das Panel liegt: angedockt der Hauptmonitor, frei der
    /// Monitor seiner gemerkten Lage.
    fn area(&self, desktop: &dyn Desktop) -> (Area, f64) {
        match (self.edge, self.free) {
            (Edge::Off, Some((x, y))) => monitor_at(desktop, x + 1, y + 1),
            _ => primary(desktop),
        }
    }

    pub fn place(&self, desktop: &dyn Desktop) -> Placement {
        let (work, scale) = self.area(desktop);
        let natural = (self.content.0 * scale, self.content.1 * scale);
        let zoom = fit_zoom(natural, work, self.zoom_user);
        let size = ((natural.0 * zoom).round() as i32, (natural.1 * zoom).round() as i32);
        Placement { rect: panel_rect(self.edge, work, self.along, size, scale, self.free), zoom, scale }
    }

    /// Hält gerade etwas das Panel offen, weil die Oberfläche zieht?
    fn held_by_drag(&self) -> bool {
        self.drag_hold.is_some_and(|t| t.elapsed() < HOLD_MAX)
    }

    pub fn panel_hwnd(&self) -> isize {
        self.panel
    }
}

/// Den wirksamen Zoom an die Webview geben, wenn er sich geändert hat (und die
/// Oberfläche davon wissen lassen).
fn apply_zoom(app: &AppHandle, ctl: &mut DockCtl, zoom: f64) {
    if (zoom - ctl.zoom).abs() < 1e-3 {
        return;
    }
    ctl.zoom = zoom;
    if let Some(w) = get(app, PANEL)
        && let Err(e) = w.set_zoom(zoom)
    {
        crate::applog::note(format!("Zoom {zoom:.2}: {e}"));
    }
    emit(app, ctl);
}

/// Das Panel neu legen: Zoom nachziehen, Fenster an die neue Größe.
pub fn relayout(app: &AppHandle, s: &Shared, ctl: &mut DockCtl) -> Placement {
    let p = ctl.place(s.desktop.as_ref());
    apply_zoom(app, ctl, p.zoom);
    native::move_to(ctl.panel, p.rect);
    p
}

fn emit(app: &AppHandle, ctl: &DockCtl) {
    let _ = app.emit("deck://dock", ctl.view());
}

/// Aufklappen: zeigen, melden, Griff weg.
fn show_open(app: &AppHandle, s: &Shared, ctl: &mut DockCtl) {
    let p = ctl.place(s.desktop.as_ref());
    apply_zoom(app, ctl, p.zoom);
    ctl.hide_gen += 1;
    native::show_at(ctl.panel, p.rect);
    emit(app, ctl);
    native::hide(ctl.handle);
}

/// Einklappen: melden, Griff zeigen, Panel verzögert verbergen.
fn show_closed(app: &AppHandle, s: &Shared, ctl: &mut DockCtl) {
    let (work, scale) = primary(s.desktop.as_ref());
    emit(app, ctl);
    native::show_at(ctl.handle, handle_rect(ctl.edge, work, ctl.along, scale));
    tooltip::hide(app);
    ctl.hide_gen += 1;
    let (generation, app) = (ctl.hide_gen, app.clone());
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(HIDE_AFTER_CLOSE_MS));
        let s = shared(&app);
        let ctl = lock(&s.dock);
        if ctl.hide_gen == generation && !ctl.open() {
            native::hide(ctl.panel);
        }
    });
}

/// Beim Start: Fenster merken, gespeicherte Kante anwenden (angedockt sofort zu, ohne
/// Gleiten), Zeiger-Takt starten.
pub fn start(app: &AppHandle) {
    let s = shared(app);
    {
        let mut ctl = lock(&s.dock);
        ctl.panel = raw_hwnd(app, PANEL);
        ctl.handle = raw_hwnd(app, HANDLE);
        if ctl.edge == Edge::Off {
            ctl.auto = AutoHide::expanded();
            show_open(app, &s, &mut ctl);
        } else {
            ctl.auto = AutoHide::collapsed();
            show_closed(app, &s, &mut ctl);
        }
    }
    let app = app.clone();
    std::thread::Builder::new()
        .name("dock-pointer".into())
        .spawn(move || {
            loop {
                std::thread::sleep(Duration::from_millis(POLL_MS));
                pointer_tick(&app);
            }
        })
        .ok();
}

/// Ein Takt des Zeiger-Polls.
fn pointer_tick(app: &AppHandle) {
    let s = shared(app);
    let menu = s.menu_open.load(Ordering::SeqCst);
    let (work, scale) = primary(s.desktop.as_ref());
    let mut ctl = lock(&s.dock);
    if ctl.edge == Edge::Off {
        return;
    }
    let hold = menu || ctl.held_by_drag();
    let p = ctl.place(s.desktop.as_ref());
    let handle = handle_rect(ctl.edge, work, ctl.along, scale);
    let sight = Sight {
        now: ctl.now_ms(),
        pointer: native::cursor(),
        // Der Schattenraum wird mitgezoomt.
        body: panel_body(ctl.edge, p.rect, p.scale * p.zoom),
        zone: reveal_zone(ctl.edge, handle, scale),
        margin: (INSIDE_MARGIN * scale).round() as i32,
        hold,
    };
    match ctl.auto.step(sight) {
        DockAction::Reveal => show_open(app, &s, &mut ctl),
        DockAction::Collapse => show_closed(app, &s, &mut ctl),
        DockAction::None => {}
    }
}

/// Die Oberfläche hat ihre Größe gemessen (CSS-Pixel): Fenster nachziehen.
pub fn content_size(app: &AppHandle, width: f64, height: f64) {
    let s = shared(app);
    let mut ctl = lock(&s.dock);
    ctl.content = (width.max(1.0), height.max(1.0));
    relayout(app, &s, &mut ctl);
}

/// Die Oberfläche beginnt bzw. beendet das Ziehen einer Karte oder eines Blocks: so
/// lange bleibt das Panel offen, auch wenn der Zeiger dabei das Panel verlässt (wie
/// `app._dragging()` im Dock-Poll der Python-Fassung).
pub fn set_hold(app: &AppHandle, hold: bool) {
    let s = shared(app);
    lock(&s.dock).drag_hold = hold.then(Instant::now);
}

/// Kante live wechseln (Einstellungen). Aus dem schwebenden Fenster heraus bleibt das
/// Panel offen, bis der Zeiger es verlässt; ein Wechsel zwischen Kanten behält den
/// Zustand (offen bleibt offen, zu bleibt zu).
pub fn set_edge(app: &AppHandle, edge: Edge) {
    let s = shared(app);
    let mut ctl = lock(&s.dock);
    if ctl.edge == edge {
        return;
    }
    let was_off = ctl.edge == Edge::Off;
    let expanded = ctl.auto.is_expanded();
    ctl.edge = edge;
    if edge == Edge::Off || was_off || expanded {
        ctl.auto = AutoHide::expanded();
        show_open(app, &s, &mut ctl);
    } else {
        ctl.auto = AutoHide::collapsed();
        show_closed(app, &s, &mut ctl);
    }
}

/// Ein zweiter Programmstart will das Deck sehen: aufklappen und eine Weile offen
/// halten (der Zeiger steht ja nicht darauf). Schwebend genügt nach vorn holen.
pub fn reveal_for_request(app: &AppHandle) {
    let s = shared(app);
    let mut ctl = lock(&s.dock);
    let now = ctl.now_ms();
    if ctl.edge == Edge::Off || ctl.auto.reveal_for_request(now) == DockAction::Reveal {
        show_open(app, &s, &mut ctl);
    }
}

/// Monitor der Karte für den Tooltip: Arbeitsfläche und Skalierung des Monitors, der
/// den Punkt enthält (sonst der Hauptmonitor).
pub fn monitor_at(desktop: &dyn Desktop, x: i32, y: i32) -> (Area, f64) {
    let mons = desktop.monitors();
    match mons.iter().find(|m| m.rect.contains(x, y)) {
        Some(m) => (Area::new(m.work.left, m.work.top, m.work.right, m.work.bottom), m.scale),
        None => primary(desktop),
    }
}

/// Liegt das Panel gerade offen? (Tooltips nur dann.)
pub fn is_open(app: &AppHandle) -> bool {
    lock(&shared(app).dock).open()
}

/// Gemerkte Lage entlang der Kante aus `deck_settings.json`.
pub fn saved_along(settings: &serde_json::Map<String, serde_json::Value>) -> Option<i32> {
    settings.get("dock_along").and_then(serde_json::Value::as_f64).map(|v| v as i32)
}

/// Gemerkte Lage des freien Panels (`panel_pos`: `[x, y]`).
pub fn saved_free(settings: &serde_json::Map<String, serde_json::Value>) -> Option<(i32, i32)> {
    let pos = settings.get("panel_pos")?.as_array()?;
    match pos.as_slice() {
        [x, y] => Some((x.as_f64()? as i32, y.as_f64()? as i32)),
        _ => None,
    }
}

/// Gemerkter Zoom (`panel_zoom`), in seinen Grenzen; fehlt er, 1.
pub fn saved_zoom(settings: &serde_json::Map<String, serde_json::Value>) -> f64 {
    settings
        .get("panel_zoom")
        .and_then(serde_json::Value::as_f64)
        .filter(|z| z.is_finite())
        .map_or(1.0, |z| z.clamp(ZOOM_MIN, ZOOM_MAX))
}


#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Map, Value, json};

    fn settings(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap_or_default()
    }

    #[test]
    fn the_floating_panel_comes_back_where_it_was_left() {
        assert_eq!(saved_free(&settings(json!({"panel_pos": [360, 260]}))), Some((360, 260)));
        assert_eq!(saved_free(&settings(json!({"panel_pos": [360.7, -20]}))), Some((360, -20)));
        assert_eq!(saved_free(&settings(json!({}))), None);
        assert_eq!(saved_free(&settings(json!({"panel_pos": [1]}))), None, "kaputter Wert = Vorgabe");
        assert_eq!(saved_free(&settings(json!({"panel_pos": "oben"}))), None);
    }

    #[test]
    fn a_saved_zoom_stays_within_its_bounds() {
        assert_eq!(saved_zoom(&settings(json!({"panel_zoom": 1.2}))), 1.2);
        assert_eq!(saved_zoom(&settings(json!({"panel_zoom": 9}))), ZOOM_MAX);
        assert_eq!(saved_zoom(&settings(json!({"panel_zoom": 0.1}))), ZOOM_MIN);
        assert_eq!(saved_zoom(&settings(json!({}))), 1.0);
    }
}
