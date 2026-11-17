//! Sonnenaufgang und Sonnenuntergang für die Einstellung „Hell/Dunkel: Sonnenstand“.
//!
//! Die Rechnung ist die NOAA-Näherung aus Jorrits früherem Skript (geplante Aufgabe
//! „DarkModeZeitplan“, `%LOCALAPPDATA%\DarkModeZeitplan\set-theme.ps1`), Zeile für Zeile
//! übernommen: Zeitgleichung und Deklination als Fourier-Reihe über den Tag des Jahres,
//! Stundenwinkel bei 90,833° (Sonnenrand plus Brechung am Horizont). Genau auf ein bis
//! zwei Minuten, das reicht für hell und dunkel.
//!
//! Gerechnet wird in UTC: der Tag des Jahres ist der des UTC-Datums, Auf- und Untergang
//! sind Minuten nach Mitternacht UTC. Für Orte in Mitteleuropa liegen beide sicher im
//! selben UTC-Tag.

use std::f64::consts::{PI, TAU};

use crate::isotime::{civil_from_days, days_from_civil};

/// Ein Ort aus der eingebauten Liste. `id` steht in `deck_settings.json` (`sun_place`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Place {
    pub id: &'static str,
    pub de: &'static str,
    pub en: &'static str,
    pub lat: f64,
    pub lon: f64,
}

const fn p(id: &'static str, de: &'static str, en: &'static str, lat: f64, lon: f64) -> Place {
    Place { id, de, en, lat, lon }
}

/// Größere Städte in Deutschland, dazu Wien und Zürich. Auf eine Stadt genau reicht: von
/// Hamburg nach Lübeck verschiebt sich der Sonnenuntergang um gut eine Minute.
pub const PLACES: &[Place] = &[
    p("augsburg", "Augsburg", "Augsburg", 48.37, 10.90),
    p("berlin", "Berlin", "Berlin", 52.52, 13.40),
    p("bremen", "Bremen", "Bremen", 53.08, 8.80),
    p("dortmund", "Dortmund", "Dortmund", 51.51, 7.47),
    p("dresden", "Dresden", "Dresden", 51.05, 13.74),
    p("duesseldorf", "Düsseldorf", "Düsseldorf", 51.23, 6.78),
    p("erfurt", "Erfurt", "Erfurt", 50.98, 11.03),
    p("essen", "Essen", "Essen", 51.46, 7.01),
    p("frankfurt", "Frankfurt am Main", "Frankfurt", 50.11, 8.68),
    p("freiburg", "Freiburg", "Freiburg", 47.99, 7.85),
    p("hamburg", "Hamburg", "Hamburg", 53.55, 10.00),
    p("hannover", "Hannover", "Hanover", 52.37, 9.73),
    p("karlsruhe", "Karlsruhe", "Karlsruhe", 49.01, 8.40),
    p("kiel", "Kiel", "Kiel", 54.32, 10.13),
    p("koeln", "Köln", "Cologne", 50.94, 6.96),
    p("leipzig", "Leipzig", "Leipzig", 51.34, 12.37),
    p("magdeburg", "Magdeburg", "Magdeburg", 52.13, 11.63),
    p("muenchen", "München", "Munich", 48.14, 11.58),
    p("muenster", "Münster", "Münster", 51.96, 7.63),
    p("nuernberg", "Nürnberg", "Nuremberg", 49.45, 11.08),
    p("rostock", "Rostock", "Rostock", 54.09, 12.10),
    p("saarbruecken", "Saarbrücken", "Saarbrücken", 49.24, 6.99),
    p("stuttgart", "Stuttgart", "Stuttgart", 48.78, 9.18),
    p("wien", "Wien", "Vienna", 48.21, 16.37),
    p("zuerich", "Zürich", "Zurich", 47.37, 8.54),
];

/// Vorgabe: Hamburg (53,55 / 10,00), wie im früheren Skript.
pub const DEFAULT_PLACE: &str = "hamburg";

