//! Muss der Fokus-Rahmen ein neues Bild zeichnen, oder steht das letzte noch richtig da?
//!
//! Ein stiller Glow (eigener Glow mit Animation „Still“, voll eingeblendet, Fenster an
//! derselben Stelle) sieht in jedem Bild gleich aus: der Shader liest die Zeit dann nicht.
//! Vorher wurde trotzdem jedes Bild neu gerechnet und präsentiert, 60-mal je Sekunde,
//! solange ein Agent den Fokus hatte; DWM mischte die Fläche jedes Mal neu. Gemessen in
//! der Test-VM (`tools/probe`, Ablauf `idle`) war das der größte Teil der Leerlauf-Last
//! des Decks. Jetzt bleibt das letzte Bild einfach stehen, bis sich etwas ändert.

use crate::frame_fx::Design;
use crate::types::Rect;

/// Was ein Bild des Rahmens bestimmt, außer der Zeit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Still {
    pub design: Design,
    pub window: Rect,
    pub stage: Rect,
    pub scale: f64,
    pub night: bool,
    pub glow: [[f32; 4]; 4],
}

impl Still {
    /// Hängt das Bild von der Zeit ab? Winter-Designs bewegen sich immer, der eigene Glow
    /// nur mit einer Animation (`deck_core::glow::GlowStyle::shader_params`, Feld [1][2]:
    /// 0 = Still).
    pub fn animated(&self) -> bool {
        self.design != Design::Eigen || self.glow[1][2] != 0.0
    }
}

/// Neues Bild nötig? `last` ist, was zuletzt präsentiert wurde (`None` = noch nichts oder
/// der Puffer ist frisch), `level` die Deckkraft der Blende (1 = voll da).
pub fn needs_paint(last: Option<&Still>, next: &Still, level: f64) -> bool {
    next.animated() || level < 1.0 || last != Some(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn still() -> Still {
        let mut glow = [[0.0; 4]; 4];
        glow[0] = [1.0, 0.3, 0.0, 1.0];
        Still {
            design: Design::Eigen,
            window: Rect::new(0, 0, 800, 600),
            stage: Rect::new(0, 0, 1920, 1080),
            scale: 1.0,
            night: false,
            glow,
        }
    }

    #[test]
    fn a_still_glow_that_is_fully_in_is_drawn_once_and_then_left_standing() {
        let s = still();
        assert!(needs_paint(None, &s, 1.0), "erstes Bild");
        assert!(!needs_paint(Some(&s), &s, 1.0), "danach steht es");
    }

    #[test]
    fn fading_moving_or_restyling_draws_again() {
        let s = still();
        assert!(needs_paint(Some(&s), &s, 0.5), "Blende läuft");
        let moved = Still { window: Rect::new(10, 0, 810, 600), ..s };
        assert!(needs_paint(Some(&s), &moved, 1.0), "Fenster bewegt");
        let mut glow = s.glow;
        glow[0][3] = 0.8;
        assert!(needs_paint(Some(&s), &Still { glow, ..s }, 1.0), "andere Stärke");
        assert!(needs_paint(Some(&s), &Still { night: true, ..s }, 1.0), "hell/dunkel");
    }

    #[test]
    fn animations_and_winter_designs_draw_every_frame() {
        let s = still();
        let mut glow = s.glow;
        glow[1][2] = 1.0;
        let breathing = Still { glow, ..s };
        assert!(needs_paint(Some(&breathing), &breathing, 1.0));
        let winter = Still { design: Design::WINTER[0], ..s };
        assert!(needs_paint(Some(&winter), &winter, 1.0));
    }
}
