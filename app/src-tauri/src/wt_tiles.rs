//! Die Windows-Terminal-Fenster eines Repo-Blocks gleich groß nebeneinander legen
//! (`deck/ops/wt_tiles.py`). Das Raster rechnet `deck_core::arrange::tile_grid`.
//!
//! Ausgelöst wird es vom [`crate::wt_host::WtHost`], sobald sich die Menge der lebenden
//! Agenten eines Blocks ändert. Von Hand verschobene Fenster bleiben bis dahin liegen.
//! Gefunden wird ein Agentenfenster an seinem TITEL (= Slot-Name). Minimierte bleiben
//! außen vor: wer eines weglegt, will es nicht zurück in die Kachel gezogen bekommen.
//! Monitor ist der des Fensters mit der kleinsten Nummer (das älteste, das der Nutzer
//! am ehesten schon dorthin gezogen hat, wo der Block leben soll).

use std::collections::HashMap;

use deck_core::arrange::{self, max_cols_for, tile_grid};
use deck_core::wt::{WINDOW_CLASS, slot_from_title};
use deck_win::effects::Desktop;
use deck_win::{Hwnd, Rect};

#[derive(Debug, Default)]
pub struct WtTiler {
    seen: HashMap<String, Vec<String>>,
}

impl WtTiler {
    /// Leben in Block `win` andere Agenten als beim letzten Blick? Ein leer gewordener
    /// Block braucht keine Kacheln mehr: dann `false`.
    pub fn changed(&mut self, win: &str, slots: &[String]) -> bool {
        if self.seen.get(win).is_some_and(|s| s == slots) {
            return false;
        }
        self.seen.insert(win.to_string(), slots.to_vec());
        !slots.is_empty()
    }
}

/// Fenster des Agenten `slot`, auch minimiert (Fokussieren holt es zurück).
pub fn hwnd_of(desktop: &dyn Desktop, slot: &str) -> Option<Hwnd> {
    desktop
        .class_windows(WINDOW_CLASS)
        .into_iter()
        .find(|w| slot_from_title(Some(&w.title)).as_deref() == Some(slot))
        .map(|w| w.hwnd)
}

/// Lage der (nicht minimierten) Fenster dieser Agenten als Reihen von oben nach unten,
/// je Reihe von links nach rechts. Minimierte und nicht gefundene fehlen.
pub fn screen_rows(desktop: &dyn Desktop, slots: &[String]) -> Vec<Vec<String>> {
    let mut wins: Vec<(String, arrange::Rect)> = Vec::new();
    for w in desktop.class_windows(WINDOW_CLASS) {
        let Some(slot) = slot_from_title(Some(&w.title)) else { continue };
        if w.minimized || !slots.contains(&slot) || wins.iter().any(|(s, _)| *s == slot) {
            continue;
        }
        if let Some(r) = desktop.window_rect(w.hwnd) {
            wins.push((slot, (i64::from(r.left), i64::from(r.top), i64::from(r.width()), i64::from(r.height()))));
        }
    }
    arrange::screen_rows(&wins)
}

/// Wie viele Fenster auf dem Monitor dieses Blocks nebeneinander passen: dieselbe Regel,
/// nach der [`tile`] kachelt. Monitor ist der des Fensters oben links (`rows[0][0]`).
/// 0 = unbekannt (kein Fenster sichtbar).
pub fn screen_cols(desktop: &dyn Desktop, rows: &[Vec<String>]) -> u32 {
    let Some(first) = rows.first().and_then(|r| r.first()) else { return 0 };
    let Some(hwnd) = hwnd_of(desktop, first) else { return 0 };
    let Some(r) = desktop.window_rect(hwnd) else { return 0 };
    let (cx, cy) = r.center();
    let Some(area) = desktop.work_area(cx, cy) else { return 0 };
    max_cols_for(i64::from(area.width()), desktop.scale_of(hwnd)) as u32
}

/// Die Fenster dieser Agenten in Slot-Reihenfolge kacheln. Liefert die Zahl der
/// gelegten Fenster.
pub fn tile(desktop: &dyn Desktop, slots: &[String]) -> usize {
    let mut found: HashMap<String, Hwnd> = HashMap::new();
    for w in desktop.class_windows(WINDOW_CLASS) {
        if let Some(slot) = slot_from_title(Some(&w.title))
            && slots.contains(&slot)
            && !w.minimized
        {
            found.entry(slot).or_insert(w.hwnd);
        }
    }
    let order: Vec<Hwnd> = slots.iter().filter_map(|s| found.get(s).copied()).collect();
    let Some(first) = order.first().copied() else { return 0 };
    let Some(r) = desktop.window_rect(first) else { return 0 };
    let (cx, cy) = r.center();
    let Some(area) = desktop.work_area(cx, cy) else { return 0 };
    let cols = max_cols_for(i64::from(area.width()), desktop.scale_of(first));
    let grid = tile_grid(
        order.len() as i64,
        (i64::from(area.left), i64::from(area.top), i64::from(area.right), i64::from(area.bottom)),
        cols,
    );
    let mut placed = 0;
    for (hwnd, (x, y, w, h)) in order.iter().zip(grid) {
        if desktop.place(*hwnd, Rect::from_xywh(x as i32, y as i32, w as i32, h as i32)).is_ok() {
            placed += 1;
        }
    }
    placed
}

