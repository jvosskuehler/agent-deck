//! Reihenfolge und Umsortieren: die Rechnung hinter dem Ziehen.
//!
//! VS Code gibt seine visuelle Reihenfolge nicht preis (weder Terminals noch Fenster),
//! also führt das Deck seine eigene:
//!
//! * `apply_order` legt die gemerkte Wahl über die gemeldete Liste, bei den Kacheln
//!   einer Reihe wie bei den Repo-Blöcken untereinander.
//! * `drop_index` / `reflow_tops` führen einen laufenden Zug: wohin gehört das gezogene
//!   Element, und wo müssen die anderen hin, damit es Platz findet. Sie rechnen mit
//!   VARIABLEN Höhen, weil Repo-Blöcke unterschiedlich hoch sind.
//! * `move_to` schreibt das Ergebnis fest, `pin_first` hält den Heimat-Block oben.

use std::collections::{BTreeMap, HashSet};

/// Die gemerkte Reihenfolge `saved` über die gemeldete Liste `live` legen.
///
/// Was in beiden steht, kommt in der Reihenfolge von `saved`; was nur `live` meldet,
/// hängt hinten in Melde-Reihenfolge an; was nur `saved` kennt, fliegt raus. `live` ist
/// die Wahrheit darüber, WAS es gibt, `saved` darüber, in welcher Reihenfolge. Doppelte
/// Einträge in `saved` zählen einmal (Selbstheilung gegen eine verhunzte Datei).
pub fn apply_order<S: AsRef<str>, T: AsRef<str>>(saved: &[S], live: &[T]) -> Vec<String> {
    let live_set: HashSet<&str> = live.iter().map(|x| x.as_ref()).collect();
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for x in saved {
        let x = x.as_ref();
        if live_set.contains(x) && seen.insert(x) {
            out.push(x.to_string());
        }
    }
    out.extend(live.iter().map(|x| x.as_ref()).filter(|x| !seen.contains(x)).map(String::from));
    out
}

/// Die neue Reihenfolge der SICHTBAREN Elemente in die gemerkte Liste einweben.
///
/// Gemerkt sind mehr Elemente, als zu sehen sind (ein geschlossenes Repo steht weiter in
/// der Datei). Die Plätze, die sichtbare Elemente in `saved` belegen, werden von vorn
/// mit `visible` neu befüllt; alle anderen bleiben, wo sie sind. Sichtbare, die noch
/// nicht in `saved` stehen, hängen vorher hinten an.
pub fn merge_visible<S: AsRef<str>, T: AsRef<str>>(saved: &[S], visible: &[T]) -> Vec<String> {
    let mut out: Vec<String> = saved.iter().map(|s| s.as_ref().to_string()).collect();
    for v in visible {
        if !out.iter().any(|x| x == v.as_ref()) {
            out.push(v.as_ref().to_string());
        }
    }
    let vis: HashSet<&str> = visible.iter().map(|v| v.as_ref()).collect();
    let mut fresh = visible.iter().map(|v| v.as_ref().to_string());
    out.into_iter()
        .map(|x| if vis.contains(x.as_str()) { fresh.next().unwrap_or(x) } else { x })
        .collect()
}

/// Die Oberkanten, die das gezogene Element an Position 0, 1, … hätte, gerechnet gegen
/// den zusammengerückten Stapel (so, als wäre es nicht dabei). Diese Kanten liegen fest,
/// während die anderen noch gleiten; darum zittert beim Ziehen nichts.
pub fn slot_tops(heights: &[f64], gap: f64, top: f64, from_index: usize) -> Vec<f64> {
    let mut out = Vec::with_capacity(heights.len());
    let mut y = top;
    for (i, h) in heights.iter().enumerate() {
        if i == from_index {
            continue;
        }
        out.push(y);
        y += h + gap;
    }
    out.push(y);
    out
}

/// Zielposition des gezogenen Elements: die Position, deren Oberkante seiner aktuellen
/// am nächsten liegt. `drag_top` ist die Oberkante des ELEMENTS, nicht der Zeiger.
///
/// Kante statt Mitte: bei variablen Höhen müsste ein hoher Block mit seiner Mitte über
/// die des ersten (flachen) Elements, also mit der Oberkante weit über den Rand;
/// Position 0 wäre für ihn unerreichbar.
pub fn drop_index(heights: &[f64], gap: f64, top: f64, from_index: usize, drag_top: f64) -> usize {
    let tops = slot_tops(heights, gap, top, from_index);
    let mut best = 0;
    for (p, t) in tops.iter().enumerate() {
        if (t - drag_top).abs() < (tops[best] - drag_top).abs() {
            best = p;
        }
    }
    best
}

/// Ziel-Oberkanten der NICHT gezogenen Elemente: `{index: y}`. Sie rücken zusammen und
/// lassen an `target` genau eine Lücke frei, so hoch wie das gezogene Element. Mit
/// `target == from_index` ergibt das exakt den Ausgangsstapel.
pub fn reflow_tops(heights: &[f64], gap: f64, top: f64, from_index: usize, target: usize) -> BTreeMap<usize, f64> {
    let mut want = BTreeMap::new();
    let mut y = top;
    let mut pos = 0;
    for (i, h) in heights.iter().enumerate() {
        if i == from_index {
            continue;
        }
        if pos == target {
            y += heights[from_index] + gap;
            pos += 1;
        }
        want.insert(i, y);
        y += h + gap;
        pos += 1;
    }
    want
}

/// `item` herausnehmen und an Position `target` wieder einsetzen. Der Zielindex wird
/// geklemmt: er kommt aus einer Zeigerposition, und die kennt keine Listengrenzen.
pub fn move_to<S: AsRef<str>>(order: &[S], item: &str, target: i64) -> Vec<String> {
    let mut rest: Vec<String> =
        order.iter().map(|x| x.as_ref()).filter(|x| *x != item).map(String::from).collect();
    let t = target.clamp(0, rest.len() as i64) as usize;
    rest.insert(t, item.to_string());
    rest
}

/// `item` an die Spitze, der Rest in seiner Reihenfolge. Für den Heimat-Block: er steht
/// immer oben. Fehlt `item`, bleibt alles, wie es ist.
pub fn pin_first<S: AsRef<str>>(order: &[S], item: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = order.iter().map(|s| s.as_ref().to_string()).collect();
    if let Some(item) = item
        && let Some(i) = out.iter().position(|x| x == item)
    {
        let x = out.remove(i);
        out.insert(0, x);
    }
    out
}
