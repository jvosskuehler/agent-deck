//! Monitore, Arbeitsflächen und Platzieren auf dem richtigen Schirm (aus
//! `platform/monitor.py` und dem Bildwiederholraten-Teil von `platform/timing.py`).
//!
//! Die Arbeitsfläche ist die Monitorfläche ohne Taskleiste. Geklemmt wird immer gegen
//! die Fläche des Monitors UNTER dem Anker, nie gegen „den Bildschirm“: genau daran ist
//! in der Tk-Fassung der erste Versuch gescheitert (Tk kennt nur den Hauptmonitor und
//! zog Tooltips vom zweiten Schirm auf den ersten zurück).
//!
//! Koordinaten sind echte Pixel, siehe [`crate::dpi`].

use windows::Win32::Foundation::{LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplayMonitors, EnumDisplaySettingsW, GetMonitorInfoW,
    HDC, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITORINFO, MONITORINFOEXW, MonitorFromPoint,
    MonitorFromWindow,
};
use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;
use windows::core::{BOOL, PCWSTR};

use crate::dpi;
use crate::types::{Hwnd, Rect, from_wide};

/// Ein angeschlossener Monitor.
#[derive(Clone, Debug, PartialEq)]
pub struct Monitor {
    /// `HMONITOR` als Zahl; gilt, solange sich die Anzeige-Konfiguration nicht ändert.
    pub handle: isize,
    /// Ganze Monitorfläche.
    pub rect: Rect,
    /// Arbeitsfläche (ohne Taskleiste).
    pub work: Rect,
    /// Hauptanzeige? Ihre linke obere Ecke liegt per Definition auf (0, 0).
    pub primary: bool,
    /// Gerätename wie `\\.\DISPLAY1`.
    pub device: String,
    /// Skalierung (1.5 = 150 %).
    pub scale: f64,
    /// Bilder je Sekunde; `None`, wenn Windows nur „Hardware-Standard“ meldet.
    pub refresh_hz: Option<u32>,
}

/// Alle Monitore in der Reihenfolge, in der Windows sie aufzählt.
pub fn monitors() -> Vec<Monitor> {
    let mut handles: Vec<HMONITOR> = Vec::new();
    // SAFETY: Der LPARAM zeigt während des synchronen Aufrufs auf `handles`.
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(collect),
            LPARAM(&mut handles as *mut _ as isize),
        );
    }
    handles.into_iter().filter_map(info).collect()
}

unsafe extern "system" fn collect(h: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
    // SAFETY: siehe monitors(); der Zeiger lebt während EnumDisplayMonitors.
    let list = unsafe { &mut *(data.0 as *mut Vec<HMONITOR>) };
    list.push(h);
    BOOL::from(true)
}

/// Monitor unter dem Punkt; liegt er außerhalb aller, der nächstgelegene.
pub fn monitor_at(x: i32, y: i32) -> Option<Monitor> {
    // SAFETY: reine Abfrage.
    info(unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST) })
}

/// Monitor, auf dem der größte Teil des Fensters liegt (sonst der nächstgelegene).
pub fn monitor_of_window(hwnd: Hwnd) -> Option<Monitor> {
    if hwnd.is_null() {
        return None;
    }
    // SAFETY: reine Abfrage.
    info(unsafe { MonitorFromWindow(hwnd.h(), MONITOR_DEFAULTTONEAREST) })
}

/// Arbeitsfläche des Monitors unter (x, y). `None` heißt für Aufrufer bewusst „nicht
/// klemmen“: lieber ein Tooltip, der am Rand übersteht, als einer, den eine geratene
/// Bildschirmgröße auf den falschen Monitor zieht.
pub fn work_area(x: i32, y: i32) -> Option<Rect> {
    monitor_at(x, y).map(|m| m.work).filter(|r| !r.is_empty())
}

fn info(h: HMONITOR) -> Option<Monitor> {
    if h.0.is_null() {
        return None;
    }
    let mut mi = MONITORINFOEXW::default();
    mi.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    // SAFETY: MONITORINFOEXW beginnt mit MONITORINFO, cbSize sagt Windows die echte Größe.
    let ok = unsafe { GetMonitorInfoW(h, (&raw mut mi).cast::<MONITORINFO>()) }.as_bool();
    if !ok {
        return None;
    }
    let device = from_wide(&mi.szDevice);
    Some(Monitor {
        handle: h.0 as isize,
        rect: mi.monitorInfo.rcMonitor.into(),
        work: mi.monitorInfo.rcWork.into(),
        primary: mi.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
        refresh_hz: refresh_of(&mi.szDevice),
        device,
        scale: dpi::factor_for_monitor(h.0 as isize),
    })
}

