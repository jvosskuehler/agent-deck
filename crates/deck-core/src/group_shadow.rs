//! Ein Schatten für eine ganze Gruppe bündig liegender Agentenfenster.
//!
//! Windows gibt jedem Fenster seinen eigenen Schatten. Liegen die Agenten als Raster
//! Kante an Kante, fällt er nicht auf den Hintergrund, sondern auf den Nachbarn: jedes
//! Fenster hat dann an einer Seite einen dunklen Streifen. Darum bekommen bündig liegende
//! Fenster ihren eigenen Schatten abgestellt und zusammen EINEN, der um die ganze Gruppe
//! läuft; zwischen ihnen bleibt nur eine dünne Naht. Wer frei liegt oder über einem
//! anderen, behält den Schatten von Windows.
//!
//! In der Naht liegt der Statusrand (Entwurf „02a Fein“, 11.11.2026): um jedes Fenster
//! ein feiner Ring in der Farbe seines Zustands, zwei Nachbarn teilen sich die Naht je
//! zur Hälfte. Die Randfarbe von Windows taugt dafür nicht: mit dem Schatten stellt das
//! Deck den Rahmen von Windows ab, und sie bleibt unsichtbar (gemessen 11.11.2026).
//!
//! Hier steht die Rechnung: welche Fenster eine Gruppe sind ([`groups`]) und wie ihr
//! Schatten als Bild aussieht ([`render`]). Das Fenster dafür macht `deck-win`.

use crate::arrange::Area;
use crate::view::Status;

/// Breite des Statusrings um ein Agentenfenster in Pixeln, bei Skalierung `factor`
/// (1.5 = 150 %): ein Pixel bei 100 %, zwei bei 150 %. Um so viel bleibt jede Kachel an
/// jeder Seite kleiner; die Naht zwischen zwei Kacheln ist also doppelt so breit.
pub fn ring_px(factor: f64) -> i64 {
    if factor.is_nan() { 1 } else { ((1.34 * factor).round() as i64).max(1) }
}

/// Farbe des Statusrands je Zustand (`0xRRGGBB`): die Pigmente aus `docs/DESIGN.md`. Auch
/// ein ruhender Agent bekommt eine (gedämpftes Grauviolett bzw. warmes Grau): ein Fenster
/// ganz ohne Rand sähe zwischen farbigen Nachbarn aus wie vergessen.
pub fn ring_rgb(status: Status, light: bool) -> u32 {
    match (status, light) {
        (Status::Waiting, true) => 0xE8813F,
        (Status::Thinking, true) => 0x4F86C6,
        (Status::Done, true) => 0x4FA36A,
        (Status::Lost, true) => 0xC4536F,
        (Status::Waiting, false) => 0xF5A27C,
        (Status::Thinking, false) => 0x8DB9EE,
        (Status::Done, false) => 0x86D2A3,
        (Status::Lost, false) => 0xF08A9E,
        (Status::Idle | Status::None, true) => 0x9893A5,
        (Status::Idle | Status::None, false) => 0x8E8A84,
    }
}

/// Wie der Schatten aussieht, in Pixeln.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    /// So weit reicht der Schatten.
    pub radius: i64,
    /// So weit fällt er nach unten.
    pub drop: i64,
    /// Deckung direkt an der Kante (0 bis 1).
    pub strength: f64,
    /// Breite des Rings um jedes Fenster ([`ring_px`]).
    pub seam: i64,
    /// Farbe des Rings um ein Fenster, dessen Zustand das Deck nicht kennt, `0xRRGGBB`.
    pub seam_rgb: u32,
}

impl Look {
    /// Das Aussehen bei Skalierung `factor`, Naht in der Farbe `seam_rgb`.
    pub fn at(factor: f64, seam_rgb: u32) -> Look {
        let f = if factor.is_nan() || factor <= 0.0 { 1.0 } else { factor };
        Look {
            radius: (30.0 * f).round() as i64,
            drop: (9.0 * f).round() as i64,
            strength: 0.42,
            seam: ring_px(f),
            seam_rgb,
        }
    }
}

