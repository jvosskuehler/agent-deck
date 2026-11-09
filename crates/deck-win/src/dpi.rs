//! DPI: wie stark ein Monitor skaliert (aus `platform/dpi.py`, nur der Win32-Teil).
//!
//! Drei Monitore mit drei Skalierungen (150 / 100 / 125 %) sind hier der Normalfall,
//! darum zählt die DPI je Monitor bzw. je Fenster, nicht die des Systems.
//!
//! Alle Koordinaten der Crate sind echte Bildschirmpixel. Das stimmt nur, wenn der
//! Prozess als Per-Monitor-V2 angemeldet ist; sonst liefert Windows virtualisierte
//! Zahlen (bei 150 % meldet derselbe Monitor 2560x1392 statt 3840x2088), und ein Fenster
//! landet irgendwo. Die Tauri-App ist das über ihr Manifest bereits; Beispiele und
//! Werkzeuge rufen [`enable_per_monitor_v2`].
//!
//! Nicht übernommen: der Oberflächenfaktor (`set_ui`, `px`, `fontpx`) und
//! `sync_tk_scaling`. Das war die Umrechnung für Tk-Canvas und Tk-Widgets; die WebView
//! skaliert CSS-Pixel selbst.

use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, GetDpiForSystem, GetDpiForWindow,
    MDT_EFFECTIVE_DPI, PROCESS_PER_MONITOR_DPI_AWARE, SetProcessDpiAwareness,
    SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::SetProcessDPIAware;

use crate::types::Hwnd;

/// 96 dpi entsprechen 100 %.
pub const BASE_DPI: u32 = 96;

/// Den Prozess als Per-Monitor-V2 anmelden. Muss vor dem ersten Fenster passieren,
/// danach steht die Einstellung fest. Drei Stufen, weil die modernste API erst ab
/// Windows 10 1703 existiert. `true`, wenn eine gegriffen hat (auch: war schon so).
pub fn enable_per_monitor_v2() -> bool {
    // SAFETY: Prozessweite Einstellung ohne Zeiger; Fehlschlag heißt nur „schon gesetzt“
    // oder „zu altes Windows“.
    unsafe {
        if SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2).is_ok() {
            return true;
        }
        if SetProcessDpiAwareness(PROCESS_PER_MONITOR_DPI_AWARE).is_ok() {
            return true;
        }
        SetProcessDPIAware().as_bool()
    }
}

/// DPI → Faktor (144 → 1.5). 0 heißt „unbekannt“ und ergibt `None`.
pub fn factor_from_dpi(dpi: u32) -> Option<f64> {
    (dpi > 0).then(|| f64::from(dpi) / f64::from(BASE_DPI))
}

/// Skalierung des Hauptmonitors; Rückfall, solange es kein Fenster gibt.
pub fn system_factor() -> f64 {
    // SAFETY: reine Abfrage.
    factor_from_dpi(unsafe { GetDpiForSystem() }).unwrap_or(1.0)
}

/// Skalierung des Monitors, auf dem `hwnd` gerade liegt. Wechselt beim Verschieben
/// zwischen unterschiedlich skalierten Monitoren; ohne Fenster die des Systems.
pub fn factor_for_window(hwnd: Hwnd) -> f64 {
    if hwnd.is_null() {
        return system_factor();
    }
    // SAFETY: reine Abfrage; ein ungültiges Handle liefert 0.
    factor_from_dpi(unsafe { GetDpiForWindow(hwnd.h()) }).unwrap_or_else(system_factor)
}

/// Skalierung eines Monitors (Handle aus [`crate::monitor`]).
pub fn factor_for_monitor(monitor: isize) -> f64 {
    let (mut x, mut y) = (0u32, 0u32);
    // SAFETY: Zeiger auf lokale Variablen.
    let ok = unsafe {
        GetDpiForMonitor(
            HMONITOR(monitor as *mut _),
            MDT_EFFECTIVE_DPI,
            &mut x,
            &mut y,
        )
        .is_ok()
    };
    if ok {
        factor_from_dpi(x).unwrap_or(1.0)
    } else {
        system_factor()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpi_maps_to_scale_factor_and_zero_is_unknown() {
        assert_eq!(factor_from_dpi(96), Some(1.0));
        assert_eq!(factor_from_dpi(144), Some(1.5));
        assert_eq!(factor_from_dpi(120), Some(1.25));
        assert_eq!(factor_from_dpi(0), None);
    }

    #[test]
    fn system_scale_is_a_plausible_factor() {
        let f = system_factor();
        assert!((0.5..=6.0).contains(&f), "{f}");
    }
}
