//! Der Fokus-Rahmen, Pixel für Pixel (Port von `focus-frame/Frame.hlsl` der
//! Python-Fassung, Stand 2026-11-06 23:19, auf die CPU).
//!
//! Jeder Pixel fragt: wie tief liege ich im Agentenfenster (`u`, innen positiv, außen
//! negativ) und an welcher Stelle des Umfangs (`s`, im Uhrzeigersinn ab der Ecke oben
//! links)? Daraus ergeben sich Linie und Leuchthof analytisch, ohne Pixelraster
//! dazwischen: Kanten geglättet, Verläufe stufenlos, in voller Monitorauflösung. Was je
//! Bild wechselt, kommt aus [`crate::frame_fx`].
//!
//! Ausgabe ist vormultipliziertes BGRA, genau was `UpdateLayeredWindow` will.
//!
//! **Nicht für die Daueranimation gedacht:** gemessen kostet ein Bild um ein
//! 2560x1400-Fenster bei 150 % (720 000 Pixel in den Streifen) 10 bis 20 ms auf acht
//! Threads, bei 30 Bildern je Sekunde also mehrere Kerne. Die App zeichnet den Rahmen
//! mit [`crate::frame_gpu_overlay`]; dieser Port ist der Prüfstein für den Shader
//! (Test `frame_gpu::gpu_and_cpu_agree`, Abweichung höchstens eine Stufe) und ein
//! Rückfall, etwa für ein Standbild.
//!
//! Zwei Abweichungen vom Shader, beide unsichtbar: Das Rauschen gegen Stufen im Hof
//! kommt aus einem Ganzzahl-Hash statt aus `sin` (auf der GPU billig, hier nicht; nur
//! das Muster ist anders, nicht die Stärke). Und Beiträge, die nachweislich null sind
//! (Pixel weit weg von einem Lichtbogen oder Funken), werden übersprungen statt
//! ausgerechnet.

use crate::frame_fx::{Design, FrameParams, INNER_PT};
use crate::types::Rect;

type V3 = [f32; 3];

const ORANGE: V3 = [217.0 / 255.0, 119.0 / 255.0, 87.0 / 255.0];
const EMBER: [V3; 8] = [
    [124.0 / 255.0, 45.0 / 255.0, 18.0 / 255.0],
    [194.0 / 255.0, 65.0 / 255.0, 12.0 / 255.0],
    [217.0 / 255.0, 119.0 / 255.0, 87.0 / 255.0],
    [245.0 / 255.0, 158.0 / 255.0, 11.0 / 255.0],
    [253.0 / 255.0, 230.0 / 255.0, 138.0 / 255.0],
    [245.0 / 255.0, 158.0 / 255.0, 11.0 / 255.0],
    [217.0 / 255.0, 119.0 / 255.0, 87.0 / 255.0],
    [194.0 / 255.0, 65.0 / 255.0, 12.0 / 255.0],
];

/// Ab `exp(-x)` mit x darüber ist der Beitrag kleiner als ein Hundertmillionstel.
const NEGLIGIBLE: f32 = 18.0;

fn sat(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}
fn frac(x: f32) -> f32 {
    x - x.floor()
}
fn mix(a: V3, b: V3, t: f32) -> V3 {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}
fn add(acc: &mut [f32; 4], c: V3, w: f32) {
    acc[0] += c[0] * w;
    acc[1] += c[1] * w;
    acc[2] += c[2] * w;
}
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = sat((x - e0) / (e1 - e0));
    t * t * (3.0 - 2.0 * t)
}
/// `exp(-(d/s)²)`, bei vernachlässigbarem Ergebnis ohne `exp`.
fn gauss(d: f32, s: f32) -> f32 {
    let x = (d / s) * (d / s);
    if x > NEGLIGIBLE { 0.0 } else { (-x).exp() }
}
fn hash11(x: f32) -> f32 {
    frac(((x * 127.1 + 311.7).sin()) * 43758.547)
}
fn noise1(x: f32) -> f32 {
    let (i, f) = (x.floor(), frac(x));
    let u = f * f * (3.0 - 2.0 * f);
    let (a, b) = (hash11(i), hash11(i + 1.0));
    a + (b - a) * u
}
/// Abstand auf dem Umfang über die Naht bei s = 0 hinweg, Ergebnis in (-P/2, P/2].
fn wrapd(x: f32, p: f32) -> f32 {
    x - p * (x / p).round_ties_even()
}

