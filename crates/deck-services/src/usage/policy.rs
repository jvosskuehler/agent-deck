//! Die Poll-Politik des Usage-Endpoints: gelernter Takt, 429-Backoff, `Retry-After`.
//!
//! Wörtlich aus `claude-usage-shared/usage_poller.py` übernommen, samt Zahlen: der
//! gemeinsame Cache wird von Tray-Monitor, Python-Deck und dieser Fassung zugleich
//! gelesen und geschrieben, alle drei müssen dieselbe Politik fahren.
//!
//! Vorgeschichte (aus dem Python-Original): 90 s liefen tagelang mit 100 % Erfolg, bis
//! zum 2026-08-17, 19 Uhr. Ab da scheiterte bei UNVERÄNDERTEM Takt fast die Hälfte aller
//! Abrufe. Das Limit ist nichts, was man einmal ausmisst; darum lernt der Takt selbst.

/// Startwert des gelernten Takts (s), gemessen sicher.
pub const INTERVAL_START: f64 = 300.0;
/// Schnellster erlaubter Takt: feiner braucht keine Anzeige eines 5-Stunden-Fensters.
pub const INTERVAL_MIN: f64 = 120.0;
/// Langsamster Takt: darüber wird die Zahl unbrauchbar (und ab hier gilt sie als alt).
pub const INTERVAL_MAX: f64 = 900.0;
/// 429 → Takt strecken (multiplikativ, wirkt schnell).
pub const INTERVAL_UP: f64 = 1.5;
/// Erfolg → vorsichtig wieder verkürzen.
pub const INTERVAL_DOWN: f64 = 0.9;
/// Start der akuten 429-Kurve (s). Getrennt vom gelernten Takt: sie beantwortet „wie
/// lange ist JETZT zu?“ und darf nicht mitwachsen.
pub const BACKOFF_BASE: f64 = 90.0;
/// Deckel des akuten Backoffs (10 Minuten).
pub const BACKOFF_MAX: f64 = 600.0;
/// Untergrenze, falls der Server eine Winzigkeit nennt: `Retry-After: 1` wird kein
/// Sekundentakt.
pub const BACKOFF_MIN: f64 = 20.0;
/// Wartezeit nach weichem Fehler (Netz, 5xx).
pub const SOFT_RETRY: f64 = 25.0;
/// ±12 % Streuung, damit niemand mit Claude Desktop im Gleichschritt pollt.
pub const JITTER: f64 = 0.12;

/// Sekunden mit Streuung; `unit` ist eine Zufallszahl in [-1, 1].
pub fn jit(seconds: f64, unit: f64) -> f64 {
    seconds * (1.0 + JITTER * unit.clamp(-1.0, 1.0))
}

/// `Retry-After` → Sekunden oder `None`. Erlaubt sind eine Zahl oder ein HTTP-Datum;
/// alles andere gilt als „nicht gesagt“, NICHT als 0: aus „unverständlich“ darf kein
/// „sofort nochmal“ werden. Ein Datum in der Vergangenheit ist eine erfüllte Frist (0).
pub fn parse_retry_after(value: Option<&str>, now: f64) -> Option<f64> {
    let txt = value?.trim();
    if txt.is_empty() {
        return None;
    }
    if let Ok(n) = txt.parse::<f64>() {
        return n.is_finite().then_some(n.max(0.0));
    }
    let at = httpdate::parse_http_date(txt).ok()?;
    let secs = at.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs_f64();
    Some((secs - now).max(0.0))
}

/// Gelernter Grundtakt nach einem Abruf (AIMD): bei Ablehnung schnell zurückweichen
/// (×1,5), bei Erfolg nur zögerlich heran (×0,9). Die Asymmetrie ist der ganze Trick:
/// der Takt kriecht von unten an die Grenze und rückt beim ersten 429 deutlich ab.
/// Ein kaputter gespeicherter Wert (fehlt, ≤ 0) fällt auf den Startwert, nie auf
/// Dauerfeuer.
pub fn next_interval(current: Option<f64>, ok: bool) -> f64 {
    let cur = current.filter(|c| c.is_finite() && *c > 0.0).unwrap_or(INTERVAL_START);
    let next = cur * if ok { INTERVAL_DOWN } else { INTERVAL_UP };
    next.clamp(INTERVAL_MIN, INTERVAL_MAX)
}

