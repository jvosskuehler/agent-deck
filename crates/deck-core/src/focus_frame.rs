//! Die Rechnungen hinter dem Fokus-Rahmen.
//!
//! Der Rahmen besteht aus vier Teilfenstern rund um das Agentenfenster: oben und unten
//! je ein Streifen über die ganze Breite samt Ecken, links und rechts je einer
//! dazwischen. So wird je Bild nur der schmale Streifen gezeichnet, in dem wirklich etwas
//! leuchtet. Jeder Streifen reicht `outer` Pixel nach außen (Leuchthof) und `inner`
//! Pixel nach innen (die Linie selbst und ein Rest Hof).

/// Was beim Fokuswechsel zur Wahl steht (Tafeln 03–06 des Canvas „Fokus-Rahmen
/// Varianten“, 2026-11-06).
pub const DESIGNS: [&str; 4] = ["kometen", "glut", "hochspannung", "lauflicht"];

/// Rechteck `(x, y, breite, höhe)`.
pub type Rect = (i32, i32, i32, i32);
/// Fläche bzw. Ausschnitt `(links, oben, rechts, unten)`.
pub type Ltrb = (i32, i32, i32, i32);

/// Zufälliges Design für den nächsten Fokus, nie zweimal dasselbe hintereinander (sonst
/// wirkt der Wechsel wie keiner). `choose(n)` liefert einen Index in `0..n`; der Aufrufer
/// bringt seinen Zufall mit, Tests einen festen.
pub fn pick<'a>(designs: &[&'a str], last: Option<&str>, choose: &mut dyn FnMut(usize) -> usize) -> &'a str {
    let mut pool: Vec<&'a str> = designs.iter().copied().filter(|d| Some(*d) != last).collect();
    if pool.is_empty() {
        pool = designs.to_vec();
    }
    pool[choose(pool.len()) % pool.len()]
}

/// Ein Teilfenster des Rahmens mit seiner Seite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Piece {
    pub side: &'static str,
    pub rect: Rect,
}

/// Die vier Teilfenster um `rect` = (links, oben, rechts, unten), ohne Überlappung.
/// Ist das Fenster zu niedrig für Seitenstreifen, gibt es nur oben und unten.
pub fn pieces(rect: Ltrb, outer: i32, inner: i32) -> Vec<Piece> {
    let (left, top, right, bottom) = rect;
    let (w, h) = (right - left, bottom - top);
    let band = outer + inner;
    let mut out = vec![
        Piece { side: "top", rect: (left - outer, top - outer, w + 2 * outer, band) },
        Piece { side: "bottom", rect: (left - outer, bottom - inner, w + 2 * outer, band) },
    ];
    if h - 2 * inner > 0 {
        out.push(Piece { side: "left", rect: (left - outer, top + inner, band, h - 2 * inner) });
        out.push(Piece { side: "right", rect: (right - inner, top + inner, band, h - 2 * inner) });
    }
    out
}

/// Teilfenster auf die Monitorfläche `area` stutzen, damit der Hof nicht auf den
/// Nachbarmonitor fällt: `(Fensterrechteck, Ausschnitt im Bild)`, oder `None`, wenn
/// nichts sichtbar bleibt.
pub fn clip(piece: Rect, area: Option<Ltrb>) -> Option<(Rect, Ltrb)> {
    let (x, y, w, h) = piece;
    let Some(area) = area else {
        return Some((piece, (0, 0, w, h)));
    };
    let (x0, y0) = (x.max(area.0), y.max(area.1));
    let (x1, y1) = ((x + w).min(area.2), (y + h).min(area.3));
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(((x0, y0, x1 - x0, y1 - y0), (x0 - x, y0 - y, x1 - x, y1 - y)))
}

/// Wie tief der Pixel (x, y) im Fenster w × h liegt: 0 ist die äußerste Pixelreihe,
/// positiv weiter innen, negativ außerhalb (an den Ecken der Abstand zur Ecke, damit
/// der Hof dort rund wird).
pub fn depth(x: i32, y: i32, w: i32, h: i32) -> f64 {
    if (0..w).contains(&x) && (0..h).contains(&y) {
        return f64::from(x.min(y).min(w - 1 - x).min(h - 1 - y));
    }
    let dx = (-x).max(0).max(x - (w - 1));
    let dy = (-y).max(0).max(y - (h - 1));
    -f64::from(dx).hypot(f64::from(dy))
}

/// Deckung (Linie, Hof) eines Pixels in Tiefe `u`, je 0..1.
///
/// `core` = (von, bis) ist die Linie in Tiefen-Pixeln, mit `soft` statt harter Kanten
/// ein runder Buckel. Der Hof fällt nach außen mit allen `glows` (Breite, Stärke) ab,
/// nach innen nur mit dem ersten, engsten: ein breiter Hof über dem Terminaltext würde
/// ihn verschleiern.
pub fn profile(u: f64, core: (f64, f64), glows: &[(f64, f64)], soft: bool) -> (f64, f64) {
    let (lo, hi) = core;
    let mid = u + 0.5;
    let cov = if soft {
        let half = ((hi - lo) / 2.0).max(0.5);
        (-1.2 * ((mid - (lo + hi) / 2.0) / half).powi(2)).exp()
    } else {
        ((u + 1.0).min(hi) - u.max(lo)).max(0.0)
    };
    let glow = if mid < lo {
        let d = lo - mid;
        glows.iter().map(|(g, s)| s * (-(d / g).powi(2)).exp()).sum()
    } else if mid > hi {
        let (g, s) = glows[0];
        s * (-((mid - hi) / g).powi(2)).exp()
    } else {
        glows[0].1
    };
    (cov.min(1.0), glow.min(1.0))
}
