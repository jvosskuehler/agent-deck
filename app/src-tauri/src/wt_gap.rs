//! Die freie Kachel neben den Agentenfenstern eines Blocks: 5 Fenster liegen als 3 + 2,
//! rechts unten bleibt eine Kachel leer (`arrange::grid_gap`). Der Kern zeigt dort
//! nichts; eine Erweiterung kann sie füllen (`WtHost::gap`, `ui_windows::companion`).
//!
//! Eine Lücke gibt es nur, solange die Fenster so liegen, wie [`wt_tiles::tile`] sie
//! gelegt hat. Hat der Nutzer eines verschoben, maximiert oder minimiert, weiß das Deck
//! nicht, was auf dem Bildschirm frei ist, und zeigt nichts.

use deck_core::arrange::{self, max_cols_for};
use deck_win::effects::Desktop;
use deck_win::{Hwnd, Rect};

use crate::wt_tiles;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gap {
    /// Fläche der freien Kachel (physische Pixel): die ganze Kachel samt dem Streifen, in
    /// dem bei den Agenten der Statusring liegt. Wer sie füllt, malt dort selbst einen Rand
    /// (`group_shadow::ring_px` breit), sonst schiene im Streifen und in runden Ecken der
    /// Desktop durch.
    pub rect: Rect,
    /// Das vorderste Agentenfenster des Blocks: wer die Lücke füllt, legt sich direkt
    /// dahinter, also mit den Agenten zusammen vorn oder hinten, nie über ein fremdes Fenster.
    pub above: Hwnd,
}

/// Die freie Kachel im Raster dieser Agenten (`slots` in Kachel-Reihenfolge).
pub fn gap(desktop: &dyn Desktop, slots: &[String]) -> Option<Gap> {
    let plan = wt_tiles::tile_plan(desktop, slots, 0);
    let (first, _) = *plan.first()?;
    if wt_tiles::needs_moving(desktop, &plan) {
        return None;
    }
    let (cx, cy) = desktop.window_rect(first)?.center();
    let area = desktop.work_area(cx, cy)?;
    let scale = desktop.scale_of(first);
    let cell = arrange::grid_gap(
        plan.len() as i64,
        (i64::from(area.left), i64::from(area.top), i64::from(area.right), i64::from(area.bottom)),
        max_cols_for(i64::from(area.width()), scale),
    )?;
    let ours: Vec<Hwnd> = plan.iter().map(|(h, _)| *h).collect();
    let above = desktop.z_order().into_iter().find(|h| ours.contains(h)).unwrap_or(first);
    let (x, y, w, h) = cell;
    Some(Gap { rect: Rect::from_xywh(x as i32, y as i32, w as i32, h as i32), above })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeDesktop;

    fn slots(n: usize) -> Vec<String> {
        (1..=n).map(|i| format!("B{i}")).collect()
    }

    /// `n` Agentenfenster, so gelegt, wie das Deck sie kachelt.
    fn tiled(n: usize) -> FakeDesktop {
        let d = FakeDesktop::default();
        for (i, s) in slots(n).iter().enumerate() {
            d.add_wt(Hwnd(100 + i as isize), s, Rect::new(0, 0, 10, 10), false);
        }
        for (h, r) in wt_tiles::tile_plan(&d, &slots(n), 0) {
            d.move_wt(h, r);
        }
        d
    }

    #[test]
    fn five_tiled_windows_leave_the_tile_bottom_right() {
        // Attrappe: 1920 × 1040 bei 100 %, drei passen nebeneinander, fünf liegen als 3 + 2.
        let d = tiled(5);
        let g = gap(&d, &slots(5)).expect("Lücke");
        let last = d.window_rect(Hwnd(104)).unwrap();
        let ring = deck_core::group_shadow::ring_px(1.0) as i32;
        assert_eq!(g.rect.top, last.top - ring, "gleiche Reihe wie das letzte Fenster, samt Ring");
        assert_eq!(g.rect.left, last.right + ring, "bündig an den Ring des Nachbarn");
        assert_eq!(g.rect.width(), last.width() + 2 * ring, "so breit wie eine Kachel samt Ring");
        assert_eq!(g.rect.height(), last.height() + 2 * ring);
    }

    #[test]
    fn a_full_grid_has_no_gap() {
        assert_eq!(gap(&tiled(6), &slots(6)), None);
        assert_eq!(gap(&tiled(3), &slots(3)), None);
    }

    #[test]
    fn a_window_moved_by_hand_hides_the_gap() {
        let d = tiled(5);
        d.move_wt(Hwnd(102), Rect::new(300, 200, 900, 700));
        assert_eq!(gap(&d, &slots(5)), None);
    }

    #[test]
    fn the_gap_lies_right_behind_the_front_most_agent() {
        let d = tiled(5);
        d.set_foreground(Hwnd(999), "Browser", "Chrome_WidgetWin_1");
        d.set_foreground(Hwnd(103), "B4", deck_core::wt::WINDOW_CLASS);
        d.set_foreground(Hwnd(999), "Browser", "Chrome_WidgetWin_1");
        assert_eq!(gap(&d, &slots(5)).unwrap().above, Hwnd(103));
    }
}
