//! Das Info-Fenster: die Info-Karten in der freien Kachel neben den Agentenfenstern.
//!
//! Liegen 5 Agenten als 3 + 2 auf dem Monitor, bleibt rechts unten eine Kachel frei
//! (`wt_gap.rs`). Dort steht dieses Fenster, so groß wie die Kachel, ohne Rahmen, nie
//! aktiv. In der Z-Ordnung liegt es direkt hinter dem vordersten Agentenfenster: holt
//! jemand einen Browser nach vorn, verschwindet es mit den Agenten dahinter; klickt er
//! einen Agenten an, kommt es mit nach vorn.
//!
//! Gibt es keine Lücke (Raster voll, Fenster von Hand verschoben, Karten abgeschaltet),
//! ist es versteckt. Die Karten selbst kommen wie beim Panel mit `deck://view`.
//!
//! Für diesen Fall gibt es die Info-Taste (`hotkey::start_dash`): sie klappt das Fenster
//! mitten auf dem Monitor unter dem Zeiger auf, immer oben, so groß wie eine Kachel. Die
//! Lücke ist dann egal. Nochmal die Taste oder das × oben rechts (`closePopup`) klappt es
//! wieder zu; danach gilt wieder die Lücke.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use deck_core::dock::Area;
use deck_win::{Hwnd, Rect};
use deck_win::effects::Desktop;
use tauri::{AppHandle, Emitter};

use crate::applog::note;
use crate::native;
use crate::ui_windows::{self, DASH};
use crate::wt_gap::Gap;

/// Zuletzt gesetzte Lage; `None` = versteckt.
static SHOWN: Mutex<Option<Gap>> = Mutex::new(None);

/// Per Info-Taste aufgeklappt: dann legt [`sync`] das Fenster nicht an die Lücke.
static POPUP: AtomicBool = AtomicBool::new(false);
/// Wohin die Info-Taste das Fenster gelegt hat.
static POPUP_AT: Mutex<Option<Area>> = Mutex::new(None);

/// Info-Taste: aufklappen oder, wenn schon auf, wieder zu.
pub fn toggle(app: &AppHandle) {
    if POPUP.load(Ordering::SeqCst) {
        close_popup(app);
    } else {
        open_popup(app);
    }
}

fn open_popup(app: &AppHandle) {
    let raw = ui_windows::raw_hwnd(app, DASH);
    let Some((x, y)) = native::cursor() else { return };
    let Some(area) = crate::state::shared(app).desktop.work_area(x, y) else { return };
    if raw == 0 {
        return;
    }
    POPUP.store(true, Ordering::SeqCst);
    if let Ok(mut shown) = SHOWN.lock() {
        *shown = None;
    }
    let r = popup_rect(area);
    if let Ok(mut at) = POPUP_AT.lock() {
        *at = Some(r);
    }
    native::show_at(raw, r);
    settle(raw, r, |raw, r| native::show_at(raw, r));
    let _ = app.emit("deck://dash-popup", true);
    note(format!("Info-Fenster: per Taste aufgeklappt, {}×{}", r.width(), r.height()));
}

/// Info-Taste nochmal oder × im Fenster: zuklappen; der nächste Takt legt es wieder an
/// die Lücke, falls es eine gibt.
pub fn close_popup(app: &AppHandle) {
    if !POPUP.swap(false, Ordering::SeqCst) {
        return;
    }
    native::hide(ui_windows::raw_hwnd(app, DASH));
    let _ = app.emit("deck://dash-popup", false);
    note("Info-Fenster: zugeklappt");
    crate::poll::wake();
}

/// So groß wie eine Kachel im Raster 3 × 2 (dieselbe Fläche wie die Lücke), mitten im
/// Arbeitsbereich.
fn popup_rect(area: Rect) -> Area {
    let (w, h) = (area.width() / 3, area.height() / 2);
    let left = area.left + (area.width() - w) / 2;
    let top = area.top + (area.height() - h) / 2;
    Area::new(left, top, left + w, top + h)
}