/// Bildwiederholrate über den Gerätenamen. Bewusst nicht `DwmGetCompositionTimingInfo`:
/// dessen Struktur ist zwischen Windows-Versionen gewachsen, und ein falsches cbSize
/// gibt nur E_INVALIDARG ohne Hinweis (so in der Python-Fassung gemessen).
fn refresh_of(device: &[u16; 32]) -> Option<u32> {
    let mut dm = DEVMODEW {
        dmSize: size_of::<DEVMODEW>() as u16,
        ..Default::default()
    };
    // SAFETY: nullterminierter Gerätename aus GetMonitorInfoW, dmSize gesetzt.
    let ok =
        unsafe { EnumDisplaySettingsW(PCWSTR(device.as_ptr()), ENUM_CURRENT_SETTINGS, &mut dm) };
    plausible_hz(if ok.as_bool() {
        dm.dmDisplayFrequency
    } else {
        0
    })
}

/// 0 und 1 bedeuten bei Windows „Hardware-Standard“, also unbekannt.
pub fn plausible_hz(hz: u32) -> Option<u32> {
    (hz > 1).then_some(hz)
}

/// Position (x, y) für ein Fenster der Größe `size` am Anker, versetzt um `offset`,
/// gehalten in `area`. Ohne Fläche: Anker plus Versatz, ungeklemmt.
///
/// Je Achse: bevorzugt Anker + Versatz. Ragt das über den Rand, wird auf die andere
/// Seite des Ankers gespiegelt (ein Tooltip am rechten Rand klappt nach links vom
/// Zeiger, der Einstellungsdialog erscheint links neben einem rechts angedockten Deck).
/// Passt es auch gespiegelt nicht, an den Rand geklemmt: dann ist der Bildschirm
/// schlicht kleiner als das Fenster.
pub fn fit(
    anchor: (i32, i32),
    size: (i32, i32),
    area: Option<Rect>,
    offset: (i32, i32),
) -> (i32, i32) {
    let Some(a) = area else {
        return (anchor.0 + offset.0, anchor.1 + offset.1);
    };
    (
        axis(anchor.0, size.0, offset.0, a.left, a.right),
        axis(anchor.1, size.1, offset.1, a.top, a.bottom),
    )
}

fn axis(anchor: i32, size: i32, off: i32, lo: i32, hi: i32) -> i32 {
    let p = anchor + off;
    if p + size <= hi {
        return p.max(lo);
    }
    let mirror = anchor - off - size;
    if mirror >= lo {
        return mirror;
    }
    lo.max(hi - size)
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect::new(0, 0, 1000, 800);

    #[test]
    fn fitting_window_stays_at_anchor_plus_offset() {
        assert_eq!(fit((100, 100), (200, 50), Some(AREA), (12, 16)), (112, 116));
    }

    #[test]
    fn window_overflowing_the_right_edge_flips_to_the_left_of_the_anchor() {
        assert_eq!(
            fit((950, 100), (200, 50), Some(AREA), (12, 16)),
            (950 - 12 - 200, 116)
        );
    }

    #[test]
    fn window_bigger_than_both_sides_is_clamped_to_the_edge() {
        // Rechts kein Platz, links vom Anker auch nicht -> an den rechten Rand.
        assert_eq!(fit((100, 10), (950, 50), Some(AREA), (12, 0)).0, 50);
        // Größer als die ganze Fläche -> linker Rand gewinnt.
        assert_eq!(fit((100, 10), (2000, 50), Some(AREA), (0, 0)).0, 0);
    }

    #[test]
    fn negative_coordinates_of_a_left_monitor_are_respected() {
        let left = Rect::new(-1920, 0, 0, 1040);
        assert_eq!(
            fit((-100, 500), (300, 100), Some(left), (10, 10)),
            (-100 - 10 - 300, 510)
        );
    }

    #[test]
    fn without_area_nothing_is_clamped() {
        assert_eq!(fit((5000, -20), (300, 100), None, (10, 10)), (5010, -10));
    }

    #[test]
    fn hardware_default_refresh_rate_counts_as_unknown() {
        assert_eq!(plausible_hz(0), None);
        assert_eq!(plausible_hz(1), None);
        assert_eq!(plausible_hz(144), Some(144));
    }

    #[test]
    fn the_real_desktop_has_exactly_one_primary_monitor_at_origin() {
        // Lesend gegen das echte System.
        let all = monitors();
        assert!(!all.is_empty());
        let primary: Vec<_> = all.iter().filter(|m| m.primary).collect();
        assert_eq!(primary.len(), 1);
        assert_eq!((primary[0].rect.left, primary[0].rect.top), (0, 0));
        for m in &all {
            assert!(
                m.rect.intersect(&m.work) == Some(m.work),
                "Arbeitsfläche liegt im Monitor"
            );
        }
    }
}
