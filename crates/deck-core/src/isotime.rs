//! ISO-8601-Zeitpunkte in Unix-Sekunden, so großzügig wie Pythons
//! `datetime.fromisoformat` (3.11+), ohne eine Zeit-Bibliothek nachzuziehen.
//!
//! Gebraucht für `resets_at` der Usage-API (`2026-07-21T23:00:00+00:00`, auch mit `Z`
//! oder Sekundenbruchteilen). Eine Zeit ohne Zone gilt als UTC: die API liefert Zonen,
//! aber der Parser soll nicht daran hängen.

/// Tage seit 1970-01-01 für ein Datum im gregorianischen Kalender.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        _ => 28,
    }
}

fn num(s: &str) -> Option<i64> {
    (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().ok()).flatten()
}

fn parse_date(s: &str) -> Option<(i64, i64, i64)> {
    let (y, m, d) = match s.len() {
        10 if &s[4..5] == "-" && &s[7..8] == "-" => (&s[..4], &s[5..7], &s[8..]),
        8 => (&s[..4], &s[4..6], &s[6..]),
        _ => return None,
    };
    let (y, m, d) = (num(y)?, num(m)?, num(d)?);
    ((1..=12).contains(&m) && d >= 1 && d <= days_in_month(y, m) && y >= 1).then_some((y, m, d))
}

/// `HH[:MM[:SS[.ffffff]]]` (auch ohne Doppelpunkte) → Sekunden seit Mitternacht.
fn parse_clock(s: &str) -> Option<f64> {
    let (main, frac) = match s.find(['.', ',']) {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    let parts: Vec<&str> = if main.contains(':') {
        main.split(':').collect()
    } else {
        main.as_bytes().chunks(2).map(|c| std::str::from_utf8(c).unwrap_or("")).collect()
    };
    if parts.is_empty() || parts.len() > 3 || parts.iter().any(|p| p.len() != 2) {
        return None;
    }
    let h = num(parts[0])?;
    let m = parts.get(1).map_or(Some(0), |p| num(p))?;
    let sec = parts.get(2).map_or(Some(0), |p| num(p))?;
    if h > 23 || m > 59 || sec > 59 {
        return None;
    }
    let frac = match frac {
        Some(f) if parts.len() == 3 => {
            num(f)?;
            format!("0.{f}").parse::<f64>().ok()?
        }
        Some(_) => return None,
        None => 0.0,
    };
    Some((h * 3600 + m * 60 + sec) as f64 + frac)
}

/// Zonenversatz `Z`, `±HH`, `±HH:MM`, `±HHMM` (auch mit Sekunden) → Sekunden östlich
/// von UTC.
fn parse_offset(s: &str) -> Option<f64> {
    if s.eq_ignore_ascii_case("z") {
        return Some(0.0);
    }
    let sign = match s.as_bytes().first()? {
        b'+' => 1.0,
        b'-' => -1.0,
        _ => return None,
    };
    let clock = parse_clock(&s[1..])?;
    (clock < 86_400.0).then_some(sign * clock)
}

/// ISO-Zeitpunkt → Unix-Sekunden; leer oder kaputt → `None`.
pub fn parse_iso(s: &str) -> Option<f64> {
    if !s.is_ascii() {
        return None;
    }
    let (date, rest) = if s.len() >= 10 && s.as_bytes().get(4) == Some(&b'-') {
        (&s[..10], &s[10..])
    } else if s.len() >= 8 {
        (&s[..8], &s[8..])
    } else {
        return None;
    };
    let (y, m, d) = parse_date(date)?;
    let mut secs = (days_from_civil(y, m, d) * 86_400) as f64;
    if rest.is_empty() {
        return Some(secs);
    }
    // Trenner: ein beliebiges einzelnes Zeichen (meist `T` oder Leerzeichen).
    let time = &rest[1..];
    let tz_at = time.find(['+', '-', 'Z', 'z']);
    let (clock, tz) = match tz_at {
        Some(i) => (&time[..i], Some(&time[i..])),
        None => (time, None),
    };
    secs += parse_clock(clock)?;
    if let Some(tz) = tz {
        secs -= parse_offset(tz)?;
    }
    Some(secs)
}
