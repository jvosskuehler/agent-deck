//! Fremde Fenster verschieben und kacheln, Rechtecke lesen (aus `platform/arrange.py`).
//!
//! WELCHE Fenster wohin gehören (Kachelraster, Spaltenzahl), rechnet der Fachkern; hier
//! ist nur das Handwerk.

use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowRect, SW_MINIMIZE, SW_SHOWMINNOACTIVE, SW_SHOWNOACTIVATE,
    SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos, ShowWindow,
};

use crate::guard;
use crate::types::{Error, Hwnd, Rect, Result};
use crate::window::{class_of, is_maximized, is_minimized, title_of, visible_top_level};

/// Ein Fenster einer bestimmten Klasse (Windows Terminal).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassWindow {
    pub hwnd: Hwnd,
    pub title: String,
    pub minimized: bool,
}

/// Unsichtbarer Greifrand um ein Fenster (links, oben, rechts, unten).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Insets {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// Sichtbare Fenster der Klasse `class` (auch minimierte).
pub fn class_windows(class: &str) -> Vec<ClassWindow> {
    visible_top_level()
        .into_iter()
        .filter(|&h| class_of(h) == class)
        .map(|hwnd| ClassWindow {
            hwnd,
            title: title_of(hwnd),
            minimized: is_minimized(hwnd),
        })
        .collect()
}

/// Außenrechteck samt unsichtbarem Greifrand.
pub fn window_rect(hwnd: Hwnd) -> Option<Rect> {
    let mut r = RECT::default();
    // SAFETY: Zeiger auf lokale Variable.
    unsafe { GetWindowRect(hwnd.h(), &mut r) }.ok()?;
    Some(r.into())
}

/// Der SICHTBARE Rand (DWM, ohne Greifrand). `None` ohne DWM-Auskunft.
pub fn visible_rect(hwnd: Hwnd) -> Option<Rect> {
    if hwnd.is_null() {
        return None;
    }
    let mut r = RECT::default();
    // SAFETY: Zeiger und Größe passen zu RECT.
    unsafe {
        DwmGetWindowAttribute(
            hwnd.h(),
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&raw mut r).cast(),
            size_of::<RECT>() as u32,
        )
    }
    .ok()?;
    Some(r.into())
}

/// Wie weit das Außenrechteck über den sichtbaren Rahmen hinausragt. Wer das nicht
/// abzieht, bekommt zwischen zwei Kacheln eine Lücke. Ohne DWM-Auskunft: kein Rand.
pub fn frame_insets(hwnd: Hwnd) -> Insets {
    match (window_rect(hwnd), visible_rect(hwnd)) {
        (Some(outer), Some(vis)) => insets_between(outer, vis),
        _ => Insets::default(),
    }
}

/// Rand zwischen Außen- und sichtbarem Rechteck.
pub fn insets_between(outer: Rect, visible: Rect) -> Insets {
    Insets {
        left: visible.left - outer.left,
        top: visible.top - outer.top,
        right: outer.right - visible.right,
        bottom: outer.bottom - visible.bottom,
    }
}

/// Außenrechteck, dessen sichtbarer Rahmen genau `target` füllt.
pub fn outer_for_visible(target: Rect, insets: Insets) -> Rect {
    Rect::new(
        target.left - insets.left,
        target.top - insets.top,
        target.right + insets.right,
        target.bottom + insets.bottom,
    )
}

/// Fenster so legen, dass sein SICHTBARER Rahmen genau `target` füllt, ohne es nach
/// vorn zu holen (Kacheln der Agentenfenster). Ein maximiertes wird erst
/// wiederhergestellt (sonst klebt es). Gelegt wird ZWEIMAL: zieht das Fenster auf einen
/// Monitor mit anderer Skalierung, passt es beim ersten Mal seine Größe an die neue DPI
/// an, erst der zweite Aufruf trifft die Kachel.
pub fn place_window(hwnd: Hwnd, target: Rect) -> Result<()> {
    guard::check("place_window")?;
    // SAFETY: Handles als Werte; Windows prüft sie selbst.
    unsafe {
        if is_maximized(hwnd) {
            let _ = ShowWindow(hwnd.h(), SW_SHOWNOACTIVATE);
        }
        let mut last = Ok(());
        for _ in 0..2 {
            let o = outer_for_visible(target, frame_insets(hwnd));
            last = SetWindowPos(
                hwnd.h(),
                None,
                o.left,
                o.top,
                o.width(),
                o.height(),
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
            .map_err(|e| Error::win("SetWindowPos", e));
        }
        last
    }
}

/// Ein minimiertes Fenster wiederherstellen, ohne es nach vorn zu holen (Taste „alle
/// nach vorn“). Ein nicht minimiertes bleibt, wie es ist. `Ok(true)` = es war minimiert.
pub fn restore_window(hwnd: Hwnd) -> Result<bool> {
    guard::check("restore_window")?;
    if !is_minimized(hwnd) {
        return Ok(false);
    }
    // SAFETY: Handle als Wert; Windows prüft es selbst.
    let _ = unsafe { ShowWindow(hwnd.h(), SW_SHOWNOACTIVATE) };
    Ok(true)
}

/// Ein Fenster minimieren (Taste „alle nach vorn“, zweiter Druck). `Ok(true)` = es lag
/// offen. Liegt es im Hintergrund, bleibt das aktive Fenster aktiv
/// (`SW_SHOWMINNOACTIVE`). Ist es selbst das aktive, reicht Windows die Tastatur an das
/// nächste Fenster weiter (`SW_MINIMIZE`); sonst tippte man in ein verstecktes Terminal.
pub fn minimize_window(hwnd: Hwnd) -> Result<bool> {
    guard::check("minimize_window")?;
    if is_minimized(hwnd) {
        return Ok(false);
    }
    // SAFETY: Handle als Wert; Windows prüft es selbst.
    unsafe {
        let cmd = if GetForegroundWindow() == hwnd.h() { SW_MINIMIZE } else { SW_SHOWMINNOACTIVE };
        let _ = ShowWindow(hwnd.h(), cmd);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invisible_border_is_added_back_so_visible_frames_touch() {
        let outer = Rect::new(-7, 0, 1007, 807);
        let vis = Rect::new(0, 0, 1000, 800);
        let ins = insets_between(outer, vis);
        assert_eq!(
            ins,
            Insets {
                left: 7,
                top: 0,
                right: 7,
                bottom: 7
            }
        );
        assert_eq!(
            outer_for_visible(Rect::new(500, 0, 1000, 400), ins),
            Rect::new(493, 0, 1007, 407)
        );
    }

    #[test]
    fn without_dwm_answer_there_is_no_border() {
        assert_eq!(frame_insets(Hwnd(0)), Insets::default());
        assert_eq!(visible_rect(Hwnd(0)), None);
    }

    #[test]
    fn moving_and_restoring_are_blocked_in_tests() {
        let r = Rect::new(0, 0, 10, 10);
        assert!(matches!(place_window(Hwnd(1), r), Err(Error::Forbidden(_))));
        assert!(matches!(restore_window(Hwnd(1)), Err(Error::Forbidden(_))));
    }
}
