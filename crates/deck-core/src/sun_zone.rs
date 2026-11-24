//! Der Ort für „Hell/Dunkel: Sonnenstand“, grob aus der Zeitzone von Windows.
//!
//! Ein Ortsfeld in den Einstellungen wäre eine Frage mehr, die kaum jemand beantworten
//! will. Für hell und dunkel reicht ein Ort auf ein paar hundert Kilometer: innerhalb einer
//! Zeitzone verschiebt sich der Sonnenuntergang um höchstens eine halbe Stunde, meist viel
//! weniger. Darum steht hier je Zeitzone (Schlüssel aus der Registratur, etwa
//! `W. Europe Standard Time`) ein Punkt in ihrer Mitte bzw. in ihrer größten Stadt.
//!
//! Unbekannte Zone: der Längengrad folgt aus dem Abstand zu UTC (15° je Stunde, die
//! Sonne steht dort um 12 Uhr Ortszeit ungefähr im Süden), die Breite ist ein mittlerer
//! Wert der Nordhalbkugel. Das liegt nie ganz daneben, und die Rechnung bleibt still.

use crate::sun::{Place, p};

/// Zeitzonen und ihr Ort. Die Kennung ist der Schlüssel unter
/// `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Time Zones`.
const ZONES: &[(&str, Place)] = &[
    ("W. Europe Standard Time", p("w-europe", "Zeitzone Mitteleuropa", "Central European time zone", 50.5, 10.0)),
    ("Central Europe Standard Time", p("central-europe", "Zeitzone Mitteleuropa (Ost)", "Central European time zone (east)", 48.5, 17.0)),
    ("Central European Standard Time", p("central-european", "Zeitzone Mitteleuropa (Ost)", "Central European time zone (east)", 50.5, 19.5)),
    ("Romance Standard Time", p("romance", "Zeitzone Westeuropa", "Western European time zone", 48.9, 2.4)),
    ("GMT Standard Time", p("gmt", "Zeitzone Großbritannien", "British time zone", 52.0, -1.5)),
    ("Greenwich Standard Time", p("greenwich", "Zeitzone Island", "Iceland time zone", 64.1, -21.9)),
    ("E. Europe Standard Time", p("e-europe", "Zeitzone Osteuropa", "Eastern European time zone", 47.0, 28.8)),
    ("FLE Standard Time", p("fle", "Zeitzone Nordosteuropa", "Northeastern European time zone", 56.5, 24.5)),
    ("GTB Standard Time", p("gtb", "Zeitzone Südosteuropa", "Southeastern European time zone", 41.0, 24.0)),
    ("Turkey Standard Time", p("turkey", "Zeitzone Türkei", "Turkey time zone", 41.0, 29.0)),
    ("Russian Standard Time", p("russian", "Zeitzone Moskau", "Moscow time zone", 55.8, 37.6)),
    ("Israel Standard Time", p("israel", "Zeitzone Israel", "Israel time zone", 31.8, 35.2)),
    ("Eastern Standard Time", p("us-eastern", "Zeitzone US-Ostküste", "US Eastern time zone", 40.7, -74.0)),
    ("Central Standard Time", p("us-central", "Zeitzone US-Mitte", "US Central time zone", 41.9, -87.6)),
    ("Mountain Standard Time", p("us-mountain", "Zeitzone US-Gebirge", "US Mountain time zone", 39.7, -105.0)),
    ("Pacific Standard Time", p("us-pacific", "Zeitzone US-Westküste", "US Pacific time zone", 37.8, -122.4)),
    ("Atlantic Standard Time", p("atlantic", "Zeitzone Atlantik (Kanada)", "Atlantic time zone (Canada)", 44.6, -63.6)),
    ("E. South America Standard Time", p("brasilia", "Zeitzone Brasília", "Brasília time zone", -15.8, -47.9)),
    ("India Standard Time", p("india", "Zeitzone Indien", "India time zone", 28.6, 77.2)),
    ("China Standard Time", p("china", "Zeitzone China", "China time zone", 31.2, 121.5)),
    ("Singapore Standard Time", p("singapore", "Zeitzone Singapur", "Singapore time zone", 1.35, 103.8)),
    ("Tokyo Standard Time", p("tokyo", "Zeitzone Japan", "Japan time zone", 35.7, 139.7)),
    ("AUS Eastern Standard Time", p("aus-eastern", "Zeitzone Ostaustralien", "Eastern Australia time zone", -33.9, 151.2)),
];