/// Ort zur Kennung; unbekannt oder leer → `None`.
pub fn place(id: &str) -> Option<&'static Place> {
    let id = id.trim();
    PLACES.iter().find(|p| p.id.eq_ignore_ascii_case(id))
}

/// Ort zur Kennung, sonst die Vorgabe.
pub fn place_or_default(id: &str) -> &'static Place {
    place(id).or_else(|| place(DEFAULT_PLACE)).unwrap_or(&PLACES[0])
}

/// Wie die Sonne an einem Tag steht.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SunDay {
    /// Aufgang und Untergang in Minuten nach Mitternacht UTC.
    Normal { rise: f64, set: f64 },
    /// Die Sonne geht nicht unter (Mitternachtssonne).
    AlwaysUp,
    /// Die Sonne geht nicht auf (Polarnacht).
    AlwaysDown,
}

/// Auf- und Untergang am Tag `day_of_year` (1 = 1. Januar) für Breite `lat` und Länge
/// `lon` in Grad (Osten positiv).
pub fn sun_day(day_of_year: u32, lat: f64, lon: f64) -> SunDay {
    let g = TAU / 365.0 * (f64::from(day_of_year) - 1.0);
    let eq_time = 229.18
        * (0.000075 + 0.001868 * g.cos() - 0.032077 * g.sin() - 0.014615 * (2.0 * g).cos() - 0.040849 * (2.0 * g).sin());
    let decl = 0.006918 - 0.399912 * g.cos() + 0.070257 * g.sin() - 0.006758 * (2.0 * g).cos() + 0.000907 * (2.0 * g).sin()
        - 0.002697 * (3.0 * g).cos()
        + 0.00148 * (3.0 * g).sin();
    let rad = PI / 180.0;
    let lat = lat * rad;
    let cos_ha = (90.833 * rad).cos() / (lat.cos() * decl.cos()) - lat.tan() * decl.tan();
    if cos_ha >= 1.0 {
        return SunDay::AlwaysDown;
    }
    if cos_ha <= -1.0 {
        return SunDay::AlwaysUp;
    }
    let ha = cos_ha.acos() / rad;
    SunDay::Normal { rise: 720.0 - 4.0 * (lon + ha) - eq_time, set: 720.0 - 4.0 * (lon - ha) - eq_time }
}

/// Tag des Jahres (1 …) für Tage seit 1970-01-01.
fn day_of_year(unix_days: i64) -> u32 {
    let (y, _, _) = civil_from_days(unix_days);
    (unix_days - days_from_civil(y, 1, 1) + 1) as u32
}

/// Ist es zum Zeitpunkt `unix_s` am Ort hell (zwischen Auf- und Untergang)?
pub fn is_light(unix_s: f64, place: &Place) -> bool {
    let day = (unix_s / 86_400.0).floor() as i64;
    let minute = (unix_s - day as f64 * 86_400.0) / 60.0;
    match sun_day(day_of_year(day), place.lat, place.lon) {
        SunDay::Normal { rise, set } => minute >= rise && minute < set,
        SunDay::AlwaysUp => true,
        SunDay::AlwaysDown => false,
    }
}

/// Nächster Wechsel nach `unix_s` (Unix-Sekunden), gesucht über heute und die beiden
/// nächsten Tage; ohne Wechsel darin (Polartag, Polarnacht) `None`.
pub fn next_change(unix_s: f64, place: &Place) -> Option<f64> {
    let today = (unix_s / 86_400.0).floor() as i64;
    (today..today + 3).find_map(|day| {
        let SunDay::Normal { rise, set } = sun_day(day_of_year(day), place.lat, place.lon) else { return None };
        let start = day as f64 * 86_400.0;
        [rise, set].into_iter().map(|m| start + m * 60.0).find(|t| *t > unix_s)
    })
}

#[cfg(test)]
#[path = "sun_tests.rs"]
mod tests;