/// Tiefe `u` und Umfangslage `s` eines Punktes relativ zum Fenster `win` (x0, y0, x1, y1).
pub fn edge(px: f32, py: f32, win: &[f32; 4]) -> (f32, f32) {
    let (qx, qy) = (px - win[0], py - win[1]);
    let (w, h) = (win[2] - win[0], win[3] - win[1]);
    let (cx, cy) = (qx.clamp(0.0, w), qy.clamp(0.0, h));
    if qx >= 0.0 && qy >= 0.0 && qx <= w && qy <= h {
        let (dl, dt, dr, db) = (qx, qy, w - qx, h - qy);
        let m = dl.min(dr).min(dt.min(db));
        let s = if m == dt {
            qx
        } else if m == dr {
            w + qy
        } else if m == db {
            w + h + (w - qx)
        } else {
            2.0 * w + h + (h - qy)
        };
        (m, s)
    } else {
        let u = -((qx - cx).powi(2) + (qy - cy).powi(2)).sqrt();
        let s = if cy <= 0.0 && cx < w {
            cx
        } else if cx >= w && cy < h {
            w + cy
        } else if cy >= h && cx > 0.0 {
            w + h + (w - cx)
        } else {
            2.0 * w + h + (h - cy)
        };
        (u, s)
    }
}

/// Deckung der Linie zwischen den Tiefen lo und hi, an den Kanten geglättet.
pub fn stroke(u: f32, lo: f32, hi: f32) -> f32 {
    sat((u - lo).min(hi - u) + 0.5)
}

/// Leuchthof um die Linie: nach außen eng (s1) plus weit (s2), nach innen nur eng, weil
/// ein breiter Hof über dem Terminaltext ihn verschleiern würde.
pub fn halo(u: f32, lo: f32, hi: f32, s1: f32, a1: f32, s2: f32, a2: f32) -> f32 {
    if u < lo {
        let d = lo - u;
        a1 * gauss(d, s1) + a2 * gauss(d, s2)
    } else if u > hi {
        a1 * gauss(u - hi, s1)
    } else {
        a1 + a2
    }
}

fn comet(k: f32, len: f32, soft: f32) -> f32 {
    if k <= -soft || k >= len {
        return 0.0;
    }
    smoothstep(-soft, soft, k) * sat(1.0 - k.max(0.0) / len).powf(1.6)
}

fn kometen(p: &FrameParams, u: f32, s: f32, perim: f32, sc: f32) -> [f32; 4] {
    let (len, hot, amp) = (p.a[2], p.b[1], p.a[3]);
    let ka = wrapd(p.a[0] - s, perim);
    let kb = wrapd(s - p.a[1], perim);
    let ca = mix([1.0, 0.988, 0.973], ORANGE, sat(ka / hot));
    let cb = mix([1.0, 1.0, 1.0], [1.0, 0.769, 0.541], sat(kb / hot));
    let blur = 7.0 * sc;
    let lin = stroke(u, 0.0, 3.0 * sc);
    let bump = amp * gauss(wrapd(s - p.b[0], perim), (0.03 * perim).max(6.0));
    let hal = halo(u, 0.0, 3.0 * sc, 5.0 * sc, 1.0, 15.0 * sc, 0.55) * (0.75 + 0.25 * amp);
    let (ia, ib) = (comet(ka, len, 0.7), comet(kb, len, 0.7));
    let (ga, gb) = (comet(ka, len, blur), comet(kb, len, blur));
    let burst = [1.0, 0.925, 0.863];
    let mut c = [0.0; 4];
    add(&mut c, ORANGE, 0.22 * lin);
    add(&mut c, ca, ia * lin + ga * hal);
    add(&mut c, cb, ib * lin + gb * hal);
    add(&mut c, burst, bump * (lin + hal));
    c[3] = 0.22 * lin + (ia + ib + bump) * lin + (ga + gb + bump) * hal;
    c
}

fn glut(p: &FrameParams, px: f32, py: f32, u: f32, s: f32, perim: f32, sc: f32) -> [f32; 4] {
    let x = frac((s - p.a[0]) / perim * 2.0) * 8.0;
    let i = x.floor() as i32;
    let col = mix(
        EMBER[i.rem_euclid(8) as usize],
        EMBER[(i + 1).rem_euclid(8) as usize],
        frac(x),
    );
    let lin = stroke(u, 0.0, 4.0 * sc);
    let hal = halo(u, 0.0, 4.0 * sc, 5.0 * sc, 0.9, 16.0 * sc, 0.5) * p.a[1];
    let w = lin + hal;
    let mut c = [col[0] * w, col[1] * w, col[2] * w, w];
    for sp in &p.items[..7] {
        let (dx, dy) = (px - sp[0], py - sp[1]);
        let r2 = (dx * dx + dy * dy) / (sp[3] * sp[3]);
        if r2 > NEGLIGIBLE {
            continue;
        }
        let g = sp[2] * (-r2).exp();
        add(&mut c, [1.0, 0.886, 0.659], g);
        c[3] += g;
    }
    c
}