/// Liegen `a` und `b` Kante an Kante? Der Spalt darf höchstens `tol` breit sein (die
/// Naht plus Rundung), und sie müssen sich entlang der Kante wirklich gegenüberliegen.
/// Fenster, die sich überdecken, liegen nicht bündig.
fn flush(a: Area, b: Area, tol: i64) -> bool {
    let side = |a0: i64, a1: i64, b0: i64, b1: i64| a1.min(b1) - a0.max(b0);
    let across = side(a.0, a.2, b.0, b.2);
    let down = side(a.1, a.3, b.1, b.3);
    let near = |gap: i64| (-tol..=tol).contains(&gap);
    (near(-across) && down > tol) || (near(-down) && across > tol)
}

/// Die Gruppen bündig liegender Fenster: jede ist eine Liste von Stellen in `rects`,
/// aufsteigend. Ein Fenster ohne bündigen Nachbarn gehört zu keiner.
pub fn groups(rects: &[Area], tol: i64) -> Vec<Vec<usize>> {
    let mut group: Vec<Option<usize>> = vec![None; rects.len()];
    let mut out: Vec<Vec<usize>> = Vec::new();
    for start in 0..rects.len() {
        if group[start].is_some() {
            continue;
        }
        let id = out.len();
        let mut members = vec![start];
        group[start] = Some(id);
        let mut next = 0;
        while next < members.len() {
            let at = members[next];
            next += 1;
            for other in 0..rects.len() {
                if group[other].is_none() && flush(rects[at], rects[other], tol) {
                    group[other] = Some(id);
                    members.push(other);
                }
            }
        }
        if members.len() > 1 {
            members.sort_unstable();
            out.push(members);
        } else {
            group[start] = Some(usize::MAX);
        }
    }
    out
}

/// Die Fläche, die das Schattenbild der Gruppe braucht: ihre Hülle plus Schatten.
pub fn bounds(rects: &[Area], look: &Look) -> Option<Area> {
    let first = rects.first()?;
    let hull = rects.iter().fold(*first, |h, r| (h.0.min(r.0), h.1.min(r.1), h.2.max(r.2), h.3.max(r.3)));
    let m = look.radius + look.seam;
    Some((hull.0 - m, hull.1 - m, hull.2 + m, hull.3 + m + look.drop))
}

/// Abstand des Punkts von einem Rechteck (0 = darin).
fn distance(x: i64, y: i64, r: Area) -> f64 {
    let dx = (r.0 - x).max(x - (r.2 - 1)).max(0) as f64;
    let dy = (r.1 - y).max(y - (r.3 - 1)).max(0) as f64;
    (dx * dx + dy * dy).sqrt()
}

/// Das Schattenbild der Gruppe über der Fläche `area` ([`bounds`]): BGRA, Zeile für
/// Zeile von oben, Farbe mit der Deckung vormultipliziert (so will es Windows).
///
/// Unter jedem Fenster und einen Ring breit darum liegt deckend seine Farbe aus
/// `colors` (fehlt sie, die Nahtfarbe): in der Naht stoßen so die Farben der beiden
/// Nachbarn aneinander. Außen herum fällt der Schatten weich ab, nach unten versetzt.
pub fn render(rects: &[Area], colors: &[u32], area: Area, look: &Look) -> Vec<u8> {
    let (w, h) = ((area.2 - area.0).max(0) as usize, (area.3 - area.1).max(0) as usize);
    let mut px = vec![0u8; w * h * 4];
    let bgra = |rgb: u32| [(rgb & 0xFF) as u8, (rgb >> 8 & 0xFF) as u8, (rgb >> 16 & 0xFF) as u8, 255];
    let half = look.seam;
    for (row, line) in px.chunks_exact_mut(w.max(1) * 4).enumerate() {
        let y = area.1 + row as i64;
        for (i, r) in rects.iter().enumerate() {
            let seam = bgra(colors.get(i).copied().unwrap_or(look.seam_rgb));
            if y < r.1 - half || y >= r.3 + half {
                continue;
            }
            let from = (r.0 - half - area.0).clamp(0, w as i64) as usize;
            let to = (r.2 + half - area.0).clamp(0, w as i64) as usize;
            for p in line[from * 4..to * 4].chunks_exact_mut(4) {
                p.copy_from_slice(&seam);
            }
        }
        // Nur was die Fenster nicht verdecken, braucht Schatten; das ist der kleinste Teil.
        let near: Vec<Area> = rects
            .iter()
            .filter(|r| y - look.drop >= r.1 - look.radius && y - look.drop < r.3 + look.radius)
            .copied()
            .collect();
        if near.is_empty() || look.radius <= 0 {
            continue;
        }
        for (col, p) in line.chunks_exact_mut(4).enumerate() {
            if p[3] != 0 {
                continue;
            }
            let x = area.0 + col as i64;
            let d = near.iter().map(|r| distance(x, y - look.drop, *r)).fold(f64::MAX, f64::min);
            let t = 1.0 - d / look.radius as f64;
            if t > 0.0 {
                p[3] = (look.strength * t * t * 255.0).round() as u8;
            }
        }
    }
    px
}

