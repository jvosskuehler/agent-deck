//! Die vier Designs des Fokus-Rahmens: was je Bild wechselt (Port von
//! `focus-frame/Designs.cs` der Python-Fassung, Stand 2026-11-06 23:19).
//!
//! Hier steht alles, was je Bild einmal gerechnet wird (Lage der Kometen, Lichtbögen,
//! Funken), in Doppelgenauigkeit; die Pixel rechnet der Shader `frame.hlsl` auf der
//! Grafikkarte ([`crate::frame_gpu`]). Maße sind Punkte mal Skalierung,
//! Geschwindigkeiten Punkte je Sekunde: so wirkt ein Design auf einem großen Fenster
//! genauso schnell wie auf der Tafel.
//!
//! [`FrameParams`] hat bewusst genau den Aufbau des `cbuffer Params` aus `frame.hlsl`
//! (`#[repr(C)]`, 12 × float4): die Parameter gehen unverändert in den Shader.

/// Die Designs, Tafeln 03 bis 06 im Canvas „Fokus-Rahmen Varianten“ (2026-11-06).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Design {
    /// Ein oranger und ein goldener Komet laufen gegeneinander; wo sie sich treffen
    /// (oben und unten in der Mitte), blitzt der Hof auf.
    Kometen,
    /// Glühender Verlauf fließt langsam herum, der Hof flackert, oben steigen Funken auf.
    Glut,
    /// Cyan wie die Deck-Kante: flackernder Draht, Lichtbögen springen über den Rand,
    /// zwei Funken rasen gegeneinander herum.
    Hochspannung,
    /// Glühbirnen wie am Kinoschild, jede dritte brennt hell, das Licht hüpft im Takt.
    Lauflicht,
    // Die Winter-Designs (Canvas „Weihnachts-Terminals“, 2026-11-10) zählen ab 10: so
    // erkennt sie der Shader, und neue Stammdesigns können darunter weiterzählen.
    /// Eisrand, auf der Oberkante wächst Schnee, unten wachsen Eiszapfen und tropfen.
    Schneewehe = 10,
    /// Grüne, türkise und violette Lichtvorhänge wehen um das Fenster.
    Nordlicht = 11,
    /// Eine kleine Dampflok zieht vier Wagen um das Fenster, hinter ihr glüht der Rand nach.
    Express = 12,
}

impl Design {
    /// Die Stammdesigns: laufen das ganze Jahr außer in der Winterzeit.
    pub const ALL: [Design; 4] = [
        Design::Kometen,
        Design::Glut,
        Design::Hochspannung,
        Design::Lauflicht,
    ];

    /// Das Winterkleid, vom ersten Advent bis Dreikönig (`deck_core::season`). Mit Schnee
    /// über dem ganzen Fenster, siehe [`crate::frame_fx_winter`].
    pub const WINTER: [Design; 3] = [Design::Schneewehe, Design::Nordlicht, Design::Express];

    /// Aus welchen Designs gerade gewählt wird.
    pub const fn pool(winter: bool) -> &'static [Design] {
        if winter { &Design::WINTER } else { &Design::ALL }
    }

    pub const fn is_winter(self) -> bool {
        self.id() >= 10
    }

    /// Nummer im Shader (`misc.z`).
    pub const fn id(self) -> u32 {
        self as u32
    }

    /// Design für den nächsten Fokus: zufällig aus `pool`, nie zweimal dasselbe
    /// hintereinander (sonst wirkt der Wechsel wie keiner). `random` ist eine beliebige
    /// Zufallszahl des Aufrufers; die Crate bringt keinen Zufallsgenerator mit.
    pub fn pick(pool: &[Design], last: Option<Design>, random: u64) -> Option<Design> {
        let fresh: Vec<Design> = pool.iter().copied().filter(|&d| Some(d) != last).collect();
        let from = if fresh.is_empty() {
            pool.to_vec()
        } else {
            fresh
        };
        (!from.is_empty()).then(|| from[(random % from.len() as u64) as usize])
    }
}

