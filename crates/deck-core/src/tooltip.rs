//! Wo der Hover-Tooltip einer Karte liegt (`deck/ui/hover.py`): unter der Karte, passt
//! er dort nicht mehr auf den Monitor, darüber; immer in die Arbeitsfläche des Monitors
//! der Karte geklemmt. Das Fenster hat rundum Luft für den Schatten, die über die Karte
//! hinausragen darf, damit der sichtbare Tooltip bündig an ihr sitzt.

use serde::Deserialize;

use crate::dock::Area;

/// Luft um die Karte im Tooltip-Fenster für den Schatten (CSS-Pixel, TooltipView.tsx).
const TIP_GUTTER: f64 = 8.0;
/// Abstand zwischen Karte und Tooltip (CSS-Pixel).
const TIP_GAP: f64 = 2.0;

/// Rechteck einer Karte in physischen Bildschirmpixeln (von der Oberfläche).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct ScreenRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Lage des Tooltip-Fensters: unter der Karte, sonst darüber, auf dem Monitor geklemmt.
/// `size_px` ist die gemessene Größe samt Schattenraum in Bildschirmpixeln.
pub fn place(card: ScreenRect, size_px: (i32, i32), work: Area, scale: f64) -> Area {
    let (w, h) = (size_px.0.min(work.width()), size_px.1.min(work.height()));
    let gutter = (TIP_GUTTER * scale).round() as i32;
    let gap = (TIP_GAP * scale).round() as i32;
    let x = (card.x - gutter).clamp(work.left, (work.right - w).max(work.left));
    let below = card.y + card.height + gap - gutter;
    let y = if below + h <= work.bottom { below } else { (card.y - gap + gutter - h).max(work.top) };
    Area::new(x, y, x + w, y + h)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Area = Area::new(0, 0, 1920, 1040);

    #[test]
    fn tooltip_sits_under_the_card_with_its_shadow_gutter_outside() {
        let card = ScreenRect { x: 100, y: 200, width: 197, height: 118 };
        let a = place(card, (376, 120), WORK, 1.0);
        assert_eq!((a.left, a.top), (92, 312));
    }

    #[test]
    fn tooltip_flips_above_the_card_when_the_monitor_ends() {
        let card = ScreenRect { x: 100, y: 900, width: 197, height: 118 };
        let a = place(card, (376, 120), WORK, 1.0);
        assert_eq!(a.bottom, 906);
    }

    #[test]
    fn tooltip_stays_on_the_monitor_of_the_card() {
        let work = Area::new(1920, 0, 3840, 1040);
        let card = ScreenRect { x: 3700, y: 10, width: 197, height: 58 };
        let a = place(card, (376, 120), work, 1.0);
        assert_eq!(a.right, 3840);
    }
}