/// Arbeitsfläche und Skalierung des Monitors unter dem Punkt `pos` (Mauszeiger).
/// `None`, wenn kein Monitor ihn enthält oder es keinen Punkt gibt.
pub fn monitor_under(desktop: &dyn Desktop, pos: Option<(i32, i32)>) -> Option<(Rect, f64)> {
    let (x, y) = pos?;
    desktop
        .monitors()
        .into_iter()
        .find(|m| (m.rect.left..m.rect.right).contains(&x) && (m.rect.top..m.rect.bottom).contains(&y))
        .map(|m| (m.work, m.scale))
}

/// Taste „alle nach vorn“: die Fenster dieser Agenten, über alle Blöcke hinweg, als EIN
/// Raster in Slot-Reihenfolge auf die Fläche `here` legen (Monitor unter der Maus).
///
/// Anders als [`tile`] auch die minimierten: sie werden wiederhergestellt, denn wer alle
/// holt, meint alle. Ein Raster statt eines je Block, weil sich zwei Blöcke auf demselben
/// Monitor sonst überdecken. Ohne `here` bleiben die Fenster liegen, wo sie sind.
/// Liefert `(Slot, Fenster)` in Slot-Reihenfolge; nach vorn holt der Aufrufer.
pub fn gather(desktop: &dyn Desktop, slots: &[String], here: Option<(Rect, f64)>) -> Vec<(String, Hwnd)> {
    let mut found: HashMap<String, (Hwnd, bool)> = HashMap::new();
    for w in desktop.class_windows(WINDOW_CLASS) {
        if let Some(slot) = slot_from_title(Some(&w.title))
            && slots.contains(&slot)
        {
            found.entry(slot).or_insert((w.hwnd, w.minimized));
        }
    }
    let order: Vec<(String, Hwnd, bool)> =
        slots.iter().filter_map(|s| found.get(s).map(|(h, m)| (s.clone(), *h, *m))).collect();
    for (_, hwnd, minimized) in &order {
        if *minimized {
            let _ = desktop.restore(*hwnd);
        }
    }
    if let Some((area, scale)) = here.filter(|_| !order.is_empty()) {
        let cols = max_cols_for(i64::from(area.width()), scale);
        let grid = tile_grid(
            order.len() as i64,
            (i64::from(area.left), i64::from(area.top), i64::from(area.right), i64::from(area.bottom)),
            cols,
        );
        for ((_, hwnd, _), (x, y, w, h)) in order.iter().zip(grid) {
            let _ = desktop.place(*hwnd, Rect::from_xywh(x as i32, y as i32, w as i32, h as i32));
        }
    }
    order.into_iter().map(|(s, h, _)| (s, h)).collect()
}

