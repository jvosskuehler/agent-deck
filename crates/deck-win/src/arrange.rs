//! Fremde Fenster verschieben und kacheln, Rechtecke lesen (aus `platform/arrange.py`).
//!
//! WELCHE Fenster wohin gehören (Kachelraster, Spaltenzahl), rechnet der Fachkern; hier
//! ist nur das Handwerk.

use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use std::collections::HashSet;
use std::sync::Mutex;

use windows::Win32::Graphics::Dwm::{
    DWMNCRP_DISABLED, DWMNCRP_USEWINDOWSTYLE, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DWMWA_NCRENDERING_POLICY,
    DWMWA_TRANSITIONS_FORCEDISABLED, DwmGetWindowAttribute, DwmSetWindowAttribute,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetClientRect, GetForegroundWindow, GetWindowLongPtrW, GetWindowPlacement, GetWindowRect,
    HWND_NOTOPMOST, HWND_TOPMOST, SW_MINIMIZE, SW_SHOWMINNOACTIVE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos, ShowWindow, WINDOWPLACEMENT, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT,
};

use crate::guard;
use crate::types::{Error, Hwnd, Rect, Result};
use crate::window::{class_of, is_maximized, is_minimized, pid_of, title_of, visible_top_level};

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

/// Die Fenster, die ein anderes verdecken können, von vorn nach hinten, mit ihrem
/// sichtbaren Rahmen (Taste „alle nach vorn“: liegt etwas über den Agenten?). Nicht
/// dabei: minimierte, die eigenen des Decks (Rahmen, Schatten, Panel), verborgene
/// („cloaked“: andere virtuelle Desktops, schlafende Apps), Durchklick-Ebenen und
/// alles, was „immer im Vordergrund“ ist. Über Letzteres kommt ohnehin kein Fenster,
/// es zählte also ewig als Verdecker.
pub fn solid_windows() -> Vec<(Hwnd, Rect)> {
    let own = std::process::id();
    visible_top_level()
        .into_iter()
        .filter(|&h| !is_minimized(h) && pid_of(h) != Some(own))
        .filter(|&h| {
            // SAFETY: reine Abfragen; Zeiger auf lokale Variable mit passender Größe.
            unsafe {
                let ex = GetWindowLongPtrW(h.h(), GWL_EXSTYLE) as u32;
                let mut cloaked = 0u32;
                let _ = DwmGetWindowAttribute(h.h(), DWMWA_CLOAKED, (&raw mut cloaked).cast(), size_of::<u32>() as u32);
                ex & (WS_EX_TOPMOST.0 | WS_EX_TRANSPARENT.0) == 0 && cloaked == 0
            }
        })
        .filter_map(|h| visible_rect(h).or_else(|| window_rect(h)).filter(|r| !r.is_empty()).map(|r| (h, r)))
        .collect()
}

/// Außenrechteck samt unsichtbarem Greifrand.
pub fn window_rect(hwnd: Hwnd) -> Option<Rect> {
    let mut r = RECT::default();
    // SAFETY: Zeiger auf lokale Variable.
    unsafe { GetWindowRect(hwnd.h(), &mut r) }.ok()?;
    Some(r.into())
}

