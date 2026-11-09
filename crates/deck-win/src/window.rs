//! Lesende Grundfragen an ein Fenster: Titel, Klasse, Prozess, Zustand, und das
//! Aufzählen der Top-Level-Fenster. Alles hier ist ohne Nebenwirkung.
//!
//! „Sichtbar“ heißt `WS_VISIBLE`, und das bleibt bei einem minimierten Fenster gesetzt.
//! Wer Fenster einsammeln oder wiederfinden will, bekommt minimierte also mit, ein
//! geschlossenes dagegen nicht mehr.

use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
    IsIconic, IsWindow, IsWindowVisible, IsZoomed,
};
use windows::core::BOOL;

use crate::types::{Hwnd, from_wide};

/// Klasse der Fenster von Windows Terminal. Verlässlicher als jeder Titel; die
/// Agentenfenster heißen zusätzlich wie ihr Slot (`--suppressApplicationTitle`).
pub const WT_WINDOW_CLASS: &str = "CASCADIA_HOSTING_WINDOW_CLASS";

/// Ein Fenster mit allem, was die App zum Wiedererkennen braucht.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowInfo {
    pub hwnd: Hwnd,
    pub title: String,
    pub class: String,
    /// Prozess-ID des Besitzers, 0 wenn Windows sie nicht herausgibt.
    pub pid: u32,
}

/// Fenstertitel, leer wenn keiner (oder das Fenster weg ist).
pub fn title_of(hwnd: Hwnd) -> String {
    if hwnd.is_null() {
        return String::new();
    }
    // SAFETY: reine Abfragen; ein ungültiges Handle liefert 0 bzw. einen leeren Puffer.
    unsafe {
        let n = GetWindowTextLengthW(hwnd.h());
        if n <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; n as usize + 1];
        let got = GetWindowTextW(hwnd.h(), &mut buf);
        from_wide(&buf[..got.max(0) as usize])
    }
}

/// Fensterklasse, leer wenn keine. Windows Terminal heißt immer [`WT_WINDOW_CLASS`].
pub fn class_of(hwnd: Hwnd) -> String {
    if hwnd.is_null() {
        return String::new();
    }
    let mut buf = [0u16; 256];
    // SAFETY: Puffergröße wird mitgegeben.
    let n = unsafe { GetClassNameW(hwnd.h(), &mut buf) };
    from_wide(&buf[..n.max(0) as usize])
}

/// Prozess-ID des Fensterbesitzers.
pub fn pid_of(hwnd: Hwnd) -> Option<u32> {
    if hwnd.is_null() {
        return None;
    }
    let mut pid = 0u32;
    // SAFETY: Zeiger auf lokale Variable.
    unsafe { GetWindowThreadProcessId(hwnd.h(), Some(&mut pid)) };
    (pid != 0).then_some(pid)
}

/// Thread, der das Fenster besitzt (0 = unbekannt).
pub(crate) fn thread_of(hwnd: HWND) -> u32 {
    // SAFETY: reine Abfrage.
    unsafe { GetWindowThreadProcessId(hwnd, None) }
}

/// Lebt das Fenster noch?
pub fn is_window(hwnd: Hwnd) -> bool {
    // SAFETY: reine Abfrage.
    !hwnd.is_null() && unsafe { IsWindow(Some(hwnd.h())) }.as_bool()
}

/// `WS_VISIBLE` gesetzt (auch bei minimierten Fenstern).
pub fn is_visible(hwnd: Hwnd) -> bool {
    // SAFETY: reine Abfrage.
    !hwnd.is_null() && unsafe { IsWindowVisible(hwnd.h()) }.as_bool()
}

pub fn is_minimized(hwnd: Hwnd) -> bool {
    // SAFETY: reine Abfrage.
    !hwnd.is_null() && unsafe { IsIconic(hwnd.h()) }.as_bool()
}

pub fn is_maximized(hwnd: Hwnd) -> bool {
    // SAFETY: reine Abfrage.
    !hwnd.is_null() && unsafe { IsZoomed(hwnd.h()) }.as_bool()
}

/// Titel, Klasse und Prozess in einem Zug; `None` für das Null-Handle.
pub fn describe(hwnd: Hwnd) -> Option<WindowInfo> {
    if hwnd.is_null() {
        return None;
    }
    Some(WindowInfo {
        hwnd,
        title: title_of(hwnd),
        class: class_of(hwnd),
        pid: pid_of(hwnd).unwrap_or(0),
    })
}

/// Alle Top-Level-Fenster in Z-Reihenfolge (vorderstes zuerst), auch unsichtbare.
pub fn top_level_windows() -> Vec<Hwnd> {
    let mut out = Vec::new();
    for_each_top_level(|h| {
        if let Some(h) = Hwnd::from_h(h) {
            out.push(h);
        }
        true
    });
    out
}

/// Alle sichtbaren Top-Level-Fenster (siehe Modulkopf zu „sichtbar“).
pub fn visible_top_level() -> Vec<Hwnd> {
    top_level_windows()
        .into_iter()
        .filter(|&h| is_visible(h))
        .collect()
}

/// `EnumWindows` mit einer Rust-Closure; gibt sie `false` zurück, ist Schluss.
pub(crate) fn for_each_top_level(mut f: impl FnMut(HWND) -> bool) {
    let mut dyn_f: &mut dyn FnMut(HWND) -> bool = &mut f;
    // SAFETY: Der LPARAM zeigt für die Dauer des (synchronen) Aufrufs auf `dyn_f`.
    // EnumWindows meldet einen Fehler, wenn die Closure abbricht; das ist gewollt.
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut dyn_f as *mut _ as isize));
    }
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: siehe for_each_top_level, der Zeiger lebt während EnumWindows.
    let f = unsafe { &mut *(lparam.0 as *mut &mut dyn FnMut(HWND) -> bool) };
    BOOL::from(f(hwnd))
}

/// Enthält `title` jede der `needles` (ohne Groß-/Kleinschreibung)? Leere Liste = ja.
pub fn contains_all(title: &str, needles: &[&str]) -> bool {
    let t = title.to_lowercase();
    needles.iter().all(|n| t.contains(&n.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needles_match_case_insensitively_and_all_must_hit() {
        assert!(contains_all(
            "agent-deck - Visual Studio Code",
            &["AGENT-DECK", "visual studio"]
        ));
        assert!(!contains_all(
            "agent-deck - Notepad",
            &["agent-deck", "Visual Studio Code"]
        ));
        assert!(contains_all("irgendwas", &[]));
        assert!(!contains_all("Größe ÄNDERN", &["grösse"]));
        assert!(contains_all("Größe ÄNDERN", &["größe", "ändern"]));
    }

    #[test]
    fn null_handle_has_no_title_class_or_process() {
        assert_eq!(title_of(Hwnd(0)), "");
        assert_eq!(class_of(Hwnd(0)), "");
        assert_eq!(pid_of(Hwnd(0)), None);
        assert_eq!(describe(Hwnd(0)), None);
        assert!(!is_window(Hwnd(0)));
    }

    #[test]
    fn enumerating_the_desktop_finds_windows_with_owners() {
        // Lesend gegen das echte System. Top-Level-Fenster gibt es in jeder Sitzung
        // (auch unsichtbare Hilfsfenster), und jedes gehört einem Prozess.
        let all = top_level_windows();
        assert!(!all.is_empty());
        assert!(all.iter().any(|&h| pid_of(h).is_some()));
    }
}
