//! Zoom des Panels (`deck/ui/layout.py`): das Fenster folgt dem Inhalt mal Zoom. Der
//! Zoom ist der vom Nutzer an der Ecke gezogene, verkleinert, wenn das Panel sonst nicht
//! auf die Arbeitsfläche passte: lieber alles kleiner als etwas abgeschnitten.

use crate::dock::Area;

/// Grenzen des vom Nutzer gezogenen Zooms (Ecke des freien Panels).
pub const ZOOM_MIN: f64 = 0.5;
pub const ZOOM_MAX: f64 = 2.0;

/// Wirksamer Zoom des Panels: der gewünschte, aber nie größer, als die Arbeitsfläche
/// zulässt (`layout.py`: lieber alles kleiner als etwas abgeschnitten). `natural` ist
/// die Inhaltsgröße bei Zoom 1 in Bildschirmpixeln (samt Schattenraum).
pub fn fit_zoom(natural: (f64, f64), work: Area, wanted: f64) -> f64 {
    let wanted = wanted.clamp(ZOOM_MIN, ZOOM_MAX);
    if natural.0 <= 0.0 || natural.1 <= 0.0 || work.width() <= 0 || work.height() <= 0 {
        return wanted;
    }
    let fit = (f64::from(work.width()) / natural.0).min(f64::from(work.height()) / natural.1);
    // Untergrenze nur als Absturzschutz, wie `_slim_fit_scale`.
    wanted.min(fit).max(0.05)
}

/// Zoom beim Ziehen an der Ecke: das Panel wächst und schrumpft proportional, seine
/// Ecke folgt dem Zeiger so dicht wie möglich (Zeigerweg auf die Diagonale projiziert).
/// `start_size` ist die Fenstergröße beim Zugreifen, `delta` der Zeigerweg seither.
pub fn zoom_from_drag(start_zoom: f64, start_size: (i32, i32), delta: (i32, i32)) -> f64 {
    let (w, h) = (f64::from(start_size.0.max(1)), f64::from(start_size.1.max(1)));
    let f = 1.0 + (f64::from(delta.0) * w + f64::from(delta.1) * h) / (w * w + h * h);
    (start_zoom * f).clamp(ZOOM_MIN, ZOOM_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Area = Area::new(0, 0, 1920, 1040);

    #[test]
    fn a_panel_too_large_for_the_monitor_is_scaled_down_not_cut() {
        let z = fit_zoom((900.0, 2080.0), WORK, 1.0);
        assert!((z - 0.5).abs() < 1e-9, "{z}");
        assert_eq!(fit_zoom((900.0, 400.0), WORK, 1.0), 1.0, "passt: bleibt natürlich");
        assert_eq!(fit_zoom((900.0, 400.0), WORK, 1.4), 1.4, "der gezogene Zoom gilt, solange er passt");
        let z = fit_zoom((900.0, 400.0), WORK, 3.0);
        assert!((z - 2.0).abs() < 1e-9, "gezogener Zoom hat Grenzen");
    }

    #[test]
    fn dragging_the_corner_scales_along_the_diagonal() {
        assert_eq!(zoom_from_drag(1.0, (900, 400), (0, 0)), 1.0);
        let grown = zoom_from_drag(1.0, (900, 400), (90, 40));
        assert!((grown - 1.1).abs() < 1e-9, "{grown}");
        let shrunk = zoom_from_drag(1.0, (900, 400), (-450, -200));
        assert!((shrunk - 0.5).abs() < 1e-9, "{shrunk}");
        assert_eq!(zoom_from_drag(1.0, (900, 400), (-5000, -5000)), ZOOM_MIN);
    }
}
