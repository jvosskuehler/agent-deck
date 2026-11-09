//! Alle VS-Code-Fenster auf den Hauptmonitor holen und dort maximieren (Python
//! `ops/window_gather.py`).
//!
//! Für den Arbeitsplatzwechsel: hängt der Laptop wieder an der Dockingstation, liegen
//! die Fenster oft alle auf dem kleinen Schirm. Zielmonitor ist der WINDOWS-
//! HAUPTBILDSCHIRM: der einzige, den das System selbst als „den einen“ benennt; jede
//! andere Wahl wäre geraten und beim nächsten Wechsel anders. Welche Fenster dazugehören
//! und wohin genau, rechnet `deck_core::arrange`; der Umzug in zwei Schritten (erst
//! normal auf die Zielfläche, dann maximieren) steckt in `deck_win::arrange`.

use deck_core::arrange::{is_editor_window, is_maximizable, normal_rect};
use deck_core::i18n::Lang;
use deck_win::Rect;
use deck_win::arrange::TopWindow;
use deck_win::effects::Desktop;

/// Bilanz für die Statuszeile des Dialogs: ein Zahlendreher wäre eine falsche Auskunft
/// („6 Fenster geholt“, während keines umgezogen ist).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatherResult {
    /// Umgezogene Fenster.
    pub moved: usize,
    /// Gefundene VS-Code-Fenster.
    pub found: usize,
    pub error: Option<String>,
}

/// Die sichtbaren VS-Code-Hauptfenster (Prozess ist ein VS Code, maximierbar). Getrennt
/// vom Umzug, damit man nachsehen kann, WAS eingesammelt würde (auch der Neustart nimmt
/// genau diese Auswahl für sein WM_CLOSE).
pub fn editor_windows(desktop: &dyn Desktop) -> Vec<TopWindow> {
    desktop
        .visible_windows()
        .into_iter()
        .filter(|w| is_editor_window(w.exe.as_deref().unwrap_or(""), &w.title) && is_maximizable(w.style))
        .collect()
}

/// Alle VS-Code-Fenster auf den Hauptmonitor holen und maximieren. Wirft nicht.
pub fn gather_to_primary(desktop: &dyn Desktop, lang: Lang) -> GatherResult {
    // Die Hauptanzeige hat ihre linke obere Ecke per Definition bei (0, 0).
    let Some(area) = desktop.work_area(0, 0) else {
        let e = lang.l("Hauptmonitor nicht ermittelbar.", "Could not determine the primary monitor.");
        return GatherResult { moved: 0, found: 0, error: Some(e.into()) };
    };
    let wins = editor_windows(desktop);
    if wins.is_empty() {
        let e = lang.l("Kein VS-Code-Fenster gefunden.", "No VS Code window found.");
        return GatherResult { moved: 0, found: 0, error: Some(e.into()) };
    }
    let area = (i64::from(area.left), i64::from(area.top), i64::from(area.right), i64::from(area.bottom));
    let mut moved = 0;
    for w in &wins {
        // Die gemerkte Normalgröße mitnehmen: nach einem späteren „Wiederherstellen“ soll
        // das Fenster so groß sein wie vorher, nur auf dem Zielmonitor. Ohne sie greift
        // die Untergrenze von normal_rect.
        let (nw, nh) = desktop.placement(w.hwnd).map_or((0, 0), |p| (p.normal.width(), p.normal.height()));
        let (x, y, rw, rh) = normal_rect(i64::from(nw), i64::from(nh), area);
        let target = Rect::from_xywh(x as i32, y as i32, rw as i32, rh as i32);
        if desktop.move_maximized(w.hwnd, target).is_ok() {
            moved += 1;
        }
    }
    let error = (moved < wins.len()).then(|| {
        let rest = wins.len() - moved;
        if lang.is_english() {
            format!("{rest} window(s) could not be moved.")
        } else {
            format!("{rest} Fenster ließen sich nicht verschieben.")
        }
    });
    GatherResult { moved, found: wins.len(), error }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::FakeDesktop;
    use deck_win::Hwnd;

    const CODE: &str = r"C:\Programs\Microsoft VS Code\Code.exe";
    const FIREFOX: &str = r"C:\Program Files\Mozilla Firefox\firefox.exe";
    const WINDOW: u32 = 0x15C7_0000; // mit WS_MAXIMIZEBOX
    const DIALOG: u32 = 0x94C8_0AC4; // ohne

    fn win(h: isize, title: &str, exe: &str, style: u32) -> TopWindow {
        TopWindow { hwnd: Hwnd(h), title: title.into(), exe: Some(exe.into()), style }
    }

    /// Zwei VS-Code-Fenster, ein Browser mit dem Marker im Titel, ein VS-Code-Dialog und
    /// das Deck selbst.
    fn zoo() -> Vec<TopWindow> {
        vec![
            win(11, "a.py - repo-eins - Visual Studio Code", CODE, WINDOW),
            win(12, "Getting Started - Visual Studio Code - Firefox", FIREFOX, WINDOW),
            win(13, "b.py - repo-zwei - Visual Studio Code", CODE, WINDOW),
            win(14, "Änderungen speichern?", CODE, DIALOG),
            win(15, "Agent Deck", r"C:\Apps\agent-deck.exe", WINDOW),
        ]
    }

    fn desk() -> FakeDesktop {
        FakeDesktop { windows: zoo(), area: Some(Rect::new(0, 0, 1920, 1040)), normal: Rect::from_xywh(0, 0, 1000, 700), ..Default::default() }
    }

    #[test]
    fn only_maximizable_vs_code_windows_are_gathered() {
        let got: Vec<isize> = editor_windows(&desk()).iter().map(|w| w.hwnd.0).collect();
        assert_eq!(got, [11, 13]);
    }

    #[test]
    fn the_balance_names_moved_and_found() {
        assert_eq!(gather_to_primary(&desk(), Lang::German), GatherResult { moved: 2, found: 2, error: None });
    }

    #[test]
    fn every_window_gets_a_rect_on_the_target_monitor() {
        let d = desk();
        gather_to_primary(&d, Lang::German);
        let moved = d.moved.lock().unwrap().clone();
        assert_eq!(moved.iter().map(|(h, _)| h.0).collect::<Vec<_>>(), [11, 13]);
        for (_, r) in moved {
            assert_eq!((r.width(), r.height()), (1000, 700));
            assert!(r.left >= 0 && r.top >= 0 && r.right <= 1920 && r.bottom <= 1040, "{r:?}");
        }
    }

    #[test]
    fn a_failed_window_is_reported_not_hidden() {
        let d = FakeDesktop { refuse: vec![Hwnd(13)], ..desk() };
        let res = gather_to_primary(&d, Lang::German);
        assert_eq!((res.moved, res.found), (1, 2));
        assert!(res.error.unwrap().contains('1'));
    }

    #[test]
    fn without_vs_code_windows_there_is_a_message_instead_of_a_zero() {
        let d = FakeDesktop { windows: vec![], ..desk() };
        let res = gather_to_primary(&d, Lang::English);
        assert_eq!(res.error.as_deref(), Some("No VS Code window found."));
    }

    #[test]
    fn without_a_primary_monitor_nothing_is_moved() {
        let d = FakeDesktop { area: None, ..desk() };
        let res = gather_to_primary(&d, Lang::German);
        assert!(res.error.is_some());
        assert!(d.moved.lock().unwrap().is_empty());
    }
}