/// Taste „alle nach vorn“: [`gather`] auf den Monitor unter `cursor`, dann alle Fenster
/// nach vorn, `target` zuletzt, damit er obenauf liegt und die Tastatur hat.
pub fn bring_front(desktop: &dyn Desktop, slots: &[String], target: Option<&str>, cursor: Option<(i32, i32)>) {
    let placed = gather(desktop, slots, monitor_under(desktop, cursor));
    let top = placed.iter().find(|(s, _)| Some(s.as_str()) == target).map(|(_, h)| *h);
    for (_, h) in placed.iter().rev() {
        if Some(*h) != top {
            let _ = desktop.focus(*h);
        }
    }
    if let Some(h) = top {
        let _ = desktop.focus(h);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeDesktop;

    #[test]
    fn gather_lays_all_agents_as_one_grid_under_the_mouse_and_restores_minimized() {
        let d = FakeDesktop::default();
        d.add_monitor(Rect::new(1920, 0, 4320, 1200), Rect::new(1920, 0, 4320, 1160), 1.0);
        d.add_wt(Hwnd(11), "A1", Rect::new(0, 0, 800, 600), false);
        d.add_wt(Hwnd(21), "B1", Rect::new(0, 0, 800, 600), true);
        d.add_wt(Hwnd(12), "A2", Rect::new(0, 0, 800, 600), false);
        let here = monitor_under(&d, Some((2000, 100)));
        assert_eq!(here, Some((Rect::new(1920, 0, 4320, 1160), 1.0)));
        let got = gather(&d, &["B1".into(), "A1".into(), "A2".into()], here);
        assert_eq!(got, [("B1".into(), Hwnd(21)), ("A1".into(), Hwnd(11)), ("A2".into(), Hwnd(12))]);
        assert_eq!(d.restored(), [Hwnd(21)]);
        assert_eq!(
            d.placed(),
            [
                (Hwnd(21), Rect::new(1920, 0, 2720, 1160)),
                (Hwnd(11), Rect::new(2720, 0, 3520, 1160)),
                (Hwnd(12), Rect::new(3520, 0, 4320, 1160)),
            ]
        );
    }

    #[test]
    fn the_target_comes_to_the_front_last_and_so_lies_on_top() {
        let d = FakeDesktop::default();
        d.add_wt(Hwnd(11), "A1", Rect::new(0, 0, 800, 600), false);
        d.add_wt(Hwnd(12), "A2", Rect::new(0, 0, 800, 600), false);
        d.add_wt(Hwnd(13), "A3", Rect::new(0, 0, 800, 600), false);
        bring_front(&d, &["A1".into(), "A2".into(), "A3".into()], Some("A2"), Some((10, 10)));
        assert_eq!(d.focused(), [Hwnd(13), Hwnd(11), Hwnd(12)]);
        assert_eq!(d.placed().len(), 3);
    }

    #[test]
    fn without_a_monitor_under_the_mouse_nothing_moves() {
        let d = FakeDesktop::default();
        d.add_wt(Hwnd(11), "A1", Rect::new(0, 0, 800, 600), false);
        assert_eq!(monitor_under(&d, Some((-5000, -5000))), None);
        assert_eq!(monitor_under(&d, None), None);
        assert_eq!(gather(&d, &["A1".into()], None), [("A1".into(), Hwnd(11))]);
        assert!(d.placed().is_empty());
    }

    #[test]
    fn screen_rows_read_the_real_window_positions_and_skip_minimized() {
        let d = FakeDesktop::default();
        d.add_wt(Hwnd(11), "A1", Rect::new(1280, 0, 2560, 720), false);
        d.add_wt(Hwnd(12), "A2", Rect::new(0, 0, 1280, 720), false);
        d.add_wt(Hwnd(13), "A3", Rect::new(0, 720, 1280, 1440), false);
        d.add_wt(Hwnd(14), "A4", Rect::new(0, 0, 10, 10), true);
        d.add_wt(Hwnd(21), "B1", Rect::new(0, 0, 10, 10), false);
        let rows = screen_rows(&d, &["A1".into(), "A2".into(), "A3".into(), "A4".into()]);
        assert_eq!(rows, [vec!["A2", "A1"], vec!["A3"]]);
        // Arbeitsfläche der Attrappe: 1920 breit, Faktor 1 = drei Fenster nebeneinander.
        assert_eq!(screen_cols(&d, &rows), 3);
        assert_eq!(screen_cols(&d, &[]), 0);
    }

    #[test]
    fn a_block_is_retiled_only_when_its_agents_change() {
        let mut t = WtTiler::default();
        let a = vec!["A1".to_string()];
        assert!(t.changed("A", &a));
        assert!(!t.changed("A", &a));
        assert!(t.changed("A", &["A1".into(), "A2".into()]));
        assert!(!t.changed("A", &[]), "leerer Block: nichts zu kacheln");
    }

    #[test]
    fn agents_are_laid_side_by_side_on_the_monitor_of_the_first() {
        let d = FakeDesktop::default();
        d.add_wt(Hwnd(11), "A1", Rect::new(0, 0, 800, 600), false);
        d.add_wt(Hwnd(12), "A2", Rect::new(100, 100, 900, 700), false);
        d.add_wt(Hwnd(13), "A3", Rect::new(100, 100, 900, 700), true);
        d.add_wt(Hwnd(14), "Einstellungen", Rect::new(0, 0, 10, 10), false);
        let n = tile(&d, &["A1".into(), "A2".into(), "A3".into()]);
        assert_eq!(n, 2, "minimierte bleiben liegen");
        let placed = d.placed();
        assert_eq!(placed[0], (Hwnd(11), Rect::new(0, 0, 960, 1040)));
        assert_eq!(placed[1], (Hwnd(12), Rect::new(960, 0, 1920, 1040)));
        assert_eq!(hwnd_of(&d, "A3"), Some(Hwnd(13)));
    }
}
