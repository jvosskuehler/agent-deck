//! Tests zu `frame_render.rs` (ausgelagert, damit beide Dateien unter 400 Zeilen bleiben).

use super::*;
use crate::frame_fx::{Design, compute};

/// Fenster 400x300 an (50, 50) in einem Rahmenbild 500x400, Skalierung 1.
const WIN: [f32; 4] = [50.0, 50.0, 450.0, 350.0];
const FRAME: Rect = Rect::new(0, 0, 500, 400);

fn params(d: Design, t: f64) -> FrameParams {
    compute(d, 7, t, 1.0, WIN, 400.0, 300.0)
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn depth_is_distance_to_the_nearest_edge_inside_and_negative_outside() {
    let (u, s) = edge(250.5, 60.5, &WIN);
    assert!(close(u, 10.5) && close(s, 200.5), "oben: {u} {s}");
    let (u, s) = edge(445.5, 200.5, &WIN);
    assert!(close(u, 4.5) && close(s, 400.0 + 150.5), "rechts: {u} {s}");
    let (u, _) = edge(250.5, 40.5, &WIN);
    assert!(close(u, -9.5), "über der Oberkante: {u}");
    let (u, s) = edge(47.0, 46.0, &WIN);
    assert!(close(u, -5.0), "Ecke oben links: Abstand zur Ecke, {u}");
    assert!(
        close(s, 1400.0) || close(s, 0.0),
        "Ecke oben links liegt auf der Naht: {s}"
    );
}

#[test]
fn position_runs_clockwise_from_the_top_left_corner() {
    // Oben nach rechts, rechts hinunter, unten nach links, links hinauf.
    let probes = [(100.5, 51.5), (449.5, 100.5), (300.5, 349.5), (50.5, 300.5)];
    let s: Vec<f32> = probes.iter().map(|&(x, y)| edge(x, y, &WIN).1).collect();
    assert!(s.windows(2).all(|w| w[0] < w[1]), "{s:?}");
    assert!(s[3] < 1400.0);
}

#[test]
fn stroke_is_full_on_the_line_and_soft_at_its_edges() {
    assert_eq!(stroke(1.5, 0.0, 3.0), 1.0);
    assert_eq!(stroke(10.0, 0.0, 3.0), 0.0);
    assert!(close(stroke(0.0, 0.0, 3.0), 0.5));
}

#[test]
fn halo_fades_outward_slower_than_inward() {
    let on = halo(1.0, 0.0, 3.0, 5.0, 1.0, 15.0, 0.55);
    assert!(close(on, 1.55));
    let out = halo(-10.0, 0.0, 3.0, 5.0, 1.0, 15.0, 0.55);
    let inn = halo(13.0, 0.0, 3.0, 5.0, 1.0, 15.0, 0.55);
    assert!(out > inn, "außen weit, innen nur eng: {out} > {inn}");
}

#[test]
fn deep_inside_the_window_nothing_glows() {
    for d in Design::ALL {
        assert_eq!(shade(&params(d, 1.0), 250.5, 200.5), [0.0; 4], "{d:?}");
    }
}

#[test]
fn every_design_lights_up_its_edge() {
    for d in Design::ALL {
        let p = params(d, 0.8);
        let mut buf = vec![0u8; (FRAME.width() * FRAME.height() * 4) as usize];
        render(&p, FRAME, &mut buf);
        let max_alpha = buf.as_chunks::<4>().0.iter().map(|px| px[3]).max().unwrap();
        assert!(max_alpha > 200, "{d:?}: hellster Pixel nur {max_alpha}");
    }
}

#[test]
fn far_outside_the_glow_the_frame_is_transparent() {
    // 36 pt Hof: in 200 px Abstand ist nichts mehr, auch nicht durch das Rauschen.
    for d in Design::ALL {
        let p = compute(d, 7, 0.8, 1.0, [300.0, 300.0, 700.0, 600.0], 400.0, 300.0);
        let mut buf = vec![0u8; 4 * 50 * 50];
        render(&p, Rect::new(0, 0, 50, 50), &mut buf);
        assert!(buf.iter().all(|&b| b == 0), "{d:?}");
    }
}

#[test]
fn output_is_premultiplied() {
    for d in Design::ALL {
        let mut buf = vec![0u8; (FRAME.width() * FRAME.height() * 4) as usize];
        render(&params(d, 2.3), FRAME, &mut buf);
        assert!(
            buf.as_chunks::<4>()
                .0
                .iter()
                .all(|px| px[0] <= px[3] && px[1] <= px[3] && px[2] <= px[3]),
            "{d:?}"
        );
    }
}

#[test]
fn a_region_renders_exactly_like_the_same_part_of_the_whole() {
    let p = params(Design::Kometen, 0.4);
    let mut full = vec![0u8; (FRAME.width() * FRAME.height() * 4) as usize];
    render(&p, FRAME, &mut full);
    let part = Rect::new(30, 20, 130, 90);
    let mut sub = vec![0u8; (part.width() * part.height() * 4) as usize];
    render(&p, part, &mut sub);
    for row in 0..part.height() {
        let start = (((part.top + row) * FRAME.width() + part.left) * 4) as usize;
        let len = (part.width() * 4) as usize;
        let mine = &sub[(row * part.width() * 4) as usize..][..len];
        assert_eq!(mine, &full[start..start + len], "Zeile {row}");
    }
}

#[test]
fn parallel_rendering_matches_single_threaded() {
    let p = params(Design::Hochspannung, 1.1);
    let n = (FRAME.width() * FRAME.height() * 4) as usize;
    let (mut one, mut many) = (vec![0u8; n], vec![0u8; n]);
    render(&p, FRAME, &mut one);
    render_parallel(&p, FRAME, &mut many, 7);
    assert!(one == many);
}

#[test]
fn skipped_interior_matches_full_evaluation() {
    // Der Spaltensprung tief innen darf nichts verschlucken, was shade() zeichnen würde.
    let p = params(Design::Lauflicht, 0.2);
    let mut buf = vec![0u8; (FRAME.width() * FRAME.height() * 4) as usize];
    render(&p, FRAME, &mut buf);
    for y in 0..FRAME.height() {
        for x in 0..FRAME.width() {
            let a = shade(&p, x as f32 + 0.5, y as f32 + 0.5)[3];
            if a > 0.01 {
                assert!(
                    buf[((y * FRAME.width() + x) * 4 + 3) as usize] > 0,
                    "({x}, {y}) fehlt"
                );
            }
        }
    }
}

#[test]
#[should_panic(expected = "Puffergröße")]
fn wrong_buffer_size_is_a_programming_error() {
    render(
        &params(Design::Glut, 0.0),
        Rect::new(0, 0, 10, 10),
        &mut [0u8; 12],
    );
}