fn hochspannung(p: &FrameParams, u: f32, s: f32, perim: f32, sc: f32) -> [f32; 4] {
    let (cyan, arc_c) = ([0.494, 0.796, 1.0], [0.749, 0.902, 1.0]);
    let (flick, level, zs) = (p.a[0], p.a[1], p.b[0]);
    let wire = stroke(u, 0.0, 2.0 * sc) * 0.7 * flick;
    let hal = halo(u, 0.0, 2.0 * sc, 4.0 * sc, 1.0, 13.0 * sc, 0.5) * level;
    let mut c = [0.0; 4];
    add(&mut c, [0.902, 0.965, 1.0], wire);
    add(&mut c, cyan, hal * 0.45 * flick);
    c[3] = wire + hal * 0.45 * flick;
    let blur = 5.0 * sc;
    for arc in p.items.iter().take(p.b[1].max(0.0) as usize) {
        let mut ds = s - arc[0];
        ds -= perim * (ds / perim).floor();
        let len = arc[1];
        if ds >= len + blur {
            continue; // weder scharfer noch weicher Teil: Beitrag ist genau null
        }
        let edge_len = (len / 4.0).max(2.0);
        let sharp = sat(((ds + 1.0) / edge_len).min((len - ds) / edge_len));
        let soft =
            sat(((ds + blur) / (edge_len + blur)).min((len - ds + blur) / (edge_len + blur)));
        // Der Bogen zittert quer zur Kante: Rauschen entlang s, neu je Takt.
        let wob = (noise1(s * 0.35 / sc + arc[3]) - 0.5) * 3.0 * sc + arc[2];
        let l = stroke(u - wob, 0.0, 2.0 * sc) * sharp;
        let h = halo(u - wob, 0.0, 2.0 * sc, 4.0 * sc, 1.0, 13.0 * sc, 0.5) * soft * level;
        add(&mut c, arc_c, l);
        add(&mut c, cyan, h);
        c[3] += l + h;
    }
    for pos in [p.a[2], p.a[3]] {
        let g = gauss(wrapd(s - pos, perim), zs);
        if g == 0.0 {
            continue;
        }
        let l = stroke(u, -0.5 * sc, 2.5 * sc) * g;
        let h = halo(u, 0.0, 2.0 * sc, 4.0 * sc, 1.0, 13.0 * sc, 0.5) * g;
        add(&mut c, [1.0, 1.0, 1.0], l);
        add(&mut c, [0.85, 0.95, 1.0], h);
        c[3] += l + h;
    }
    c
}

fn lauflicht(p: &FrameParams, u: f32, s: f32, perim: f32, sc: f32) -> [f32; 4] {
    let (gap, rad, phase) = (p.a[1], p.a[2], p.a[0] as i32);
    let rail = stroke(u, -0.6 * sc, 0.6 * sc) * 0.22;
    let mut c = [ORANGE[0] * rail, ORANGE[1] * rail, ORANGE[2] * rail, rail];
    let j0 = (s / gap).floor();
    for k in -2..=2 {
        let j = j0 + k as f32;
        let ds = wrapd(s - (j + 0.5) * gap, perim);
        let r = (ds * ds + u * u).sqrt();
        let core = gauss(r, rad / 1.2f32.sqrt());
        if ((j as i32 + 3000) - phase) % 3 == 0 {
            let col = mix([1.0, 0.965, 0.933], ORANGE, sat(r / rad));
            let g = 0.8 * gauss(r, 4.0 * sc) + 0.45 * gauss(r, 11.0 * sc);
            add(&mut c, col, core);
            add(&mut c, ORANGE, g);
            c[3] += core + g;
        } else {
            let g = 0.25 * gauss(r, 4.0 * sc);
            add(&mut c, ORANGE, 0.42 * core + g);
            c[3] += 0.42 * core + g;
        }
    }
    c
}

