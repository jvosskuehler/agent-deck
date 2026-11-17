//! Das Kommentar-Fenster (Entwurf „N1“): erscheint, wenn ein Mensch auf einem eigenen PR
//! kommentiert hat und das Deck diesen Kommentar noch nicht gemeldet hat
//! (`DashView.popup`, `deck_core::dash::feed`). Inhalt `#/comment`, gezeichnet aus
//! `deck://dash` wie das Info-Fenster.
//!
//! Nie aktiv (nimmt keinen Fokus), nicht „immer oben“: es liegt unten rechts auf dem
//! Monitor des vordersten Agentenfensters, in der Z-Ordnung direkt darüber
//! (`native::show_above`). Holt jemand einen Browser über die Agenten, liegt es mit den
//! Agenten dahinter; kommt ein anderer Agent nach vorn, rückt es über ihn.
//! Weg ist es, sobald `popup` leer ist (×, „Später“, „Öffnen“ melden den Kommentar als
//! gesehen).

use std::sync::Mutex;

use deck_core::dock::Area;
use deck_win::effects::Desktop;
use deck_win::{Hwnd, Rect};
use tauri::AppHandle;

use crate::applog::note;
use crate::native;
use crate::ui_windows::{self, COMMENT};

/// Größe in CSS-Pixeln (wie der Entwurf).
const SIZE: (f64, f64) = (420.0, 190.0);
/// Abstand zum Rand der Arbeitsfläche (CSS-Pixel).
const MARGIN: f64 = 16.0;

/// Gezeigt auf dieser Fläche über diesem Agenten; `None` = versteckt.
static SHOWN: Mutex<Option<(Area, Hwnd)>> = Mutex::new(None);

/// Unten rechts in der Arbeitsfläche, in physischen Pixeln.
fn place(work: Rect, scale: f64) -> Area {
    let (w, h) = ((SIZE.0 * scale).round() as i32, (SIZE.1 * scale).round() as i32);
    let m = (MARGIN * scale).round() as i32;
    Area::new(work.right - m - w, work.bottom - m - h, work.right - m, work.bottom - m)
}

/// Das vorderste Agentenfenster (Windows Terminal).
fn front_agent(desktop: &dyn Desktop) -> Option<Hwnd> {
    let agents: Vec<Hwnd> = desktop.class_windows(deck_core::wt::WINDOW_CLASS).into_iter().filter(|w| !w.minimized).map(|w| w.hwnd).collect();
    desktop.z_order().into_iter().find(|h| agents.contains(h))
}

/// Liegt `ours` direkt über `agent`, ohne dass dazwischen etwas unsere Fläche verdeckt?
fn right_above(desktop: &dyn Desktop, ours: Hwnd, agent: Hwnd, r: Area) -> bool {
    let z = desktop.z_order();
    let (Some(o), Some(a)) = (z.iter().position(|h| *h == ours), z.iter().position(|h| *h == agent)) else { return false };
    let rect = Rect::new(r.left, r.top, r.right, r.bottom);
    o < a && z[o + 1..a].iter().all(|h| desktop.window_rect(*h).is_none_or(|w| !overlaps(w, rect)))
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

/// Je Takt: zeigen, nachziehen oder verstecken.
pub fn sync(app: &AppHandle, desktop: &dyn Desktop, show: bool) {
    let raw = ui_windows::raw_hwnd(app, COMMENT);
    if raw == 0 {
        return;
    }
    let Ok(mut shown) = SHOWN.lock() else { return };
    if !show {
        if shown.take().is_some() {
            native::hide(raw);
        }
        return;
    }
    let agent = front_agent(desktop);
    let (work, scale) = match agent {
        Some(a) => {
            let Some((cx, cy)) = desktop.window_rect(a).map(|r| r.center()) else { return };
            (desktop.work_area(cx, cy), desktop.scale_of(a))
        }
        None => (desktop.monitors().into_iter().find(|m| m.primary).map(|m| m.work), 1.0),
    };
    let Some(work) = work else { return };
    let want = place(work, scale);
    let agent_h = agent.unwrap_or(Hwnd(0));
    let placed = native::rect_of(raw) == Some(want);
    if *shown == Some((want, agent_h)) && placed && native::is_visible(raw) && (agent.is_none() || right_above(desktop, Hwnd(raw), agent_h, want)) {
        return;
    }
    native::show_above(raw, want, agent_h.0);
    // Wechsel auf einen Monitor mit anderer Skalierung: Tauri rechnet die Größe um
    // (wie beim Info-Fenster, `dash_window::settle`), also gleich noch einmal legen.
    if native::rect_of(raw).is_some_and(|now| now != want) {
        native::show_above(raw, want, agent_h.0);
    }
    if shown.is_none() {
        note(format!("Kommentar-Fenster gezeigt, {}×{}", want.width(), want.height()));
    }
    *shown = Some((want, agent_h));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_sits_bottom_right_with_a_margin_scaled_to_the_monitor() {
        let a = place(Rect::new(0, 0, 3840, 2112), 1.5);
        assert_eq!((a.left, a.top, a.right, a.bottom), (3840 - 24 - 630, 2112 - 24 - 285, 3816, 2088));
        let b = place(Rect::new(-1920, 0, 0, 1040), 1.0);
        assert_eq!((b.width(), b.height(), b.right), (420, 190, -16));
    }
}
