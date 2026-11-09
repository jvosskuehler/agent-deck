//! Der Fokus-Rahmen als Ganzes auf dem CPU-Weg: vier Layered-Streifen um ein
//! Agentenfenster, je Bild neu gezeichnet (entspricht `App.Frame()` in
//! `focus-frame/Program.cs`). Der Hauptweg ist [`crate::frame_gpu_overlay`] mit derselben
//! Schnittstelle; dieser hier ist der Rückfall ohne Grafikgerät und zu teuer für volle
//! Bildrate (siehe [`crate::frame_render`]).
//!
//! WELCHES Fenster einen Rahmen bekommt und welches Design dran ist, entscheidet die
//! App (Python: `ui/focus_frame.py`); hier wird nur gezeichnet. Der Rahmen folgt dem
//! Fenster selbst: jedes Bild fragt dessen Lage neu ab, damit er beim Ziehen nicht
//! hinterherhinkt.
//!
//! Benutzung auf einem eigenen Thread (die Overlays gehören ihm, siehe
//! [`crate::overlay`]):
//!
//! ```no_run
//! # use deck_win::{frame_fx::Design, frame_overlay::FocusFrameOverlay, overlay, Hwnd};
//! # let (target, seed, t0) = (Hwnd(0), 42, std::time::Instant::now());
//! let mut frame = FocusFrameOverlay::new(4);
//! loop {
//!     overlay::pump_messages();
//!     frame.draw(target, Design::Kometen, seed, t0.elapsed().as_secs_f64()).ok();
//!     overlay::wait_for_vblank();
//! #   break;
//! }
//! ```

use crate::arrange::visible_rect;
use crate::dpi;
use crate::frame_fx::Design;
use crate::frame_layout::{FrameLayout, Side};
use crate::frame_render::render_parallel;
use crate::monitor::monitor_of_window;
use crate::overlay::Overlay;
use crate::types::{Hwnd, Result};
use crate::window::{is_minimized, is_window};

/// Vier Streifenfenster samt Bildpuffer.
pub struct FocusFrameOverlay {
    strips: [Option<Overlay>; 4],
    buf: Vec<u8>,
    threads: usize,
    last: Option<FrameLayout>,
}

fn slot(side: Side) -> usize {
    match side {
        Side::Top => 0,
        Side::Right => 1,
        Side::Bottom => 2,
        Side::Left => 3,
    }
}

impl FocusFrameOverlay {
    /// `threads` = auf wie viele Threads das Zeichnen je Streifen verteilt wird.
    /// Fenster werden erst beim ersten Bild angelegt.
    pub fn new(threads: usize) -> FocusFrameOverlay {
        FocusFrameOverlay {
            strips: [None, None, None, None],
            buf: Vec::new(),
            threads: threads.max(1),
            last: None,
        }
    }

    /// Ein Bild des Rahmens um `target` zeichnen und zeigen. `t` = Sekunden seit dem
    /// Fokuswechsel (dann beginnt jedes Design von vorn), `seed` = Zufall des Designs.
    ///
    /// `Ok(false)`: nichts zu zeigen (Fenster weg, minimiert, ohne DWM-Rand oder ganz
    /// außerhalb seines Monitors); der Rahmen ist dann versteckt.
    pub fn draw(&mut self, target: Hwnd, design: Design, seed: i32, t: f64) -> Result<bool> {
        let Some(layout) = self.layout_of(target) else {
            self.hide();
            return Ok(false);
        };
        let params = layout.params(design, seed, t);
        let mut used = [false; 4];
        for strip in layout.strips() {
            let i = slot(strip.side);
            used[i] = true;
            let n = (strip.region.width() * strip.region.height() * 4) as usize;
            self.buf.resize(n, 0);
            render_parallel(&params, strip.region, &mut self.buf, self.threads);
            if self.strips[i].is_none() {
                self.strips[i] = Some(Overlay::new()?);
            }
            if let Some(ov) = self.strips[i].as_mut() {
                ov.show(strip.screen, &self.buf)?;
            }
        }
        for (i, ov) in self.strips.iter_mut().enumerate() {
            if !used[i]
                && let Some(ov) = ov
            {
                ov.hide();
            }
        }
        self.last = Some(layout);
        Ok(true)
    }

    /// Alle Streifen verstecken (die Fenster bleiben für das nächste Bild bestehen).
    pub fn hide(&mut self) {
        for ov in self.strips.iter_mut().flatten() {
            ov.hide();
        }
        self.last = None;
    }

    /// Lage des zuletzt gezeigten Bildes (`None` = versteckt).
    pub fn last_layout(&self) -> Option<FrameLayout> {
        self.last
    }

    fn layout_of(&self, target: Hwnd) -> Option<FrameLayout> {
        if !is_window(target) || is_minimized(target) {
            return None;
        }
        let window = visible_rect(target)?;
        let monitor = monitor_of_window(target)?.rect;
        FrameLayout::new(window, monitor, dpi::factor_for_window(target))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vanished_window_gets_no_frame_and_creates_no_overlay() {
        let mut f = FocusFrameOverlay::new(2);
        assert_eq!(f.draw(Hwnd(0), Design::Glut, 1, 0.0), Ok(false));
        assert!(f.strips.iter().all(Option::is_none));
        assert_eq!(f.last_layout(), None);
    }
}