/// Breite für eine unbekannte Zone: Mitte der bewohnten Nordhalbkugel.
const FALLBACK_LAT: f64 = 45.0;

/// Ort zur Zeitzone `key`; unbekannt oder leer: aus `bias_min` gerechnet (Windows-Bias in
/// Minuten, UTC = Ortszeit + Bias, Mitteleuropa also -60).
pub fn place_for_zone(key: &str, bias_min: i32) -> Place {
    let key = key.trim();
    ZONES.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, p)| *p).unwrap_or_else(|| from_bias(bias_min))
}

/// Ort aus dem Abstand zu UTC allein.
pub fn from_bias(bias_min: i32) -> Place {
    // Ein Bias jenseits von ±14 h ist kaputt; dann UTC.
    let bias = if bias_min.abs() > 14 * 60 { 0 } else { bias_min };
    p("zone", "deine Zeitzone", "your time zone", FALLBACK_LAT, -f64::from(bias) / 4.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::isotime::days_from_civil;
    use crate::sun::is_light;

    #[test]
    fn a_known_zone_gives_its_place() {
        let p = place_for_zone("W. Europe Standard Time", -60);
        assert_eq!(p.id, "w-europe");
        assert!((p.lat - 50.5).abs() < 0.01 && (p.lon - 10.0).abs() < 0.01);
        assert_eq!(place_for_zone(" w. europe standard time ", 0).id, "w-europe", "Schreibweise egal");
    }

    #[test]
    fn an_unknown_zone_follows_its_distance_to_utc() {
        let cet = place_for_zone("Nirgendwo Standard Time", -60);
        assert_eq!(cet.id, "zone");
        assert!((cet.lon - 15.0).abs() < 0.01, "eine Stunde vor UTC = 15° Ost");
        assert!((place_for_zone("", 300).lon + 75.0).abs() < 0.01, "fünf Stunden hinter UTC = 75° West");
        assert_eq!(from_bias(99_999).lon, 0.0, "kaputter Bias: UTC");
    }

    #[test]
    fn every_zone_place_is_on_the_globe_and_named() {
        for (k, p) in ZONES {
            assert!(p.lat.abs() <= 90.0 && p.lon.abs() <= 180.0, "{k}");
            assert!(!p.de.is_empty() && !p.en.is_empty() && !p.id.is_empty(), "{k}");
            assert!(!p.de.contains(['—', '–']), "keine Gedankenstriche in Texten der Oberfläche: {k}");
        }
    }

    #[test]
    fn the_fallback_for_central_europe_switches_within_half_an_hour_of_the_table() {
        // Mittags hell, nachts dunkel, und an einem Novemberabend schalten Tabelle und
        // Rückfall höchstens eine halbe Stunde auseinander.
        let table = place_for_zone("W. Europe Standard Time", -60);
        let guess = from_bias(-60);
        let day = days_from_civil(2026, 11, 17) as f64 * 86_400.0;
        for p in [&table, &guess] {
            assert!(is_light(day + 11.0 * 3600.0, p) && !is_light(day + 22.0 * 3600.0, p));
        }
        let dark_from = |p: &Place| (0..24 * 60).map(|m| day + f64::from(m) * 60.0).find(|t| *t > day + 12.0 * 3600.0 && !is_light(*t, p));
        let (a, b) = (dark_from(&table).unwrap(), dark_from(&guess).unwrap());
        assert!((a - b).abs() <= 30.0 * 60.0, "{} min auseinander", (a - b).abs() / 60.0);
    }
}