/// Das Info-Fenster an die Lücke `gap` legen oder verstecken. Fasst das Fenster nur an,
/// wenn sich Lage oder Nachbar in der Z-Ordnung geändert haben.
pub fn sync(app: &AppHandle, desktop: &dyn Desktop, gap: Option<Gap>) {
    let raw = ui_windows::raw_hwnd(app, DASH);
    if raw == 0 {
        return;
    }
    if POPUP.load(Ordering::SeqCst) {
        if let Some(r) = POPUP_AT.lock().ok().and_then(|a| *a)
            && native::rect_of(raw).is_some_and(|now| now != r)
        {
            native::show_at(raw, r);
            note("Info-Fenster: Größe nach Wechsel der Skalierung nachgezogen");
        }
        return;
    }
    let Ok(mut shown) = SHOWN.lock() else { return };
    let Some(g) = gap else {
        if shown.take().is_some() {
            native::hide(raw);
            note("Info-Fenster: keine freie Kachel, versteckt");
        }
        return;
    };
    let r = g.rect;
    let want = Area::new(r.left, r.top, r.right, r.bottom);
    let placed = native::rect_of(raw) == Some(want);
    if *shown == Some(g) && placed && native::is_visible(raw) && right_behind(desktop, Hwnd(raw), &g) {
        return;
    }
    if *shown == Some(g) && !placed {
        note("Info-Fenster: Größe nach Wechsel der Skalierung nachgezogen");
    }
    native::show_behind(raw, want, g.above.0);
    settle(raw, want, |raw, r| native::show_behind(raw, r, g.above.0));
    if shown.is_none_or(|s| s.rect != g.rect) {
        note(format!("Info-Fenster: in der freien Kachel {}×{} bei {},{}", r.width(), r.height(), r.left, r.top));
    }
    *shown = Some(g);
}

/// Liegt `ours` hinter `above`, ohne dass dazwischen etwas über der Lücke liegt? Andere
/// Fenster dazwischen stören nicht, solange sie die Lücke nicht berühren (die Statusringe
/// der Agenten etwa liegen ebenfalls direkt hinter ihrem Fenster).
fn right_behind(desktop: &dyn Desktop, ours: Hwnd, g: &Gap) -> bool {
    let z = desktop.z_order();
    let (Some(a), Some(o)) = (z.iter().position(|h| *h == g.above), z.iter().position(|h| *h == ours)) else {
        return false;
    };
    a < o && z[a + 1..o].iter().all(|h| desktop.window_rect(*h).is_none_or(|r| !overlaps(r, g.rect)))
}

/// Zieht das Fenster beim Legen auf einen Monitor mit anderer Skalierung, rechnet Windows
/// (bzw. Tauri auf `WM_DPICHANGED`) seine Größe um den Faktor der Skalierungen um, damit es
/// gleich groß aussieht: von 150 % auf 100 % wird es zwei Drittel so groß, zurück anderthalb
/// mal so groß und ragt weit über die Kachel hinaus (17.11.2026: „verzerrt, geht weit nach
/// unten“). Darum gleich noch einmal legen, wenn es nicht passt; was danach noch kommt,
/// fängt der nächste Takt von [`sync`] ab, der die echte Lage vergleicht.
fn settle(raw: isize, want: Area, place: impl Fn(isize, Area)) {
    if native::rect_of(raw).is_some_and(|now| now != want) {
        place(raw, want);
    }
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_popup_is_one_tile_in_the_middle_of_the_work_area() {
        let r = popup_rect(Rect::new(0, 0, 3840, 2112));
        assert_eq!((r.left, r.top, r.width(), r.height()), (1280, 528, 1280, 1056));
        let second = popup_rect(Rect::new(-1920, 0, 0, 1040));
        assert_eq!((second.left, second.top, second.width(), second.height()), (-1280, 260, 640, 520));
    }
}
