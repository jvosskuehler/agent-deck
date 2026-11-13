//! Das rohe Win32-Fenster, in das der Fokus-Rahmen zeichnet ([`crate::frame_gpu_overlay`]),
//! und die Nachrichtenpumpe seines Threads.
//!
//! Ein rohes Win32-Fenster, kein Tauri-Fenster: das würde beim Einblenden nach vorn
//! geholt, und ein Rahmen, der dem Terminal den Fokus stiehlt, wäre schlimmer als
//! keiner. Fensterprozedur ist `DefWindowProcW`; den Inhalt liefert DirectComposition,
//! ein `WM_PAINT` braucht es nicht.
//!
//! Stil-Bits und warum:
//! - `WS_EX_LAYERED`: innen durchsichtig (dazu `SetLayeredWindowAttributes` beim Aufrufer).
//! - `WS_EX_TRANSPARENT`: Maus geht immer durch, auch auf dem Leuchten selbst.
//! - `WS_EX_NOACTIVATE`: wird nie aktiv, nimmt nie den Fokus.
//! - `WS_EX_TOOLWINDOW`: nicht in Taskleiste und Alt+Tab.
//! - `WS_EX_TOPMOST`: liegt über dem Terminal; gezeigt wird es nur, solange das
//!   Terminal ohnehin vorn ist, verdeckt also nichts Fremdes. Außer der Taskleiste, die
//!   ebenfalls topmost ist: darum schneidet [`clip_to_work_areas`] jedes Overlay auf die
//!   Arbeitsbereiche der Monitore zu.
//!
//! **Thread-Bindung:** Ein Fenster gehört dem Thread, der es anlegt; dessen Schleife
//! muss [`pump_messages`] rufen. Sonst hängen Rundsendungen anderer Programme
//! (`WM_SETTINGCHANGE` & Co.) bis zu ihrem Timeout an diesem Fenster.

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{CombineRgn, CreateRectRgn, DeleteObject, RGN_OR, SetWindowRgn};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, RegisterClassExW,
    TranslateMessage, WINDOW_EX_STYLE, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use crate::monitor;
use crate::types::{Error, Rect, Result};

unsafe extern "system" fn wndproc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: reine Weitergabe an die Standardbehandlung.
    unsafe { DefWindowProcW(h, msg, wp, lp) }
}

/// Ein unsichtbares, durchklickbares, nie aktives Topmost-Popup der Klasse `class`
/// anlegen (Stil-Bits siehe Modulkopf), dazu `extra` Stil-Bits.
pub(crate) fn create_click_through(class: PCWSTR, extra: WINDOW_EX_STYLE) -> Result<HWND> {
    create_layer(class, WS_EX_TOPMOST | extra)
}

/// Wie [`create_click_through`], aber ohne „immer im Vordergrund“: für ein Fenster, das
/// HINTER anderen liegen soll (der Gruppenschatten, [`crate::shadow_window`]).
pub(crate) fn create_layer(class: PCWSTR, extra: WINDOW_EX_STYLE) -> Result<HWND> {
    let ex = WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | extra;
    // SAFETY: statische Klassennamen, Prozedur eine echte extern-Funktion. Die zweite
    // Registrierung derselben Klasse scheitert harmlos (ERROR_CLASS_ALREADY_EXISTS).
    unsafe {
        let hinst = GetModuleHandleW(None).map_err(|e| Error::win("GetModuleHandleW", e))?;
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        CreateWindowExW(
            ex,
            class,
            w!("Agent Deck Fokus"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(hinst.into()),
            None,
        )
        .map_err(|e| Error::win("CreateWindowExW", e))
    }
}

/// Die Teile von `area`, die auf einem Arbeitsbereich `works` liegen (Monitor ohne
/// Taskleiste), relativ zur oberen linken Ecke von `area`. `None`: `area` liegt ganz auf
/// einem Arbeitsbereich, es gibt nichts zuzuschneiden.
pub fn work_parts(area: Rect, works: &[Rect]) -> Option<Vec<Rect>> {
    let parts: Vec<Rect> = works.iter().filter_map(|w| area.intersect(w)).collect();
    if parts.contains(&area) {
        return None;
    }
    Some(parts.into_iter().map(|p| p.offset(-area.left, -area.top)).collect())
}

/// Ein Overlay, das auf `area` (Bildschirmpixel) liegt, auf die Arbeitsbereiche der
/// Monitore zuschneiden. Fokus-Rahmen und Gruppenschatten ragen über das Agentenfenster
/// hinaus und lägen sonst auf der Taskleiste; die bleibt so immer unberührt. Bei
/// automatisch ausgeblendeter Taskleiste ist der Arbeitsbereich der ganze Monitor.
pub(crate) fn clip_to_work_areas(hwnd: HWND, area: Rect) {
    let works: Vec<Rect> = monitor::monitors().into_iter().map(|m| m.work).collect();
    // SAFETY: eigenes Fenster; die Region gehört nach erfolgreichem SetWindowRgn dem
    // System, sonst wird sie hier freigegeben.
    unsafe {
        let Some(parts) = work_parts(area, &works) else {
            SetWindowRgn(hwnd, None, false);
            return;
        };
        let rgn = CreateRectRgn(0, 0, 0, 0);
        for p in parts {
            let r = CreateRectRgn(p.left, p.top, p.right, p.bottom);
            CombineRgn(Some(rgn), Some(rgn), Some(r), RGN_OR);
            let _ = DeleteObject(r.into());
        }
        if SetWindowRgn(hwnd, Some(rgn), true) == 0 {
            let _ = DeleteObject(rgn.into());
        }
    }
}

/// Alle anstehenden Nachrichten des aufrufenden Threads abarbeiten, ohne zu warten.
pub fn pump_messages() {
    let mut msg = MSG::default();
    // SAFETY: lokale MSG-Struktur.
    unsafe {
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Rect = Rect::new(0, 0, 1920, 1032); // Taskleiste 48 px unten

    #[test]
    fn an_overlay_inside_the_work_area_is_not_clipped() {
        assert_eq!(work_parts(Rect::new(100, 100, 900, 700), &[WORK]), None);
    }

    #[test]
    fn an_overlay_reaching_into_the_taskbar_keeps_only_the_part_above_it() {
        let parts = work_parts(Rect::new(100, 600, 900, 1060), &[WORK]);
        assert_eq!(parts, Some(vec![Rect::new(0, 0, 800, 432)]));
    }

    #[test]
    fn an_overlay_across_two_monitors_keeps_both_work_areas() {
        let right = Rect::new(1920, 0, 3840, 1080); // ohne Taskleiste
        let parts = work_parts(Rect::new(1800, 900, 2000, 1060), &[WORK, right]);
        assert_eq!(parts, Some(vec![Rect::new(0, 0, 120, 132), Rect::new(120, 0, 200, 160)]));
    }
}
