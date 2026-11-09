//! Fremde Fenster verschieben, kacheln, auf einem Monitor maximieren, schließen, und
//! Rechteck und Zustand lesen (aus `platform/arrange.py`).
//!
//! WELCHE Fenster wohin gehören (Kachelraster, Spaltenzahl, Zielrechteck beim
//! Einsammeln), rechnet der Fachkern; hier ist nur das Handwerk.
//!
//! **Ein maximiertes Fenster klebt an seinem Monitor** (gemessen 2026-08-19): ein
//! einzelnes `SetWindowPlacement` mit `SW_SHOWMAXIMIZED` und einem Normalrechteck auf
//! dem Zielmonitor meldet Erfolg, und das Fenster bleibt trotzdem, wo es war. Windows
//! hält für ein maximiertes Fenster den Maximierungs-Monitor fest. Darum erst
//! `SW_SHOWNORMAL` auf die Zielfläche (zieht wirklich um), dann maximieren.

use windows::Win32::Foundation::{CloseHandle, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_STYLE, GetWindowLongW, GetWindowPlacement, GetWindowRect, PostMessageW, SW_SHOWMAXIMIZED,
    SW_SHOWNOACTIVATE, SW_SHOWNORMAL, SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPlacement,
    SetWindowPos, ShowWindow, WINDOWPLACEMENT, WM_CLOSE,
};
use windows::core::PWSTR;

use crate::guard;
use crate::types::{Error, Hwnd, Rect, Result, from_wide};
use crate::window::{class_of, is_maximized, is_minimized, pid_of, title_of, visible_top_level};

/// Ein sichtbares Top-Level-Fenster mit Titel, so weit das Einsammeln es braucht.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TopWindow {
    pub hwnd: Hwnd,
    pub title: String,
    /// Voller Pfad der Exe; `None` = nicht ermittelbar (Dienste, höher berechtigte
    /// Programme). Der Fachkern fällt dann auf den Titel zurück.
    pub exe: Option<String>,
    /// Fensterstil-Bits (`GWL_STYLE`), vorzeichenlos.
    pub style: u32,
}

/// Ein Fenster einer bestimmten Klasse (Windows Terminal).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassWindow {
    pub hwnd: Hwnd,
    pub title: String,
    pub minimized: bool,
}

/// Anzeigezustand eines Fensters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShowState {
    Normal,
    Minimized,
    Maximized,
}

/// Lage und Zustand eines Fensters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    pub state: ShowState,
    /// Außenrechteck im aktuellen Zustand (samt unsichtbarem Greifrand).
    pub rect: Rect,
    /// Rechteck nach einem „Wiederherstellen“, auch während maximiert/minimiert.
    pub normal: Rect,
}

/// Unsichtbarer Greifrand um ein Fenster (links, oben, rechts, unten).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Insets {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// Alle sichtbaren Top-Level-Fenster mit Titel, samt Exe-Pfad und Stil. Minimierte sind
/// dabei, die will man beim Einsammeln ja mitnehmen.
pub fn visible_windows() -> Vec<TopWindow> {
    visible_top_level()
        .into_iter()
        .filter_map(|hwnd| {
            let title = title_of(hwnd);
            if title.is_empty() {
                return None;
            }
            // SAFETY: reine Abfrage. Als u32 gelesen, sonst käme ein gesetztes oberstes
            // Bit als negative Zahl an.
            let style = unsafe { GetWindowLongW(hwnd.h(), GWL_STYLE) } as u32;
            Some(TopWindow {
                hwnd,
                title,
                exe: exe_of(hwnd),
                style,
            })
        })
        .collect()
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

/// Voller Pfad der Exe hinter dem Fenster.
pub fn exe_of(hwnd: Hwnd) -> Option<String> {
    let pid = pid_of(hwnd)?;
    // SAFETY: Handle wird geschlossen; Puffergröße wird mitgegeben.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        let ok =
            QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut size)
                .is_ok();
        let _ = CloseHandle(h);
        ok.then(|| from_wide(&buf[..size as usize]))
    }
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

