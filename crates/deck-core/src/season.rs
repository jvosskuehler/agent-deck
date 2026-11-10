//! Wann der Fokus-Rahmen sein Winterkleid trägt: vom ersten Advent bis Dreikönig
//! (6. Januar). Davor und danach laufen die Stammdesigns.
//!
//! Der erste Advent ist der vierte Sonntag vor dem 25. Dezember, also der Sonntag vom
//! 27. November bis 3. Dezember. Gerechnet wird ohne Zeit-Bibliothek; das Datum (in
//! Ortszeit) gibt der Aufrufer herein.

use serde_json::Value;

/// Letzter Tag des Winterkleids im Januar (Dreikönig).
const LAST_DAY_IN_JANUARY: u32 = 6;

/// Wochentag eines Datums, 0 = Sonntag (Sakamoto).
fn weekday(year: i32, month: u32, day: u32) -> u32 {
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if month < 3 { year - 1 } else { year };
    (y + y / 4 - y / 100 + y / 400 + T[(month as usize - 1) % 12] + day as i32).rem_euclid(7) as u32
}

/// Der erste Advent des Jahres als `(Monat, Tag)`.
pub fn first_advent(year: i32) -> (u32, u32) {
    let day = 27 + (7 - weekday(year, 11, 27)) % 7;
    if day > 30 { (12, day - 30) } else { (11, day) }
}

/// Liegt der Tag zwischen dem ersten Advent und Dreikönig (beide eingeschlossen)?
pub fn in_season(year: i32, month: u32, day: u32) -> bool {
    match month {
        1 => day <= LAST_DAY_IN_JANUARY,
        11 | 12 => (month, day) >= first_advent(year),
        _ => false,
    }
}

/// Soll der Rahmen heute sein Winterkleid tragen? Die Einstellung `frame_season` in
/// `deck_settings.json` geht vor: `"winter"` zeigt es sofort (zum Ausprobieren vor dem
/// Advent), `"standard"` nie. Fehlt sie oder steht etwas anderes darin, gilt das Datum.
pub fn winter_frames(setting: Option<&Value>, today: (i32, u32, u32)) -> bool {
    match setting.and_then(Value::as_str).map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("winter") => true,
        Some("standard") => false,
        _ => in_season(today.0, today.1, today.2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_first_advent_is_the_sunday_between_november_27_and_december_3() {
        assert_eq!(first_advent(2022), (11, 27), "frühester Fall");
        assert_eq!(first_advent(2023), (12, 3), "spätester Fall");
        assert_eq!(first_advent(2025), (11, 30));
        assert_eq!(first_advent(2026), (11, 29));
        assert_eq!(first_advent(2027), (11, 28));
        for year in 2020..2060 {
            let (m, d) = first_advent(year);
            assert_eq!(weekday(year, m, d), 0, "{year}: ein Sonntag");
        }
    }

    #[test]
    fn the_winter_look_runs_from_the_first_advent_through_epiphany() {
        assert!(!in_season(2026, 11, 10));
        assert!(!in_season(2026, 11, 28), "Samstag vor dem ersten Advent");
        assert!(in_season(2026, 11, 29));
        assert!(in_season(2026, 12, 24));
        assert!(in_season(2027, 1, 6));
        assert!(!in_season(2027, 1, 7));
        assert!(!in_season(2027, 7, 1));
        assert!(!in_season(2023, 12, 2), "2023 beginnt der Advent erst am 3. Dezember");
        assert!(in_season(2023, 12, 3));
    }

    #[test]
    fn the_setting_overrides_the_calendar_in_both_directions() {
        let summer = (2026, 7, 1);
        let advent = (2026, 12, 10);
        assert!(winter_frames(Some(&json!("winter")), summer));
        assert!(winter_frames(Some(&json!(" Winter ")), summer), "Schreibweise egal");
        assert!(!winter_frames(Some(&json!("standard")), advent));
        assert!(!winter_frames(None, summer));
        assert!(winter_frames(None, advent));
        assert!(winter_frames(Some(&json!("auto")), advent), "Unbekanntes: Datum gilt");
        assert!(winter_frames(Some(&json!(true)), advent), "kein Text: Datum gilt");
    }
}