fn design_of(p: &FrameParams) -> Design {
    match p.misc[2] as u32 {
        0 => Design::Kometen,
        1 => Design::Glut,
        2 => Design::Hochspannung,
        _ => Design::Lauflicht,
    }
}

/// Farbe eines Pixelmittelpunkts (px, py) im Rahmenbild, vormultipliziert und VOR dem
/// Rauschen und Runden: (r, g, b, a), Werte nicht geklemmt.
pub fn shade(p: &FrameParams, px: f32, py: f32) -> [f32; 4] {
    let (u, s) = edge(px, py, &p.win);
    let (sc, perim) = (p.misc[0], p.misc[1]);
    // Tief im Fenster leuchtet nichts mehr.
    if u > INNER_PT as f32 * sc {
        return [0.0; 4];
    }
    match design_of(p) {
        Design::Kometen => kometen(p, u, s, perim, sc),
        Design::Glut => glut(p, px, py, u, s, perim, sc),
        Design::Hochspannung => hochspannung(p, u, s, perim, sc),
        Design::Lauflicht => lauflicht(p, u, s, perim, sc),
    }
}

/// Rauschen in [-0.5, 0.5) / 255 je Pixel und Bild, gegen Stufen in den dunklen
/// Ausläufen des Hofs.
fn dither(x: i32, y: i32, frame: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8DA6_B343)
        ^ (y as u32).wrapping_mul(0xD816_3841)
        ^ frame.wrapping_mul(0xCB1A_B31F);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    ((h >> 8) as f32 / (1u32 << 24) as f32 - 0.5) / 255.0
}

/// Ein Pixel fertig für das Fenster: Rauschen, Klemmen, vormultipliziertes BGRA8.
fn finish(c: [f32; 4], x: i32, y: i32, frame: u32) -> [u8; 4] {
    if c[3] <= 0.0 {
        return [0; 4];
    }
    let n = dither(x, y, frame);
    let a = sat(c[3] + n);
    let ch = |v: f32| (sat(v + n).min(a) * 255.0 + 0.5) as u8;
    [ch(c[2]), ch(c[1]), ch(c[0]), (a * 255.0 + 0.5) as u8]
}

/// Den Ausschnitt `region` (Koordinaten im Rahmenbild) nach `out` zeichnen: Zeile für
/// Zeile von oben, je Pixel vier Bytes BGRA, vormultipliziert. `out` muss genau
/// `region.width() * region.height() * 4` Bytes lang sein.
pub fn render(p: &FrameParams, region: Rect, out: &mut [u8]) {
    let w = region.width().max(0) as usize;
    assert_eq!(
        out.len(),
        w * region.height().max(0) as usize * 4,
        "Puffergröße passt nicht zum Ausschnitt"
    );
    if w == 0 {
        return;
    }
    let cut = INNER_PT as f32 * p.misc[0];
    let win = p.win;
    let frame = p.misc[3].to_bits();
    // Spalten, die in einer tief innen liegenden Zeile sicher leer bleiben.
    let skip_lo = (win[0] + cut - 0.5).floor() as i32 + 1;
    let skip_hi = (win[2] - cut - 0.5).ceil() as i32;
    for (row, line) in out.chunks_exact_mut(w * 4).enumerate() {
        let y = region.top + row as i32;
        let py = y as f32 + 0.5;
        let deep = py - win[1] > cut && win[3] - py > cut;
        for (col, px_out) in line.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let x = region.left + col as i32;
            if deep && x >= skip_lo && x < skip_hi {
                *px_out = [0; 4];
                continue;
            }
            *px_out = finish(shade(p, x as f32 + 0.5, py), x, y, frame);
        }
    }
}

/// Wie [`render`], auf `threads` Threads verteilt (zeilenweise Streifen).
pub fn render_parallel(p: &FrameParams, region: Rect, out: &mut [u8], threads: usize) {
    let (w, h) = (
        region.width().max(0) as usize,
        region.height().max(0) as usize,
    );
    let threads = threads.clamp(1, h.max(1));
    if threads == 1 || w == 0 {
        return render(p, region, out);
    }
    let rows = h.div_ceil(threads);
    std::thread::scope(|scope| {
        for (i, chunk) in out.chunks_mut(rows * w * 4).enumerate() {
            let top = region.top + (i * rows) as i32;
            let sub = Rect::new(
                region.left,
                top,
                region.right,
                top + (chunk.len() / (w * 4)) as i32,
            );
            scope.spawn(move || render(p, sub, chunk));
        }
    });
}

#[cfg(test)]
#[path = "frame_render_tests.rs"]
mod tests;
