//! Die Winter-Designs des Fokus-Rahmens: was je Bild wechselt (Canvas
//! „Weihnachts-Terminals“, Runde 2, 2026-11-10). Die Pixel rechnet `frame_winter.hlsl`.
//!
//! Anders als die Stammdesigns rechnen diese in Punkten (Pixel / Skalierung) und treiben
//! Schnee und Licht im Shader aus der Zeit in `misc.w`. Hier steht nur, was über lange
//! Zeit genau bleiben muss oder am Anfang eines Fokus anders ist:
//!
//! - Schneewehe: `a.x` wie hoch Schnee und Eiszapfen schon gewachsen sind (0 bis 1),
//!   `a.y` die Schneemenge (beim Fokuswechsel wirbelt es wie in einer geschüttelten
//!   Schneekugel, dann beruhigt es sich auf 1).
//! - Express: `a.x` wo die Lok auf dem Gleis ist (Punkte ab dem Anfang der Oberkante).
//! - Nordlicht: nichts.

use crate::frame_fx::{Design, FrameParams, modp};

/// So weit außerhalb der Fensterkante liegt das Gleis, und mit diesem Radius nimmt es die
/// Ecken (Punkte). Dieselben Zahlen wie `TRACK_OUT` und `TRACK_R` im Shader.
const TRACK_OUT: f64 = 4.5;
const TRACK_R: f64 = 14.0;
/// Fahrt der Lok in Punkten je Sekunde: ein Fenster von 680 × 480 umrundet sie in 27 s.
const TRAIN_SPEED: f64 = 85.0;

/// Länge einer Runde auf dem Gleis um ein Fenster von `w` × `h` Punkten.
pub fn track_length(w: f64, h: f64) -> f64 {
    let straight = |side: f64| side - 2.0 * (TRACK_R - TRACK_OUT);
    2.0 * (straight(w) + straight(h)) + std::f64::consts::TAU * TRACK_R
}

/// Wie weit die Schneewehe `t` Sekunden nach dem Fokus gewachsen ist (0 bis 1).
pub fn grown(t: f64) -> f64 {
    1.0 - (-t / 9.0).exp()
}

/// Schneemenge `t` Sekunden nach dem Fokus: erst das 2,4-fache, nach wenigen Sekunden 1.
pub fn flurry(t: f64) -> f64 {
    1.0 + 1.4 * (-t / 2.5).exp()
}

/// Parameter eines Winter-Designs; `w` und `h` sind das Fenster in Punkten.
pub(crate) fn fill(p: &mut FrameParams, design: Design, t: f64, w: f64, h: f64) {
    match design {
        Design::Schneewehe => p.a = [grown(t) as f32, flurry(t) as f32, 0.0, 0.0],
        Design::Express => p.a = [modp(t * TRAIN_SPEED, track_length(w, h)) as f32, 0.0, 0.0, 0.0],
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame_fx::compute;

    #[test]
    fn winter_designs_carry_the_numbers_the_shader_dispatches_on() {
        assert_eq!(Design::WINTER.map(Design::id), [10, 11, 12]);
        assert!(Design::WINTER.iter().all(|d| d.is_winter()));
        assert!(Design::ALL.iter().all(|d| !d.is_winter()));
        assert_eq!(Design::pool(true), &Design::WINTER);
        assert_eq!(Design::pool(false), &Design::ALL);
    }

    #[test]
    fn the_snowdrift_starts_bare_and_settles() {
        assert_eq!(grown(0.0), 0.0);
        assert!(grown(9.0) > 0.6 && grown(60.0) > 0.99 && grown(600.0) <= 1.0);
        assert!((flurry(0.0) - 2.4).abs() < 1e-9, "Schneekugel beim Fokuswechsel");
        assert!(flurry(20.0) < 1.01 && flurry(20.0) >= 1.0);
        let p = compute(Design::Schneewehe, 0, 9.0, 1.5, [0.0; 4], 1020.0, 720.0);
        assert_eq!((p.a[0], p.a[1]), (grown(9.0) as f32, flurry(9.0) as f32));
    }

    #[test]
    fn the_train_runs_laps_at_the_same_speed_on_every_monitor() {
        let lap = track_length(680.0, 480.0);
        assert!((lap - (2.0 * (661.0 + 461.0) + std::f64::consts::TAU * 14.0)).abs() < 1e-9);
        for sc in [1.0, 1.5, 2.0] {
            let at = |t: f64| compute(Design::Express, 0, t, sc, [0.0; 4], 680.0 * sc, 480.0 * sc).a[0];
            assert_eq!(at(0.0), 0.0);
            assert!((at(2.0) - 170.0).abs() < 1e-3, "85 Punkte je Sekunde bei Skalierung {sc}");
            let after_a_lap = f64::from(at(lap / TRAIN_SPEED + 1.0));
            assert!((after_a_lap - 85.0).abs() < 1e-2, "nach einer Runde wieder vorn: {after_a_lap}");
        }
    }

    #[test]
    fn winter_time_wraps_only_once_an_hour() {
        let time = |d, t| compute(d, 0, t, 1.0, [0.0; 4], 800.0, 600.0).misc[3];
        assert_eq!(time(Design::Nordlicht, 1500.0), 1500.0);
        assert_eq!(time(Design::Nordlicht, 3601.0), 1.0);
        assert_eq!(time(Design::Glut, 1500.0), 500.0, "Stammdesigns wie bisher");
    }

    #[test]
    fn night_goes_into_the_slot_no_design_uses() {
        for d in Design::ALL.into_iter().chain(Design::WINTER) {
            let p = compute(d, 3, 1.2, 1.0, [0.0; 4], 800.0, 600.0);
            assert_eq!(p.b[3], 0.0, "{d:?} lässt b.w frei");
            assert_eq!(p.with_night(true).b[3], 1.0);
            assert_eq!(p.with_night(true).with_night(false), p);
        }
    }
}