/// Wie weit der Hof über die Fensterkante hinausreicht, in Punkten.
pub const MARGIN_PT: f64 = 36.0;

/// Was der Renderer je Bild bekommt; Aufbau wie `cbuffer Params` in `Frame.hlsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameParams {
    /// Agentenfenster im Rahmenbild: x0, y0, x1, y1 (Pixel).
    pub win: [f32; 4],
    /// Skalierung, Umfang P, Design (0..3, Winter ab 10), Zeit (mod 1000 s, Winter mod
    /// 3600 s: dort treibt sie Schnee und Licht, ein Umbruch ist ein sichtbarer Sprung).
    pub misc: [f32; 4],
    pub a: [f32; 4],
    /// Je Design; `b[3]` ist bei allen frei für hell oder dunkel ([`FrameParams::with_night`]).
    pub b: [f32; 4],
    /// Funken bzw. Lichtbögen.
    pub items: [[f32; 4]; 8],
}

impl FrameParams {
    /// Dunkles Terminal? Die Winter-Designs malen ihre Flocken dann weiß, auf hellem
    /// Papier eisblau (weiße wären dort unsichtbar). Die Stammdesigns lesen es nicht.
    pub fn with_night(mut self, night: bool) -> FrameParams {
        self.b[3] = if night { 1.0 } else { 0.0 };
        self
    }
}

pub(crate) fn modp(x: f64, m: f64) -> f64 {
    x - m * (x / m).floor()
}

