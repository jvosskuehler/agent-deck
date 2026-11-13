//! Wo der Fokus-Rahmen liegt: das Rahmenbild um das Agentenfenster.
//!
//! Das Rahmenbild ist das Agentenfenster plus Hof ([`MARGIN_PT`] je Seite), gestutzt auf
//! den Monitor des Fensters: der Hof soll nicht auf den Nachbarbildschirm leuchten (so
//! auch `Program.cs` der Python-Fassung).

use crate::frame_fx::{Design, FrameParams, MARGIN_PT, compute};
use crate::types::Rect;

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
}

#[cfg(test)]
mod tests {
    use super::*;

    const MON: Rect = Rect::new(0, 0, 2560, 1400);

    #[test]
    fn the_glow_margin_scales_with_the_monitor() {
        assert_eq!(margin_px(1.0), 60);
        assert_eq!(margin_px(1.5), 90);
        assert_eq!(margin_px(1.25), 75);
    }

    #[test]
    fn frame_is_window_plus_glow_cut_to_its_monitor() {
        let win = Rect::new(10, 100, 810, 700);
        let l = FrameLayout::new(win, MON, 1.0).unwrap();
        assert_eq!(
            l.frame,
            Rect::new(0, 40, 870, 760),
            "links an den Monitor gestutzt"
        );
        assert_eq!(l.win_in_frame(), [10.0, 60.0, 810.0, 660.0]);
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