/// Zustand, Außenrechteck und Normalrechteck.
pub fn placement(hwnd: Hwnd) -> Option<Placement> {
    let wp = get_placement(hwnd)?;
    let state = if is_minimized(hwnd) {
        ShowState::Minimized
    } else if is_maximized(hwnd) {
        ShowState::Maximized
    } else {
        ShowState::Normal
    };
    Some(Placement {
        state,
        rect: window_rect(hwnd)?,
        normal: wp.rcNormalPosition.into(),
    })
}

/// Größe (Breite, Höhe) des NICHT maximierten Fensters, auch während es maximiert oder
/// minimiert ist. `GetWindowRect` wäre hier die Größe des aktuellen Zustands.
pub fn normal_size(hwnd: Hwnd) -> Option<(i32, i32)> {
    let r: Rect = get_placement(hwnd)?.rcNormalPosition.into();
    Some((r.width(), r.height()))
}

fn get_placement(hwnd: Hwnd) -> Option<WINDOWPLACEMENT> {
    let mut wp = WINDOWPLACEMENT {
        length: size_of::<WINDOWPLACEMENT>() as u32,
        ..Default::default()
    };
    // SAFETY: length gesetzt, Zeiger auf lokale Variable.
    unsafe { GetWindowPlacement(hwnd.h(), &mut wp) }.ok()?;
    Some(wp)
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

/// Fenster auf `normal` legen und dort maximieren; `normal` bestimmt über seinen
/// Monitor, WO maximiert wird. Arbeitet auch am minimierten Fenster und holt es nicht
/// ungefragt nach vorn. Die zwei Schritte sind Pflicht, siehe Modulkopf.
pub fn move_maximized(hwnd: Hwnd, normal: Rect) -> Result<()> {
    guard::check("move_maximized")?;
    let mut wp = get_placement(hwnd).ok_or_else(|| Error::last("GetWindowPlacement"))?;
    wp.rcNormalPosition = normal.to_win();
    // SAFETY: vollständig ausgefüllte Struktur aus GetWindowPlacement.
    unsafe {
        wp.showCmd = SW_SHOWNORMAL.0 as u32; // 1. entmaximieren: zieht wirklich um
        SetWindowPlacement(hwnd.h(), &wp).map_err(|e| Error::win("SetWindowPlacement", e))?;
        wp.showCmd = SW_SHOWMAXIMIZED.0 as u32; // 2. auf dem Zielmonitor maximieren
        SetWindowPlacement(hwnd.h(), &wp).map_err(|e| Error::win("SetWindowPlacement", e))
    }
}

/// Ein Fenster zum Schließen auffordern (`WM_CLOSE`, wie Alt+F4). Bewusst
/// `PostMessage`: `SendMessage` wartet auf das fremde Fenster und hängt genau dann, wenn
/// es hängt. Bewusst kein `TerminateProcess`: VS Code fragt so selbst nach
/// ungespeicherten Dateien. `Ok` heißt darum nur „gefragt“; ob es zu ist, zeigt danach
/// die Prozessliste.
pub fn post_close(hwnd: Hwnd) -> Result<()> {
    guard::check("post_close")?;
    // SAFETY: Handle als Wert, Nachricht ohne Zeiger.
    unsafe { PostMessageW(Some(hwnd.h()), WM_CLOSE, WPARAM(0), LPARAM(0)) }
        .map_err(|e| Error::win("PostMessageW(WM_CLOSE)", e))
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
    fn moving_and_closing_are_blocked_in_tests() {
        let r = Rect::new(0, 0, 10, 10);
        assert!(matches!(place_window(Hwnd(1), r), Err(Error::Forbidden(_))));
        assert!(matches!(
            move_maximized(Hwnd(1), r),
            Err(Error::Forbidden(_))
        ));
        assert!(matches!(post_close(Hwnd(1)), Err(Error::Forbidden(_))));
    }

    #[test]
    fn listing_visible_windows_only_returns_titled_ones() {
        // Lesend gegen das echte System.
        assert!(visible_windows().iter().all(|w| !w.title.is_empty()));
    }
}
