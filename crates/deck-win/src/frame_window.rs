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
//!   Terminal ohnehin vorn ist, verdeckt also nichts Fremdes.
//!
//! **Thread-Bindung:** Ein Fenster gehört dem Thread, der es anlegt; dessen Schleife
//! muss [`pump_messages`] rufen. Sonst hängen Rundsendungen anderer Programme
//! (`WM_SETTINGCHANGE` & Co.) bis zu ihrem Timeout an diesem Fenster.

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, RegisterClassExW,
    TranslateMessage, WINDOW_EX_STYLE, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use crate::types::{Error, Result};

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
