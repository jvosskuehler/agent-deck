//! Versteckte Fenster: für den Reserve-Agenten (`deck_core::reserve`). Alles andere im Deck
//! sieht nur sichtbare Fenster ([`crate::arrange::class_windows`]); ein verstecktes
//! Agentenfenster zählt darum nirgends mit, bis das Deck es zeigt.

use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, IsWindowVisible, SW_HIDE, SW_SHOWNA, ShowWindow};
use windows::core::HSTRING;

use crate::guard;
use crate::types::{Hwnd, Result};

/// Das Fenster mit genau dieser Klasse und diesem Titel, ob sichtbar oder nicht.
pub fn find_window(class: &str, title: &str) -> Option<Hwnd> {
    // SAFETY: zwei gültige, nullterminierte Zeichenketten; reine Abfrage.
    let h = unsafe { FindWindowW(&HSTRING::from(class), &HSTRING::from(title)) }.ok()?;
    let hwnd = Hwnd(h.0 as isize);
    (!hwnd.is_null()).then_some(hwnd)
}

/// Verstecken (`SW_HIDE`: auch aus Taskleiste und Alt+Tab) oder zeigen, ohne zu aktivieren.
/// `Ok(true)` = war vorher anders.
pub fn set_visible(hwnd: Hwnd, visible: bool) -> Result<bool> {
    guard::check("set_visible")?;
    // SAFETY: Handle als Wert; Windows prüft es selbst.
    unsafe {
        let was = IsWindowVisible(hwnd.h()).as_bool();
        if was != visible {
            let _ = ShowWindow(hwnd.h(), if visible { SW_SHOWNA } else { SW_HIDE });
        }
        Ok(was != visible)
    }
}
