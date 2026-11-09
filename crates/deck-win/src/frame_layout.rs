//! Wo der Fokus-Rahmen liegt: Rahmenbild um das Agentenfenster und die vier Streifen,
//! in die es zerlegt gezeigt wird.
//!
//! Das Rahmenbild ist das Agentenfenster plus Hof ([`MARGIN_PT`] je Seite), gestutzt auf
//! den Monitor des Fensters: der Hof soll nicht auf den Nachbarbildschirm leuchten (so
//! auch `Program.cs` der Python-Fassung).
//!
//! Gezeigt wird es in VIER Streifen statt einem Fenster: oben und unten je einer über
//! die ganze Breite samt Ecken, links und rechts je einer dazwischen. Innen ab
//! [`INNER_PT`] Tiefe ist das Bild ohnehin leer, und ein Fenster von der Größe des
//! ganzen Terminals hieße je Bild Megabytes durch `UpdateLayeredWindow` (bei 2560x1400
//! und 150 % rund 16 MB statt knapp 3 MB). Dieselbe Zerlegung hatte die
//! Python-Fassung vor dem Wechsel auf die Grafikkarte (`domain/focus_frame.pieces`).

use crate::frame_fx::{Design, FrameParams, INNER_PT, MARGIN_PT, compute};
use crate::types::Rect;

/// Seite eines Streifens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

impl Side {
    pub const ALL: [Side; 4] = [Side::Top, Side::Right, Side::Bottom, Side::Left];
}

/// Ein Streifen: wo er auf dem Bildschirm liegt und welcher Ausschnitt des Rahmenbilds
/// hineingehört (beide gleich groß).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Strip {
    pub side: Side,
    pub screen: Rect,
    pub region: Rect,
}

/// Lage des Rahmens für ein Agentenfenster.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameLayout {
    /// Das Rahmenbild in Bildschirmkoordinaten; seine linke obere Ecke ist (0, 0) im Bild.
    pub frame: Rect,
    /// Sichtbarer Rand des Agentenfensters in Bildschirmkoordinaten.
    pub window: Rect,
    pub scale: f64,
}

/// Hof nach außen in Pixeln.
pub fn margin_px(scale: f64) -> i32 {
    (MARGIN_PT * scale).ceil() as i32
}

/// Tiefe nach innen, bis zu der noch etwas leuchten kann, in Pixeln. Ein Pixel in Tiefe
/// k (ab 0) hat seine Mitte bei k + 0.5 und leuchtet, solange das ≤ 24 pt ist; ceil
/// deckt damit alle ab.
pub fn inner_px(scale: f64) -> i32 {
    (INNER_PT * scale).ceil() as i32
}

impl FrameLayout {
    /// `window` = sichtbarer Rand des Agentenfensters, `monitor` = ganze Fläche seines
    /// Monitors. `None`, wenn vom Rahmen nichts auf dem Monitor bliebe.
    pub fn new(window: Rect, monitor: Rect, scale: f64) -> Option<FrameLayout> {
        if window.is_empty() || scale.is_nan() || scale <= 0.0 {
            return None;
        }
        let g = margin_px(scale);
        let grown = Rect::new(
            window.left - g,
            window.top - g,
            window.right + g,
            window.bottom + g,
        );
        let frame = grown.intersect(&monitor)?;
        Some(FrameLayout {
            frame,
            window,
            scale,
        })
    }

    /// Das Agentenfenster im Rahmenbild (x0, y0, x1, y1), wie der Renderer es braucht.
    pub fn win_in_frame(&self) -> [f32; 4] {
        let r = self.window.offset(-self.frame.left, -self.frame.top);
        [r.left as f32, r.top as f32, r.right as f32, r.bottom as f32]
    }

    /// Parameter für ein Bild.
    pub fn params(&self, design: Design, seed: i32, t: f64) -> FrameParams {
        compute(
            design,
            seed,
            t,
            self.scale,
            self.win_in_frame(),
            f64::from(self.window.width()),
            f64::from(self.window.height()),
        )
    }