/// Zufallszahl in [0, 1), fest je (seed, a, b): dieselben Funken und Bögen im selben
/// Takt, ohne je Bild einen Zufallsgenerator anzulegen. Bitgenau wie `Designs.Hash`.
pub fn hash(seed: i32, a: i64, b: i64) -> f64 {
    let mut x = (seed as i64 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (a as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9)
        ^ (b as u64).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    x = x.wrapping_mul(0x7FB5_D329_728E_A185);
    x ^= x >> 27;
    x = x.wrapping_mul(0x81DA_DEF4_BC2D_D44D);
    x ^= x >> 33;
    (x >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
}

/// Parameter für ein Bild. `t` Sekunden seit Fokusbeginn, `sc` Skalierung des Monitors,
/// `win` das Agentenfenster im Rahmenbild, `width`/`height` seine Größe in Pixeln.
pub fn compute(
    design: Design,
    seed: i32,
    t: f64,
    sc: f64,
    win: [f32; 4],
    width: f64,
    height: f64,
) -> FrameParams {
    let perimeter = 2.0 * (width + height);
    let mut p = FrameParams {
        win,
        misc: [
            sc as f32,
            perimeter as f32,
            design.id() as f32,
            modp(t, if design.is_winter() { 3600.0 } else { 1000.0 }) as f32,
        ],
        ..Default::default()
    };
    match design {
        Design::Kometen => kometen(&mut p, t, sc, perimeter, width),
        Design::Glut => glut(&mut p, seed, t, sc, perimeter, win, width),
        Design::Hochspannung => hochspannung(&mut p, seed, t, sc, perimeter),
        Design::Lauflicht => lauflicht(&mut p, t, sc),
        Design::Schneewehe | Design::Nordlicht | Design::Express => {
            crate::frame_fx_winter::fill(&mut p, design, t, width / sc, height / sc)
        }
    }
    p
}

fn v4(a: f64, b: f64, c: f64, d: f64) -> [f32; 4] {
    [a as f32, b as f32, c as f32, d as f32]
}

fn kometen(p: &mut FrameParams, t: f64, sc: f64, perim: f64, width: f64) {
    let lap = perim / (430.0 * sc);
    let ph = modp(t / lap, 1.0);
    let start = width / 2.0;
    let head_a = modp(start + ph * perim, perim);
    let head_b = modp(start - ph * perim, perim);
    let near = ph.min((ph - 0.5).abs()).min(1.0 - ph) * lap;
    let amp = (-(near / 0.2).powi(2)).exp();
    let burst_at = if !(0.25..=0.75).contains(&ph) {
        start
    } else {
        modp(start + perim / 2.0, perim)
    };
    p.a = v4(head_a, head_b, 0.17 * perim, amp);
    p.b = v4(burst_at, 10.0 * sc, 0.0, 0.0);
}

fn glut(p: &mut FrameParams, seed: i32, t: f64, sc: f64, perim: f64, win: [f32; 4], width: f64) {
    use std::f64::consts::PI;
    let n = 0.5
        + 0.25 * (2.0 * PI * 0.83 * t).sin()
        + 0.15 * (2.0 * PI * 2.3 * t + 1.0).sin()
        + 0.1 * (2.0 * PI * 5.1 * t + 2.0).sin();
    p.a = v4(modp(t * 107.0 * sc, perim), 0.55 + 0.45 * n, 0.0, 0.0);
    for k in 0..7i64 {
        let period = 2.1 + hash(seed, k, 1) * 1.1;
        let phase = hash(seed, k, 2) * 3.0;
        let u = modp((t + phase) / period, 1.0);
        let cycle = ((t + phase) / period).floor() as i64;
        let xf = 0.05 + 0.9 * hash(seed, k, cycle);
        let alpha = (u / 0.12).min(1.0) * (1.0 - u);
        p.items[k as usize] = v4(
            f64::from(win[0]) + xf * width + 9.0 * sc * u,
            f64::from(win[1]) - u * 26.0 * sc,
            alpha,
            1.4 * sc,
        );
    }
}

const FLICK: [(f64, f64); 7] = [
    (0.0, 1.0),
    (0.12, 0.55),
    (0.2, 1.0),
    (0.43, 0.8),
    (0.5, 1.0),
    (0.71, 0.4),
    (0.76, 1.0),
];
const LEVELS: [f64; 4] = [0.5, 0.65, 0.8, 1.0];

fn hochspannung(p: &mut FrameParams, seed: i32, t: f64, sc: f64, perim: f64) {
    let ph = modp(t / 0.9, 1.0);
    let flick = FLICK
        .iter()
        .rev()
        .find(|(at, _)| ph >= *at)
        .map_or(1.0, |f| f.1);
    let tick = (t / 0.06).floor() as i64; // so oft springen die Bögen
    let n = 5 + (hash(seed, 99, tick) * 4.0) as i32;
    let level = LEVELS[((hash(seed, 98, tick) * LEVELS.len() as f64) as usize) % LEVELS.len()];
    for k in 0..8i64 {
        p.items[k as usize] = v4(
            hash(seed, k, tick * 4) * perim,
            (10.0 + hash(seed, k, tick * 4 + 1) * 50.0) * sc,
            (hash(seed, k, tick * 4 + 2) - 0.5) * 2.0 * sc,
            hash(seed, k, tick * 4 + 3) * 100.0,
        );
    }
    let z1 = modp(t * 1010.0 * sc, perim);
    let z2 = modp(-t * 660.0 * sc, perim);
    p.a = v4(flick, level, z1, z2);
    p.b = v4(0.45 * 12.0 * sc, f64::from(n), tick as f64, 0.0);
}

fn lauflicht(p: &mut FrameParams, t: f64, sc: f64) {
    let phase = (t / (0.42 / 3.0)).floor() % 3.0;
    // C# Math.Round rundet halbe Werte zur geraden Zahl; so auch hier.
    p.a = v4(
        phase,
        (8.0 * sc).round_ties_even().max(6.0),
        (2.6 * sc).max(1.5),
        0.0,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_have_the_exact_layout_of_the_shader_cbuffer() {
        assert_eq!(size_of::<FrameParams>(), 12 * 16);
    }

    #[test]
    fn the_same_design_never_comes_twice_in_a_row() {
        for r in 0..40 {
            let d = Design::pick(&Design::ALL, Some(Design::Glut), r).unwrap();
            assert_ne!(d, Design::Glut);
        }
        assert_eq!(
            Design::pick(&[Design::Glut], Some(Design::Glut), 7),
            Some(Design::Glut)
        );
        assert_eq!(Design::pick(&[], None, 7), None);
    }

    #[test]
    fn hash_is_deterministic_in_unit_range_and_varies() {
        let a = hash(42, 3, 17);
        assert_eq!(a, hash(42, 3, 17));
        assert!((0.0..1.0).contains(&a));
        assert_ne!(a, hash(42, 3, 18));
        assert_ne!(a, hash(-42, 3, 17));
        let mean: f64 = (0..2000).map(|i| hash(7, i, 0)).sum::<f64>() / 2000.0;
        assert!((mean - 0.5).abs() < 0.03, "gleichverteilt: {mean}");
    }

    #[test]
    fn comets_start_in_the_middle_of_the_top_edge_and_flash_there() {
        let p = compute(
            Design::Kometen,
            0,
            0.0,
            1.0,
            [0.0, 0.0, 800.0, 600.0],
            800.0,
            600.0,
        );
        assert_eq!(
            (p.a[0], p.a[1]),
            (400.0, 400.0),
            "beide Köpfe oben in der Mitte"
        );
        assert_eq!(p.a[3], 1.0, "beim Treffen voller Blitz");
        assert_eq!(p.misc[1], 2800.0, "Umfang");
    }

    #[test]
    fn comets_meet_again_at_the_bottom_after_half_a_lap() {
        let (w, h, sc) = (800.0, 600.0, 1.0);
        let lap = 2.0 * (w + h) / (430.0 * sc);
        let p = compute(
            Design::Kometen,
            0,
            lap / 2.0,
            sc,
            [0.0, 0.0, 800.0, 600.0],
            w,
            h,
        );
        assert!((p.a[0] - p.a[1]).abs() < 0.5, "Köpfe treffen sich");
        assert!(
            (p.b[0] - 1800.0).abs() < 0.5,
            "Blitz unten in der Mitte (400 + P/2)"
        );
    }

    #[test]
    fn high_voltage_shows_five_to_eight_arcs() {
        for t in [0.0, 0.07, 1.3, 9.99] {
            let p = compute(Design::Hochspannung, 11, t, 1.5, [0.0; 4], 1000.0, 700.0);
            assert!((5.0..=8.0).contains(&p.b[1]), "{}", p.b[1]);
            assert!(
                LEVELS.map(|l| l as f32).contains(&p.a[1]),
                "Hof-Stufe aus der Tabelle"
            );
        }
    }

    #[test]
    fn marquee_bulb_spacing_rounds_half_to_even_like_csharp() {
        // 8 * 1.0625 = 8.5 -> C# Math.Round ergibt 8, nicht 9.
        assert_eq!(
            compute(Design::Lauflicht, 0, 0.0, 1.0625, [0.0; 4], 10.0, 10.0).a[1],
            8.0
        );
        assert_eq!(
            compute(Design::Lauflicht, 0, 0.0, 0.5, [0.0; 4], 10.0, 10.0).a[1],
            6.0,
            "mindestens 6 px"
        );
        let phases: Vec<f32> = [0.0, 0.15, 0.29, 0.43]
            .iter()
            .map(|&t| compute(Design::Lauflicht, 0, t, 1.0, [0.0; 4], 10.0, 10.0).a[0])
            .collect();
        assert_eq!(phases, vec![0.0, 1.0, 2.0, 0.0]);
    }

    #[test]
    fn embers_rise_above_the_top_edge() {
        let win = [50.0, 60.0, 850.0, 660.0];
        for t in [0.3, 1.7, 4.2] {
            let p = compute(Design::Glut, 5, t, 1.0, win, 800.0, 600.0);
            for s in &p.items[..7] {
                assert!(
                    s[1] <= 60.0 && s[1] >= 60.0 - 26.0,
                    "Funke über der Oberkante: {s:?}"
                );
                assert!((0.0..=1.0).contains(&s[2]));
            }
        }
    }
}