/// Das Bild eines einzelnen Statusrings: `w` × `h` Pixel, außen ein `ring` breiter Rand
/// deckend in `rgb`, innen durchsichtig (dort liegt das Fenster). BGRA, Zeilen von oben.
/// Jedes Agentenfenster bekommt so ein Bild direkt hinter sich: der Ring bleibt bei
/// seinem Fenster, auch wenn es allein nach vorn geholt wird und zwischen ihm und den
/// anderen Agenten ein fremdes Fenster liegt.
pub fn render_ring(w: i64, h: i64, ring: i64, rgb: u32) -> Vec<u8> {
    let (w, h, ring) = (w.max(0) as usize, h.max(0) as usize, ring.max(0) as usize);
    let color = [(rgb & 0xFF) as u8, (rgb >> 8 & 0xFF) as u8, (rgb >> 16 & 0xFF) as u8, 255];
    let mut px = vec![0u8; w * h * 4];
    for (row, line) in px.chunks_exact_mut(w.max(1) * 4).enumerate() {
        let edge_row = row < ring || row + ring >= h;
        for (col, p) in line.chunks_exact_mut(4).enumerate() {
            if edge_row || col < ring || col + ring >= w {
                p.copy_from_slice(&color);
            }
        }
    }
    px
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_ring_is_a_solid_band_around_an_empty_middle() {
        let px = render_ring(10, 8, 2, 0x112233);
        let at = |x: usize, y: usize| [px[(y * 10 + x) * 4], px[(y * 10 + x) * 4 + 1], px[(y * 10 + x) * 4 + 2], px[(y * 10 + x) * 4 + 3]];
        assert_eq!(px.len(), 10 * 8 * 4);
        for (x, y) in [(0, 0), (1, 4), (8, 4), (5, 1), (5, 6), (9, 7)] {
            assert_eq!(at(x, y), [0x33, 0x22, 0x11, 255], "Rand bei {x},{y}");
        }
        for (x, y) in [(2, 2), (7, 5), (5, 4)] {
            assert_eq!(at(x, y), [0, 0, 0, 0], "Mitte bei {x},{y}");
        }
    }

    fn look() -> Look {
        Look { radius: 10, drop: 2, strength: 0.5, seam: 1, seam_rgb: 0x112233 }
    }

    fn at(px: &[u8], area: Area, x: i64, y: i64) -> [u8; 4] {
        let w = (area.2 - area.0) as usize;
        let i = ((y - area.1) as usize * w + (x - area.0) as usize) * 4;
        [px[i], px[i + 1], px[i + 2], px[i + 3]]
    }

    #[test]
    fn windows_lying_edge_to_edge_form_one_group_and_a_lone_one_none() {
        // Raster 2 × 2 mit einer Naht von 1 px, dazu eines frei daneben.
        let rects = [(0, 0, 99, 99), (100, 0, 199, 99), (0, 100, 99, 199), (400, 0, 500, 100)];
        assert_eq!(groups(&rects, 3), [vec![0, 1, 2]]);
        assert!(groups(&rects[3..], 3).is_empty());
    }

    #[test]
    fn overlapping_windows_and_windows_meeting_only_at_a_corner_are_not_flush() {
        let stacked = [(0, 0, 100, 100), (40, 40, 140, 140)];
        assert!(groups(&stacked, 3).is_empty(), "übereinander: jedes behält seinen Schatten");
        let corner = [(0, 0, 100, 100), (100, 100, 200, 200)];
        assert!(groups(&corner, 3).is_empty());
    }

    #[test]
    fn two_blocks_far_apart_are_two_groups() {
        let rects = [(0, 0, 100, 100), (100, 0, 200, 100), (1000, 0, 1100, 100), (1100, 0, 1200, 100)];
        assert_eq!(groups(&rects, 3), [vec![0, 1], vec![2, 3]]);
    }

    #[test]
    fn the_ring_follows_the_scaling_and_never_vanishes() {
        assert_eq!(ring_px(1.0), 1);
        assert_eq!(ring_px(1.5), 2);
        assert_eq!(ring_px(2.0), 3);
        assert_eq!(ring_px(0.2), 1);
        assert_eq!(Look::at(1.5, 0).radius, 45);
    }

    #[test]
    fn each_half_of_the_seam_takes_the_status_color_of_its_window() {
        // Zwei Kacheln mit 2 px Naht, Ring 1 px: jede Hälfte trägt die Farbe ihres Fensters.
        let rects = [(0, 0, 99, 100), (101, 0, 200, 100)];
        let area = bounds(&rects, &look()).unwrap();
        let waiting = ring_rgb(Status::Waiting, true);
        let px = render(&rects, &[waiting], area, &look());
        assert_eq!(at(&px, area, 99, 50), [0x3F, 0x81, 0xE8, 255], "linke Hälfte: Rückfrage");
        assert_eq!(at(&px, area, 100, 50), [0x33, 0x22, 0x11, 255], "rechte Hälfte: ohne Farbe die Naht");
        assert_eq!(ring_rgb(Status::Idle, true), 0x9893A5, "auch ein ruhender Agent hat seinen Ring");
        assert_ne!(ring_rgb(Status::Done, true), ring_rgb(Status::Done, false), "dunkel eigene Pigmente");
    }

    #[test]
    fn the_gap_between_the_windows_is_filled_with_the_seam_color() {
        let rects = [(0, 0, 99, 100), (100, 0, 200, 100)];
        let area = bounds(&rects, &look()).unwrap();
        assert_eq!(area, (-11, -11, 211, 113));
        let px = render(&rects, &[], area, &look());
        assert_eq!(px.len(), 222 * 124 * 4);
        assert_eq!(at(&px, area, 99, 50), [0x33, 0x22, 0x11, 255], "im Spalt: Nahtfarbe als BGRA");
        assert_eq!(at(&px, area, -1, 50), [0x33, 0x22, 0x11, 255], "Saum um die Gruppe");
    }

    #[test]
    fn the_shadow_fades_outwards_and_falls_downwards() {
        let rects = [(0, 0, 100, 100)];
        let area = bounds(&rects, &look()).unwrap();
        let px = render(&rects, &[], area, &look());
        let below = at(&px, area, 50, 104)[3];
        let far_below = at(&px, area, 50, 109)[3];
        let above = at(&px, area, 50, -5)[3];
        assert!(below > far_below && far_below > 0, "{below} {far_below}");
        assert!(below > above, "unten kräftiger als oben: {below} {above}");
        assert_eq!(at(&px, area, -11, -11), [0, 0, 0, 0], "ganz außen nichts");
        assert_eq!(at(&px, area, 50, 104)[..3], [0, 0, 0], "Schatten ist schwarz");
    }
}