    /// Die vier Streifen, ohne Überlappung, gestutzt aufs Rahmenbild. Ist das Fenster
    /// zu niedrig für Seitenstreifen, gibt es nur oben und unten; was ganz vom Monitor
    /// fällt, fehlt.
    pub fn strips(&self) -> Vec<Strip> {
        let (outer, inner) = (margin_px(self.scale), inner_px(self.scale));
        let w = self.window;
        let band = outer + inner;
        let mut pieces = vec![
            (
                Side::Top,
                Rect::from_xywh(w.left - outer, w.top - outer, w.width() + 2 * outer, band),
            ),
            (
                Side::Bottom,
                Rect::from_xywh(
                    w.left - outer,
                    w.bottom - inner,
                    w.width() + 2 * outer,
                    band,
                ),
            ),
        ];
        let side_h = w.height() - 2 * inner;
        if side_h > 0 {
            pieces.push((
                Side::Left,
                Rect::from_xywh(w.left - outer, w.top + inner, band, side_h),
            ));
            pieces.push((
                Side::Right,
                Rect::from_xywh(w.right - inner, w.top + inner, band, side_h),
            ));
        }
        // Bei einem sehr niedrigen Fenster überlappen sich oben und unten: dann bekommt
        // unten nur den Rest, sonst würde ein Pixel doppelt gedeckt (und doppelt hell).
        if let [(_, top), (_, bottom), ..] = pieces.as_mut_slice()
            && bottom.top < top.bottom
        {
            bottom.top = top.bottom;
        }
        pieces
            .into_iter()
            .filter_map(|(side, r)| {
                let screen = r.intersect(&self.frame)?;
                Some(Strip {
                    side,
                    screen,
                    region: screen.offset(-self.frame.left, -self.frame.top),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MON: Rect = Rect::new(0, 0, 2560, 1400);

    fn area(strips: &[Strip]) -> i64 {
        strips
            .iter()
            .map(|s| i64::from(s.screen.width()) * i64::from(s.screen.height()))
            .sum()
    }

    #[test]
    fn margin_and_depth_scale_with_the_monitor() {
        assert_eq!((margin_px(1.0), inner_px(1.0)), (36, 24));
        assert_eq!((margin_px(1.5), inner_px(1.5)), (54, 36));
        assert_eq!((margin_px(1.25), inner_px(1.25)), (45, 30));
    }

    #[test]
    fn frame_is_window_plus_glow_cut_to_its_monitor() {
        let win = Rect::new(10, 100, 810, 700);
        let l = FrameLayout::new(win, MON, 1.0).unwrap();
        assert_eq!(
            l.frame,
            Rect::new(0, 64, 846, 736),
            "links an den Monitor gestutzt"
        );
        assert_eq!(l.win_in_frame(), [10.0, 36.0, 810.0, 636.0]);
    }

    #[test]
    fn strips_cover_the_glow_band_without_overlap() {
        let win = Rect::new(200, 200, 1000, 800);
        let l = FrameLayout::new(win, MON, 1.0).unwrap();
        let s = l.strips();
        assert_eq!(s.len(), 4);
        for (i, a) in s.iter().enumerate() {
            for b in &s[i + 1..] {
                assert_eq!(
                    a.screen.intersect(&b.screen),
                    None,
                    "{:?} / {:?}",
                    a.side,
                    b.side
                );
            }
            assert_eq!(
                (a.screen.width(), a.screen.height()),
                (a.region.width(), a.region.height())
            );
        }
        // Band = außen + innen um das ganze Fenster.
        let outer = Rect::new(200 - 36, 200 - 36, 1000 + 36, 800 + 36);
        let inner = Rect::new(200 + 24, 200 + 24, 1000 - 24, 800 - 24);
        let expected = i64::from(outer.width()) * i64::from(outer.height())
            - i64::from(inner.width()) * i64::from(inner.height());
        assert_eq!(area(&s), expected);
    }

    #[test]
    fn a_flat_window_gets_top_and_bottom_only_and_they_do_not_overlap() {
        let win = Rect::new(200, 200, 1000, 230);
        let s = FrameLayout::new(win, MON, 1.0).unwrap().strips();
        assert_eq!(
            s.iter().map(|s| s.side).collect::<Vec<_>>(),
            vec![Side::Top, Side::Bottom]
        );
        assert_eq!(s[0].screen.intersect(&s[1].screen), None);
    }

    #[test]
    fn strip_regions_are_relative_to_the_frame_origin() {
        let win = Rect::new(500, 300, 900, 700);
        let l = FrameLayout::new(win, MON, 1.0).unwrap();
        let top = l.strips()[0];
        assert_eq!(top.screen.left - top.region.left, l.frame.left);
        assert_eq!(top.region, Rect::new(0, 0, 400 + 72, 60));
    }

    #[test]
    fn a_window_off_its_monitor_has_no_frame() {
        assert_eq!(
            FrameLayout::new(Rect::new(3000, 0, 3500, 400), MON, 1.0),
            None
        );
        assert_eq!(FrameLayout::new(Rect::new(0, 0, 0, 400), MON, 1.0), None);
        assert_eq!(FrameLayout::new(Rect::new(0, 0, 10, 10), MON, 0.0), None);
    }
}
