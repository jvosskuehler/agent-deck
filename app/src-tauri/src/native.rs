//! Die eigenen Fenster ohne Umweg über Tauri zeigen, legen und verbergen.
//!
//! Warum nicht `show()`/`set_position()`: Tauri zeigt ein Fenster mit `SW_SHOW` und
//! aktiviert es damit. Das Panel klappt aber auf, wenn der Zeiger den Griff streift;
//! dabei darf es dem Editor nicht den Tastaturfokus nehmen. `SetWindowPos` mit
//! `SWP_NOACTIVATE` zeigt und legt in einem Schritt, in physischen Pixeln, immer oben.
//!
//! Nur für Panel, Griff und Tooltip, und nur für Fenster dieses Prozesses. Trotzdem hält
//! sich alles an das Schutzgitter: in Tests passiert nichts.

use deck_core::dock::Area;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, HWND_TOPMOST, IsIconic, IsWindowVisible, SM_SWAPBUTTON, SW_HIDE,
    SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_SHOWWINDOW, SetWindowPos, ShowWindow,
};

fn hwnd(raw: isize) -> HWND {
    HWND(raw as *mut core::ffi::c_void)
}

fn allowed() -> bool {
    !cfg!(test) && !deck_win::effects_forbidden()
}

/// Fenster auf `rect` legen und zeigen, ohne es zu aktivieren.
pub fn show_at(raw: isize, rect: Area) {
    place(raw, rect, true);
}

/// Fenster auf `rect` legen (sichtbar bleibt es, wie es war).
pub fn move_to(raw: isize, rect: Area) {
    place(raw, rect, false);
}

fn place(raw: isize, r: Area, show: bool) {
    if !allowed() || raw == 0 {
        return;
    }
    let mut flags = SWP_NOACTIVATE | SWP_NOOWNERZORDER;
    if show {
        flags |= SWP_SHOWWINDOW;
        // Ein minimiertes Fenster (Win+M, „Alle Fenster minimieren“) liegt bei -32000 und
        // bleibt dort: `SetWindowPos` holt es nicht zurück, der Griff wäre für immer
        // unsichtbar (10.11.2026). Erst wiederherstellen, ohne zu aktivieren.
        // SAFETY: eigenes Fenster.
        if unsafe { IsIconic(hwnd(raw)) }.as_bool() {
            let _ = unsafe { ShowWindow(hwnd(raw), SW_SHOWNOACTIVATE) };
        }
    }
    // SAFETY: eigenes Fenster; SetWindowPos liest nur die übergebenen Werte.
    let _ = unsafe { SetWindowPos(hwnd(raw), Some(HWND_TOPMOST), r.left, r.top, r.width().max(1), r.height().max(1), flags) };
}

pub fn hide(raw: isize) {
    if !allowed() || raw == 0 {
        return;
    }
    // SAFETY: eigenes Fenster.
    let _ = unsafe { ShowWindow(hwnd(raw), SW_HIDE) };
}

pub fn is_visible(raw: isize) -> bool {
    // SAFETY: reine Abfrage.
    raw != 0 && unsafe { IsWindowVisible(hwnd(raw)) }.as_bool()
}

/// Zeigerposition in physischen Pixeln (die App ist Per-Monitor-V2).
pub fn cursor() -> Option<(i32, i32)> {
    let mut p = POINT::default();
    // SAFETY: schreibt nur in `p`.
    unsafe { GetCursorPos(&mut p) }.ok().map(|_| (p.x, p.y))
}

/// Ist die primäre Maustaste gedrückt? (`GetAsyncKeyState` meldet physische Tasten: bei
/// vertauschten Tasten ist die primäre die rechte.)
pub fn primary_button_down() -> bool {
    // SAFETY: reine Abfragen.
    let swapped = unsafe { GetSystemMetrics(SM_SWAPBUTTON) } != 0;
    let key = if swapped { VK_RBUTTON } else { VK_LBUTTON };
    // SAFETY: reine Abfrage; das oberste Bit heißt „gerade gedrückt“.
    (unsafe { GetAsyncKeyState(i32::from(key.0)) } as u16 & 0x8000) != 0
}
