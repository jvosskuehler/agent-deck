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
//!
//! Den Ort liefert `sun_zone.rs` grob aus der Zeitzone von Windows; eine Erweiterung kann
//! einen genaueren vorgeben (`extension.rs` der App).

use std::f64::consts::{PI, TAU};

use crate::isotime::{civil_from_days, days_from_civil};

/// Ein Ort für die Rechnung: Name zum Anzeigen und Koordinaten in Grad (Osten positiv).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Place {
    pub id: &'static str,
    pub de: &'static str,
    pub en: &'static str,
    pub lat: f64,
    pub lon: f64,
}

/// Einen Ort bauen (für Tabellen mit `const`).
pub const fn p(id: &'static str, de: &'static str, en: &'static str, lat: f64, lon: f64) -> Place {
    Place { id, de, en, lat, lon }
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
