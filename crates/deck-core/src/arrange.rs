//! Kachelraster der Windows-Terminal-Agenten: wie viele nebeneinander passen, wohin
//! jede Kachel kommt, und in welchen Reihen die Fenster gerade liegen. Der Win32-Teil
//! (aufzählen, verschieben) steht in `deck-win`.

/// Höchstens so viele Agentenfenster nebeneinander; danach beginnt eine zweite Reihe.
pub const MAX_COLS: i64 = 4;
/// Schmaler wird keine Kachel (Pixel bei 100 %): rund 60 Zeichen Cascadia Mono 12 pt.
/// Schmaler bricht Claude Code fast jede Zeile um.
pub const MIN_TILE_W: i64 = 600;

/// Rechteck `(x, y, w, h)`.
pub type Rect = (i64, i64, i64, i64);
/// Fläche `(links, oben, rechts, unten)`.
pub type Area = (i64, i64, i64, i64);

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

/// Reihenfolge, in der „alle holen“ die Fenster ins Raster legt. Liegen sie sauber
/// nebeneinander, so wie sie liegen ([`screen_rows`]): wer sie von Hand sortiert hat,
/// findet jedes am gewohnten Platz wieder. Überdecken sich zwei davon zu mehr als einem
/// Viertel des kleineren, ist die Lage keine Ordnung mehr, sondern ein Haufen (Monitor
/// weg, etwa beim Umschalten am KVM: Windows schiebt alle auf den verbliebenen
/// Bildschirm). Dann nach Slot-Nummer (B5, B14, B20), denn die Lage gibt nichts her.
pub fn grid_order(wins: &[(String, Rect)]) -> Vec<String> {
    let piled = wins.iter().enumerate().any(|(i, (_, a))| wins[i + 1..].iter().any(|(_, b)| overlaps_much(*a, *b)));
    if !piled {
        return screen_rows(wins).into_iter().flatten().collect();
    }
    let mut names: Vec<&String> = wins.iter().map(|(n, _)| n).collect();
    names.sort_by_key(|n| (crate::wt::split_slot(n), (*n).clone()));
    names.into_iter().cloned().collect()
}

/// Überdecken sich `a` und `b` zu mehr als einem Viertel des kleineren? Ein paar Pixel
/// Überlappung (Snap, Fensterschatten) zählen nicht.
fn overlaps_much(a: Rect, b: Rect) -> bool {
    let w = (a.0 + a.2).min(b.0 + b.2) - a.0.max(b.0);
    let h = (a.1 + a.3).min(b.1 + b.3) - a.1.max(b.1);
    let smaller = (a.2 * a.3).min(b.2 * b.3).max(1);
    w > 0 && h > 0 && w * h * 4 > smaller
}

/// Die Fläche, die [`tile_grid`] bei `n` Kacheln am Ende der letzten Reihe frei lässt
/// (5 Fenster in 2 × 3: die Kachel rechts unten), als ein Rechteck über alle freien
/// Kacheln. `None`, wenn das Raster aufgeht. Dort steht das Info-Fenster des Decks.
pub fn grid_gap(n: i64, area: Area, max_cols: i64) -> Option<Rect> {
    if n <= 0 {
        return None;
    }
    let rows = -((-n).div_euclid(max_cols.max(1)));
    let cols = -((-n).div_euclid(rows));
    let full = rows * cols;
    if full == n {
        return None;
    }
    // Volles Raster mit derselben Teilung: Reihen und Spalten bleiben gleich, weil
    // (rows - 1) · max_cols < n ≤ full ≤ rows · max_cols.
    let cells = tile_grid(full, area, max_cols);
    let first = cells[n as usize];
    let last = cells[(full - 1) as usize];
    Some((first.0, first.1, last.0 + last.2 - first.0, first.3))
}

/// Reihenfolge fürs nächste Kacheln: wer schon liegt, behält seinen Platz (`lying`, so
/// wie die Fenster auf dem Bildschirm liegen), wer dazukommt, kommt hinten dran (`live`
/// in seiner Reihenfolge). Nur lebende, jeder genau einmal, ohne `gone`.
///
/// Früher hieß das `lying + live` mit `Vec::dedup`, das nur benachbarte Doppelte
/// entfernt: aus A1 A2 A3 + A1 A2 A3 ohne A2 wurde A1 A3 A1 A3, gekachelt als 2 × 2,
/// und die übrigen Fenster landeten nach dem Schließen in der unteren Hälfte (gemessen
/// 24.11.2026 mit `tools/probe`, Ablauf `close`; erst ein zweiter Vorhang richtete es).
pub fn keep_order(lying: &[String], live: &[String], gone: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in lying.iter().chain(live) {
        if live.contains(s) && Some(s.as_str()) != gone && !out.contains(s) {
            out.push(s.clone());
        }
    }
    out
}

/// Ein Monitor, wie das Kacheln ihn sieht: Gerätename, Arbeitsfläche, Skalierung.
pub type Screen = (String, Area, f64);

/// Soll das Deck nach einer Änderung der Anzeige die Blöcke nachkacheln? Nur wenn es
/// dieselben Monitore sind und einer eine andere Arbeitsfläche oder Skalierung hat
/// (Auflösung, Skalierung, Taskleiste umgestellt). Sonst blieben die Fenster an ihren
/// alten Pixeln stehen, mit Lücken oder übereinander (gemessen 24.11.2026 in der Test-VM).
/// Kommt ein Monitor dazu oder fällt einer weg (KVM, Dock), bleibt alles liegen: dann
/// entscheidet der Mensch mit „alle holen“, wohin der Block soll.
pub fn resized_in_place(before: &[Screen], after: &[Screen]) -> bool {
    let names = |s: &[Screen]| {
        let mut n: Vec<String> = s.iter().map(|(d, _, _)| d.clone()).collect();
        n.sort();
        n
    };
    if before.is_empty() || names(before) != names(after) {
        return false;
    }
    after.iter().any(|(d, area, scale)| {
        before.iter().any(|(bd, ba, bs)| bd == d && (ba != area || (bs - scale).abs() > 0.001))
    })
}
