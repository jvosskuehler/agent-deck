use super::*;
use crate::isotime::days_from_civil;

/// Unix-Sekunden für ein UTC-Datum mit Uhrzeit.
fn at(y: i64, m: i64, d: i64, h: i64, min: i64) -> f64 {
    (days_from_civil(y, m, d) * 86_400 + h * 3600 + min * 60) as f64
}

const HAMBURG: Place = p("hamburg", "Hamburg", "Hamburg", 53.55, 10.00);

fn hamburg() -> &'static Place {
    &HAMBURG
}

/// Auf- und Untergang eines Datums in Minuten nach Mitternacht UTC.
fn rise_set(y: i64, m: i64, d: i64, p: &Place) -> (f64, f64) {
    let day = days_from_civil(y, m, d);
    match sun_day(day_of_year(day), p.lat, p.lon) {
        SunDay::Normal { rise, set } => (rise, set),
        other => panic!("kein Auf- und Untergang: {other:?}"),
    }
}

/// Vergleichswerte für Hamburg (53,55 / 10,00) aus dem vollständigen NOAA-Verfahren
/// (Julianische Jahrhunderte, wie im Solar Calculator der NOAA), Minuten nach Mitternacht
/// UTC. Die Näherung trifft um die Sonnenwenden auf zwei Minuten; dazwischen liegt sie bis
/// zu drei Minuten daneben (gemessen 17.11.2026: März 2,4, November 2,1). Für hell und
/// dunkel genügt das, und es ist genau die Rechnung des früheren Skripts.
#[test]
fn hamburg_sunrise_and_sunset_match_the_reference() {
    let cases = [
        // (Jahr, Monat, Tag, Aufgang, Untergang, erlaubte Abweichung in Minuten)
        (2026, 6, 21, 170.3, 1193.3, 2.0),
        (2026, 11, 17, 409.4, 919.9, 3.0),
        (2026, 12, 21, 454.3, 901.8, 2.0),
        (2026, 3, 20, 322.7, 1053.2, 3.0),
    ];
    for (y, m, d, want_rise, want_set, tol) in cases {
        let (rise, set) = rise_set(y, m, d, hamburg());
        assert!((rise - want_rise).abs() <= tol, "{y}-{m}-{d} Aufgang {rise:.1} statt {want_rise}");
        assert!((set - want_set).abs() <= tol, "{y}-{m}-{d} Untergang {set:.1} statt {want_set}");
    }
}

#[test]
fn it_is_light_between_sunrise_and_sunset_and_dark_otherwise() {
    let h = hamburg();
    // 17.11.2026: Aufgang 06:48 UTC, Untergang 15:22 UTC.
    assert!(!is_light(at(2026, 11, 17, 6, 40), h));
    assert!(is_light(at(2026, 11, 17, 7, 0), h));
    assert!(is_light(at(2026, 11, 17, 15, 15), h));
    assert!(!is_light(at(2026, 11, 17, 15, 30), h));
    assert!(!is_light(at(2026, 11, 17, 23, 0), h));
}

#[test]
fn the_next_change_is_todays_sunrise_then_sunset_then_tomorrows_sunrise() {
    let h = hamburg();
    let (rise, set) = rise_set(2026, 11, 17, h);
    let day = at(2026, 11, 17, 0, 0);
    assert_eq!(next_change(at(2026, 11, 17, 3, 0), h), Some(day + rise * 60.0));
    assert_eq!(next_change(at(2026, 11, 17, 12, 0), h), Some(day + set * 60.0));
    let (rise2, _) = rise_set(2026, 11, 18, h);
    assert_eq!(next_change(at(2026, 11, 17, 20, 0), h), Some(day + 86_400.0 + rise2 * 60.0));
    // Genau am Wechsel: der nächste, nicht derselbe.
    assert_eq!(next_change(day + rise * 60.0, h), Some(day + set * 60.0));
}

#[test]
fn the_day_of_year_counts_from_the_first_of_january() {
    assert_eq!(day_of_year(days_from_civil(2026, 1, 1)), 1);
    assert_eq!(day_of_year(days_from_civil(2026, 11, 17)), 321);
    assert_eq!(day_of_year(days_from_civil(2028, 12, 31)), 366);
}

#[test]
fn polar_day_and_night_have_no_change() {
    let north = Place { id: "x", de: "x", en: "x", lat: 78.0, lon: 15.0 };
    assert_eq!(sun_day(172, north.lat, north.lon), SunDay::AlwaysUp);
    assert_eq!(sun_day(355, north.lat, north.lon), SunDay::AlwaysDown);
    assert!(is_light(at(2026, 6, 21, 0, 0), &north));
    assert!(!is_light(at(2026, 12, 21, 12, 0), &north));
    assert_eq!(next_change(at(2026, 6, 21, 12, 0), &north), None);
}
