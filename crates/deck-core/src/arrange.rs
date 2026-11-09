//! Fenster anordnen: welche gehören dazu, und wohin genau.
//!
//! Die anzeigefreien Regeln hinter „VS-Code-Fenster einsammeln“ und hinter dem
//! Kachelraster der Windows-Terminal-Agenten. Der Win32-Teil (aufzählen, verschieben,
//! maximieren) steht in `deck-win`.
//!
//! **Erkannt wird am PROZESS, nicht am Titel.** Ein Browser-Tab „VS-Code-Doku - Visual
//! Studio Code - Firefox“ würde sonst mitgenommen, ein Fenster mit eigenem
//! `window.title` übersehen. Der Titel ist nur der Notnagel, wenn Windows den Prozess
//! nicht herausgibt.

use crate::config::VSCODE_MARKER;
use crate::pystr::strip;

/// Prozessnamen, die als VS-Code-Fenster gelten (klein verglichen). Absichtlich knapp:
/// Stable, Insiders, VSCodium. Wer hier zu breit filtert, verschiebt fremde Fenster.
pub const EDITOR_EXES: [&str; 3] = ["code.exe", "code - insiders.exe", "codium.exe"];

/// Fensterstil-Bit „hat einen Maximieren-Knopf“.
pub const WS_MAXIMIZEBOX: u32 = 0x0001_0000;

/// Untergrenze des Wiederherstellungs-Rechtecks: eine kaputte Normalgröße (0x0) läge
/// sonst nach dem Entmaximieren als unauffindbarer Punkt auf dem Monitor.
pub const MIN_W: i64 = 640;
pub const MIN_H: i64 = 420;

/// Höchstens so viele Agentenfenster nebeneinander; danach beginnt eine zweite Reihe.
pub const MAX_COLS: i64 = 4;
/// Schmaler wird keine Kachel (Pixel bei 100 %): rund 60 Zeichen Cascadia Mono 12 pt.
/// Schmaler bricht Claude Code fast jede Zeile um.
pub const MIN_TILE_W: i64 = 600;

/// Rechteck `(x, y, w, h)`.
pub type Rect = (i64, i64, i64, i64);
/// Fläche `(links, oben, rechts, unten)`.
pub type Area = (i64, i64, i64, i64);

/// Gehört das Fenster zu VS Code? `exe` ist der volle Pfad (leer, wenn Windows ihn nicht
/// hergibt: dann entscheidet der Titel). Ist der Prozess bekannt, entscheidet nur er.
pub fn is_editor_window(exe: &str, title: &str) -> bool {
    let base = exe.rsplit(['/', '\\']).next().unwrap_or("");
    let base = strip(base).to_lowercase();
    if !base.is_empty() {
        return EDITOR_EXES.contains(&base.as_str());
    }
    !title.is_empty() && title.to_lowercase().contains(&VSCODE_MARKER.to_lowercase())
}

/// Darf dieses Fenster maximiert werden? Dialoge haben das Bit nicht; maximiert wären
/// sie ein Bildschirm voll grauer Fläche.
pub fn is_maximizable(style: u32) -> bool {
    style & WS_MAXIMIZEBOX != 0
}

/// Wiederherstellungs-Rechteck für ein Fenster `w × h`, zentriert auf der Arbeitsfläche
/// und nie größer als sie (eine vom großen Monitor gemerkte Größe stünde sonst über).
pub fn normal_rect(w: i64, h: i64, area: Area) -> Rect {
    let (left, top, right, bottom) = area;
    let aw = (right - left).max(1);
    let ah = (bottom - top).max(1);
    let w = w.max(MIN_W).min(aw);
    let h = h.max(MIN_H).min(ah);
    (left + (aw - w).div_euclid(2), top + (ah - h).div_euclid(2), w, h)
}

/// Wie viele Kacheln auf `width` Pixel nebeneinander passen, bei Skalierung `factor`
/// (1.5 = 150 %): jede mindestens `MIN_TILE_W` breit, höchstens `MAX_COLS`, mindestens 1.
pub fn max_cols_for(width: i64, factor: f64) -> i64 {
    let factor = if factor == 0.0 || factor.is_nan() { 1.0 } else { factor };
    let logical = (width as f64 / factor.max(0.1)).trunc() as i64;
    MAX_COLS.min(logical.div_euclid(MIN_TILE_W)).max(1)
}

/// `n` gleich große Kacheln auf der Fläche, zeilenweise von links oben. Erst so wenige
/// Reihen wie möglich, dann die Spalten gleichmäßig verteilt (5 Fenster = 2 × 3, nicht
/// 4 + 1); ein freier Platz bleibt leer. Die Kanten werden über die ganze Fläche
/// gerundet, damit keine Pixelreihe übrig bleibt.
pub fn tile_grid(n: i64, area: Area, max_cols: i64) -> Vec<Rect> {
    if n <= 0 {
        return Vec::new();
    }
    let (left, top, right, bottom) = area;
    let rows = -((-n).div_euclid(max_cols.max(1)));
    let cols = -((-n).div_euclid(rows));
    let xs: Vec<i64> = (0..=cols).map(|c| left + ((right - left) * c).div_euclid(cols)).collect();
    let ys: Vec<i64> = (0..=rows).map(|r| top + ((bottom - top) * r).div_euclid(rows)).collect();
    (0..n)
        .map(|i| {
            let (r, c) = ((i / cols) as usize, (i % cols) as usize);
            (xs[c], ys[r], xs[c + 1] - xs[c], ys[r + 1] - ys[r])
        })
        .collect()
}

/// Wie die Fenster auf dem Bildschirm liegen, als Reihen von oben nach unten, jede
/// Reihe von links nach rechts. `wins` sind `(Name, Rechteck)`; geliefert werden die
/// Namen. Damit legt das Deck seine Karten so wie die Fenster: rechts oben auf dem
/// Bildschirm = rechts oben im Block, 3 + 3 Fenster = 3 + 3 Karten.
///
/// Eine Reihe beginnt beim obersten noch freien Fenster; dazu gehört jedes Fenster,
/// dessen senkrechte Mitte in dessen Höhe liegt. So bleiben leicht versetzte Fenster
/// (von Hand gezogen, Snap mit Rand) in derselben Reihe, und ein Fenster, das unter
/// einem anderen beginnt, macht eine neue auf.
pub fn screen_rows(wins: &[(String, Rect)]) -> Vec<Vec<String>> {
    let mut rest: Vec<&(String, Rect)> = wins.iter().collect();
    rest.sort_by_key(|(_, (x, y, _, _))| (*y, *x));
    let mut rows = Vec::new();
    while let Some(&&(_, (_, top, _, h))) = rest.first() {
        let bottom = top + h.max(1);
        let (mut row, others): (Vec<_>, Vec<_>) = rest.into_iter().partition(|(_, (_, y, _, wh))| {
            let mid = y + (*wh).max(1).div_euclid(2);
            (top..bottom).contains(&mid)
        });
        if row.is_empty() {
            // Kann nur ein entartetes Rechteck sein: das oberste allein nehmen, sonst
            // liefe die Schleife ewig.
            row = others[..1].to_vec();
            rest = others[1..].to_vec();
        } else {
            rest = others;
        }
        row.sort_by_key(|(_, (x, y, _, _))| (*x, *y));
        rows.push(row.into_iter().map(|(n, _)| n.clone()).collect());
    }
    rows
}