/// Wartezeit nach einem 429; `n` = Fehlversuche in Folge. Mit Auskunft des Servers gilt
/// genau diese (geklemmt), ohne wird exponentiell geraten (90 → 180 → 360 → 600).
pub fn backoff_after_429(n: u32, retry_after: Option<f64>) -> f64 {
    if let Some(ra) = retry_after {
        return ra.clamp(BACKOFF_MIN, BACKOFF_MAX);
    }
    let exp = n.max(1) - 1;
    (BACKOFF_BASE * 2f64.powi(exp.min(30) as i32)).min(BACKOFF_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2026-08-18 11:00:00 UTC, fest gesetzt: sonst läuft der Test irgendwann ab.
    const ELF_UHR: f64 = 1_787_050_800.0;

    #[test]
    fn a_number_is_read_as_seconds() {
        assert_eq!(parse_retry_after(Some("45"), 0.0), Some(45.0));
        assert_eq!(parse_retry_after(Some(" 45 "), 0.0), Some(45.0));
    }

    #[test]
    fn an_http_date_becomes_the_remaining_seconds() {
        assert_eq!(parse_retry_after(Some("Tue, 18 Aug 2026 12:00:00 GMT"), ELF_UHR), Some(3600.0));
    }

    #[test]
    fn an_unreadable_header_is_no_information_not_zero() {
        for junk in [None, Some(""), Some("   "), Some("bald"), Some("vielleicht 30")] {
            assert_eq!(parse_retry_after(junk, ELF_UHR), None, "{junk:?}");
        }
    }

    #[test]
    fn a_date_in_the_past_yields_zero_not_negative() {
        assert_eq!(parse_retry_after(Some("Tue, 18 Aug 2026 12:00:00 GMT"), ELF_UHR + 7200.0), Some(0.0));
    }

    #[test]
    fn without_information_the_backoff_is_guessed_exponentially() {
        assert_eq!(backoff_after_429(1, None), 90.0);
        assert_eq!(backoff_after_429(2, None), 180.0);
        assert_eq!(backoff_after_429(3, None), 360.0);
    }

    #[test]
    fn the_guessed_backoff_is_capped() {
        assert_eq!(backoff_after_429(9, None), BACKOFF_MAX);
        assert_eq!(backoff_after_429(u32::MAX, None), BACKOFF_MAX);
    }

    #[test]
    fn the_servers_answer_beats_the_curve_even_downwards() {
        // Bei n=4 hätte die Kurve 600 s gewartet. Sagt der Server 30, gilt 30.
        assert_eq!(backoff_after_429(4, Some(30.0)), 30.0);
    }

    #[test]
    fn the_servers_answer_is_capped_upwards() {
        assert_eq!(backoff_after_429(1, Some(99_999.0)), BACKOFF_MAX);
    }

    #[test]
    fn a_tiny_answer_falls_to_the_lower_bound() {
        assert_eq!(backoff_after_429(1, Some(1.0)), BACKOFF_MIN);
    }

    #[test]
    fn a_rejection_stretches_the_interval_noticeably() {
        assert_eq!(next_interval(Some(300.0), false), 450.0);
    }

    #[test]
    fn a_success_shortens_only_hesitantly() {
        assert_eq!(next_interval(Some(300.0), true), 270.0);
    }

    #[test]
    fn the_asymmetry_is_the_whole_trick() {
        let up = next_interval(Some(300.0), false) - 300.0;
        let down = 300.0 - next_interval(Some(300.0), true);
        assert!(up > down);
    }

    #[test]
    fn the_interval_stays_within_its_bounds() {
        assert_eq!(next_interval(Some(10.0), true), INTERVAL_MIN);
        assert_eq!(next_interval(Some(99_999.0), false), INTERVAL_MAX);
        let mut iv = 900.0;
        for _ in 0..50 {
            iv = next_interval(Some(iv), true);
        }
        assert_eq!(iv, INTERVAL_MIN);
    }

    #[test]
    fn a_broken_stored_value_falls_back_to_the_start_value() {
        for junk in [None, Some(0.0), Some(-5.0), Some(f64::NAN)] {
            let iv = next_interval(junk, true);
            assert!((INTERVAL_MIN..=INTERVAL_MAX).contains(&iv), "{junk:?}");
        }
    }

    #[test]
    fn the_interval_finds_its_way_back_after_a_blocked_phase() {
        let mut iv = INTERVAL_START;
        for _ in 0..6 {
            iv = next_interval(Some(iv), false);
        }
        assert!(iv > INTERVAL_START);
        for _ in 0..40 {
            iv = next_interval(Some(iv), true);
        }
        assert_eq!(iv, INTERVAL_MIN);
    }

    #[test]
    fn jitter_spreads_by_at_most_twelve_percent() {
        assert_eq!(jit(100.0, 0.0), 100.0);
        assert!((jit(100.0, 1.0) - 112.0).abs() < 1e-9);
        assert!((jit(100.0, -5.0) - 88.0).abs() < 1e-9);
    }
}