/// Wo das Fenster liegt oder, ist es minimiert, wieder liegen wird (der Platz, den Windows
/// sich für das Wiederherstellen merkt). Damit hat auch ein verstecktes Fenster eine Lage.
pub fn resting_rect(hwnd: Hwnd) -> Option<Rect> {
    if !is_minimized(hwnd) {
        return window_rect(hwnd);
    }
    let mut p = WINDOWPLACEMENT { length: size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
    // SAFETY: Zeiger auf lokale Variable mit gesetzter Länge.
    unsafe { GetWindowPlacement(hwnd.h(), &mut p) }.ok()?;
    Some(p.rcNormalPosition.into())
}

/// Das Fenster über alle anderen legen, OHNE ihm die Tastatur zu geben (Taste „alle nach
/// vorn“). Kurz „immer im Vordergrund“ und gleich wieder zurück: nur so hebt Windows ein
/// fremdes Fenster über das aktive, ohne es zu aktivieren.
pub fn raise_window(hwnd: Hwnd) -> Result<()> {
    guard::check("raise_window")?;
    let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
    // SAFETY: Handles als Werte; Windows prüft sie selbst.
    unsafe {
        let _ = SetWindowPos(hwnd.h(), Some(HWND_TOPMOST), 0, 0, 0, 0, flags);
        SetWindowPos(hwnd.h(), Some(HWND_NOTOPMOST), 0, 0, 0, 0, flags).map_err(|e| Error::win("SetWindowPos", e))
    }
}

/// Die Animation beim Minimieren und Wiederherstellen für dieses Fenster abstellen. Wer
/// alle Agenten auf einmal ein- und ausblendet, will sie nicht einzeln in die Taskleiste
/// fliegen sehen. Scheitert das, bleibt eben die Animation.
fn no_transitions(hwnd: Hwnd) {
    let on: u32 = 1;
    // SAFETY: Zeiger auf einen lokalen u32, Größe passend.
    let _ = unsafe {
        DwmSetWindowAttribute(hwnd.h(), DWMWA_TRANSITIONS_FORCEDISABLED, (&raw const on).cast(), size_of::<u32>() as u32)
    };
}

/// Fenster, deren Schatten das Deck abgestellt hat ([`set_shadow`]).
///
/// Ohne Schatten zeichnet DWM nur noch die Client-Fläche des Fensters, meldet als
/// sichtbaren Rahmen aber das ganze Außenrechteck (gemessen 11.11.2026 an Windows
/// Terminal bei 150 %: außen -9..967, Client 2..956, gezeichnet 2..956; mit Schatten
/// sichtbar 0..958). Wer danach kachelt, bekäme Lücken; darum gilt für diese Fenster die
/// Client-Fläche als sichtbarer Rahmen.
static SHADOWLESS: Mutex<Option<HashSet<isize>>> = Mutex::new(None);

fn is_shadowless(hwnd: Hwnd) -> bool {
    SHADOWLESS.lock().is_ok_and(|s| s.as_ref().is_some_and(|s| s.contains(&hwnd.0)))
}

/// Die Client-Fläche in Bildschirmkoordinaten.
fn client_rect(hwnd: Hwnd) -> Option<Rect> {
    let mut r = RECT::default();
    let mut origin = POINT::default();
    // SAFETY: Zeiger auf lokale Variablen.
    unsafe {
        GetClientRect(hwnd.h(), &mut r).ok()?;
        ClientToScreen(hwnd.h(), &mut origin).ok().ok()?;
    }
    Some(Rect::new(origin.x, origin.y, origin.x + r.right, origin.y + r.bottom))
}

/// Den Schatten, den Windows dem Fenster gibt, ab- oder wieder anstellen (auch bei
/// fremden Fenstern). Mit ihm gehen der Fensterrand und die runden Ecken. `Ok(false)`:
/// DWM hat abgelehnt, das Fenster bleibt, wie es ist.
pub fn set_shadow(hwnd: Hwnd, on: bool) -> Result<bool> {
    guard::check("set_shadow")?;
    if hwnd.is_null() {
        return Ok(false);
    }
    let policy: i32 = if on { DWMNCRP_USEWINDOWSTYLE.0 } else { DWMNCRP_DISABLED.0 };
    // SAFETY: Zeiger auf einen lokalen i32, Größe passend.
    let done = unsafe {
        DwmSetWindowAttribute(hwnd.h(), DWMWA_NCRENDERING_POLICY, (&raw const policy).cast(), size_of::<i32>() as u32)
    }
    .is_ok();
    if let Ok(mut set) = SHADOWLESS.lock() {
        let set = set.get_or_insert_with(HashSet::new);
        if done && !on {
            set.insert(hwnd.0);
        } else if done {
            set.remove(&hwnd.0);
        }
    }
    Ok(done)
}

/// Der SICHTBARE Rand (DWM, ohne Greifrand). `None` ohne DWM-Auskunft.
pub fn visible_rect(hwnd: Hwnd) -> Option<Rect> {
    if hwnd.is_null() {
        return None;
    }
    if is_shadowless(hwnd) {
        return client_rect(hwnd);
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
    no_transitions(hwnd);
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
    no_transitions(hwnd);
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
