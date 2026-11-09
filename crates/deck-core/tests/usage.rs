//! Usage-Auswertung (Python `test_claude_usage.py`, ohne Netz und Token): Parser,
//! Formatierung, die zwei Fallen „hinter dem Reset“ und „eingefrorener Wert“.

use deck_core::claude::usage::*;
use deck_core::i18n::Lang;
use deck_core::isotime::parse_iso;
use serde_json::{Value, json};

const DE: Lang = Lang::German;

fn at(iso: &str) -> f64 {
    parse_iso(iso).expect(iso)
}

fn sample() -> Value {
    json!({
        "five_hour": {"utilization": 91.0, "resets_at": "2026-07-21T23:00:00+00:00"},
        "seven_day": {"utilization": 15.0, "resets_at": "2026-07-28T12:00:00+00:00"},
        "limits": [
            {"kind": "session", "group": "session", "percent": 91, "severity": "critical",
             "resets_at": "2026-07-21T23:00:00+00:00", "scope": null, "is_active": true},
            {"kind": "weekly_all", "group": "weekly", "percent": 15, "severity": "normal",
             "resets_at": "2026-07-28T12:00:00+00:00", "scope": null, "is_active": false},
            {"kind": "weekly_scoped", "group": "weekly", "percent": 0, "severity": "normal",
             "resets_at": null, "is_active": false,
             "scope": {"model": {"id": null, "display_name": "Fable"}, "surface": null}}
        ]
    })
}

fn snap(limits: Vec<Limit>, error: Option<&str>, ts: Option<f64>) -> UsageSnapshot {
    UsageSnapshot { limits, error: error.map(String::from), ts, ..Default::default() }
}

#[test]
fn iso_times_parse_like_python_fromisoformat() {
    assert_eq!(at("1970-01-01T00:00:00+00:00"), 0.0);
    assert_eq!(at("2026-01-01T12:45:00"), at("2026-01-01T12:45:00Z"));
    assert_eq!(at("2026-01-01T13:45:00+01:00"), at("2026-01-01T12:45:00+00:00"));
    assert_eq!(at("2026-07-21 23:00:00.500+00:00") - at("2026-07-21T23:00:00Z"), 0.5);
    assert_eq!(at("2026-01-01"), at("2026-01-01T00:00:00"));
    assert_eq!(parse_iso("kaputt"), None);
    assert_eq!(parse_iso("2026-02-30T00:00:00"), None);
}

#[test]
fn the_reset_is_shown_as_time_left() {
    let base = at("2026-01-01T12:00:00+00:00");
    assert_eq!(fmt_reset(Some("2026-01-01T12:45:00+00:00"), base, DE), "45 Min.");
    assert_eq!(fmt_reset(Some("2026-01-01T14:00:00+00:00"), base, DE), "2 Std.");
    assert_eq!(fmt_reset(Some("2026-01-01T14:30:00+00:00"), base, DE), "2 Std. 30 Min.");
    assert_eq!(fmt_reset(Some("2026-01-03T12:00:00+00:00"), base, DE), "2 Tg.");
    assert_eq!(fmt_reset(Some("2026-01-03T15:00:00+00:00"), base, DE), "2 Tg. 3 Std.");
    assert_eq!(fmt_reset(Some("2026-01-01T11:00:00+00:00"), base, DE), "jetzt");
    assert_eq!(fmt_reset(Some("2026-01-01T12:45:00"), base, DE), "45 Min.");
    assert_eq!(fmt_reset(None, base, DE), "");
    assert_eq!(fmt_reset(Some("kaputt"), base, DE), "");
    assert_eq!(fmt_reset(Some("2026-01-03T15:00:00+00:00"), base, Lang::English), "2d 3h");
}

#[test]
fn the_traffic_light_follows_severity_then_thresholds() {
    assert_eq!(severity_color("critical", Some(91)), "#ff6b6b");
    assert_eq!(severity_color("warning", Some(60)), "#ffc48a");
    assert_eq!(severity_color("normal", Some(15)), "#6ee7a8");
    assert_eq!(severity_color("", None), "#8b8b99");
    assert_eq!(severity_color("", Some(30)), "#6ee7a8");
    assert_eq!(severity_color("", Some(70)), "#ffc48a");
    assert_eq!(severity_color("", Some(95)), "#ff6b6b");
}

#[test]
fn modern_limits_are_parsed_with_readable_labels() {
    let limits = parse_usage(&sample(), DE);
    let s = session_of(&limits).unwrap();
    assert_eq!((s.percent, s.severity.as_str(), s.group.as_str()), (Some(91), "critical", "session"));
    assert_eq!(limits.iter().map(|l| l.label.as_str()).collect::<Vec<_>>(), ["Session", "Woche", "Fable (Woche)"]);
}

#[test]
fn an_older_answer_is_rebuilt_from_five_hour_and_seven_day() {
    let limits = parse_usage(&json!({"five_hour": {"utilization": 91.0, "resets_at": "x"}, "seven_day": {"utilization": 15.0, "resets_at": "y"}}), DE);
    assert_eq!(session_of(&limits).unwrap().percent, Some(91));
    assert_eq!(limits.iter().map(|l| l.label.as_str()).collect::<Vec<_>>(), ["Session", "Woche"]);
    assert!(parse_usage(&json!({}), DE).is_empty());
}

#[test]
fn percent_rounds_half_up_to_whole_numbers() {
    let limits = parse_usage(&json!({"limits": [{"kind": "session", "percent": 12.5}, {"kind": "five_hour_x", "percent": 13.5}]}), DE);
    assert_eq!(limits[0].percent, Some(13));
    assert_eq!(limits[1].percent, Some(14));
    assert_eq!(limits[1].label, "Five Hour X");
}

#[test]
fn the_tooltip_lists_session_and_week_with_resets_and_hides_silent_model_limits() {
    let base = at("2026-07-21T21:55:00+00:00");
    let txt = tooltip_text(&snap(parse_usage(&sample(), DE), None, None), base, DE);
    assert!(txt.contains("Claude · Nutzung"));
    assert!(txt.contains("Session: 91 %"));
    assert!(txt.contains("Woche: 15 %"));
    assert!(txt.contains("Reset in 1 Std. 5 Min."));
    assert!(!txt.contains("Fable"));
    assert!(tooltip_text(&snap(vec![], Some("nicht angemeldet"), None), base, DE).contains("nicht angemeldet"));
}

#[test]
fn a_limit_behind_its_reset_loses_its_number() {
    let base = at("2026-07-21T23:30:00+00:00");
    let limits = expire_limits(&parse_usage(&sample(), DE), base);
    let s = session_of(&limits).unwrap();
    assert_eq!((s.percent, s.severity.as_str(), s.stale), (None, "", true));
    let week = limits.iter().find(|l| l.kind == "weekly_all").unwrap();
    assert_eq!((week.percent, week.stale), (Some(15), false));
}

#[test]
fn a_limit_before_its_reset_or_without_one_stays() {
    let limits = expire_limits(&parse_usage(&sample(), DE), at("2026-07-21T21:55:00+00:00"));
    assert_eq!(session_of(&limits).unwrap().percent, Some(91));
    assert!(limits.iter().all(|l| !l.stale));
    let unknown = Limit { kind: "weekly_scoped".into(), percent: Some(7), resets_at: None, ..Default::default() };
    let broken = Limit { kind: "session".into(), percent: Some(7), resets_at: Some("murks".into()), ..Default::default() };
    assert_eq!(expire_limits(&[unknown, broken], 1e12).iter().map(|l| l.percent).collect::<Vec<_>>(), [Some(7), Some(7)]);
}

#[test]
fn age_is_named_only_from_the_threshold_on() {
    let now = 1_000_000.0;
    assert_eq!(fmt_age(Some(now - 60.0), now, STALE_AFTER, DE), "");
    assert_eq!(fmt_age(Some(now - 300.0), now, STALE_AFTER, DE), "");
    assert_eq!(fmt_age(Some(now - 3600.0), now, STALE_AFTER, DE), "vor 1 Std.");
    assert_eq!(fmt_age(Some(now - 5400.0), now, STALE_AFTER, DE), "vor 1 Std. 30 Min.");
    assert_eq!(fmt_age(None, now, STALE_AFTER, DE), "");
}

#[test]
fn the_tooltip_names_age_and_reason() {
    let base = at("2026-07-21T21:55:00+00:00");
    let limits = parse_usage(&sample(), DE);
    let old = tooltip_text(&snap(limits.clone(), Some("Rate-Limit"), Some(base - 3600.0)), base, DE);
    assert!(old.contains("(Stand vor 1 Std. · Rate-Limit)"), "{old}");
    let no_ts = tooltip_text(&snap(limits.clone(), Some("Rate-Limit"), None), base, DE);
    assert!(no_ts.contains("(letzter Wert · Rate-Limit)"), "{no_ts}");
    let fresh = tooltip_text(&snap(limits, None, Some(base)), base, DE);
    assert!(!fresh.contains('('), "{fresh}");
}

#[test]
fn applying_keeps_data_time_and_error_of_the_shared_poller() {
    let now = 2_000_000_000.0;
    let data = json!({"limits": [
        {"kind": "session", "group": "session", "percent": 48, "severity": "normal", "resets_at": "2033-05-18T03:03:20+00:00", "is_active": true},
        {"kind": "weekly_all", "group": "weekly", "percent": 9, "severity": "normal", "resets_at": "2033-05-19T03:33:20+00:00", "is_active": false}]});
    // 2033-05-18T03:03:20Z liegt 30 Min. vor `now` (2033-05-18T03:33:20Z).
    assert_eq!(at("2033-05-18T03:33:20+00:00"), now);
    let mut s = UsageSnapshot::default();
    apply(&mut s, &data, Some(now - 3600.0), Some("Rate-Limit".into()), now, DE);
    assert_eq!(s.ts, Some(now - 3600.0), "die Datenzeit ist die des Abrufs, nicht jetzt");
    assert_eq!(s.error.as_deref(), Some("Rate-Limit"));
    assert_eq!(s.state, "error");
    assert_eq!(s.session_percent, None, "48 % aus dem alten Fenster sind keine Zahl");
    assert_eq!(s.session_severity, "");
    assert_eq!(s.limits.iter().find(|l| l.kind == "weekly_all").unwrap().percent, Some(9));
}

#[test]
fn a_hard_failure_hides_the_number_and_a_soft_one_keeps_it() {
    let mut s = UsageSnapshot { session_percent: Some(5), limits: parse_usage(&sample(), DE), ..Default::default() };
    fail(&mut s, "Rate-Limit", false);
    assert_eq!((s.session_percent, s.state.as_str()), (Some(5), "error"));
    fail(&mut s, "Token ungueltig", true);
    assert_eq!(s.session_percent, None);
    assert!(s.limits.is_empty());
}

#[test]
fn badge_and_tooltip_share_one_staleness_threshold_and_no_timestamp_is_not_old() {
    let now = 1_000_000.0;
    let just_fresh = UsageSnapshot { ts: Some(now - (STALE_AFTER - 30.0)), ..Default::default() };
    let just_old = UsageSnapshot { ts: Some(now - (STALE_AFTER + 30.0)), ..Default::default() };
    assert!(!is_stale(&just_fresh, now) && is_stale(&just_old, now));
    assert_eq!(fmt_age(just_fresh.ts, now, STALE_AFTER, DE), "");
    assert_ne!(fmt_age(just_old.ts, now, STALE_AFTER, DE), "");
    assert!(!is_stale(&UsageSnapshot::default(), now));
}

#[test]
fn the_view_always_has_session_and_week_and_shows_extra_limits_only_with_signal() {
    let now = at("2026-07-21T21:55:00+00:00");
    let mut limits = parse_usage(&sample(), DE);
    limits.push(Limit { kind: "weekly_opus".into(), group: "weekly".into(), label: "Opus (Woche)".into(), percent: Some(30), ..Default::default() });
    let v = usage_view(&snap(limits, None, Some(now)), now, DE);
    assert_eq!(v.primary.iter().map(|l| (l.label.as_str(), l.percent)).collect::<Vec<_>>(), [("Session", Some(91.0)), ("Woche", Some(15.0))]);
    assert_eq!(v.primary[0].resets, "1 Std. 5 Min.");
    assert_eq!(v.primary[0].color, RED);
    assert_eq!(v.extra.iter().map(|l| l.label.as_str()).collect::<Vec<_>>(), ["Opus (Woche)"]);
    assert!(!v.stale && v.error.is_none() && v.as_of.is_empty());
    let empty = usage_view(&UsageSnapshot::default(), now, DE);
    assert_eq!(empty.primary.iter().map(|l| (l.label.as_str(), l.percent, l.color.as_str())).collect::<Vec<_>>(), [("Session", None, GRAY), ("Woche", None, GRAY)]);
    let old = usage_view(&snap(vec![], Some("Rate-Limit"), Some(now - 3600.0)), now, DE);
    assert!(old.stale);
    assert_eq!((old.as_of.as_str(), old.error.as_deref()), ("Stand vor 1 Std.", Some("Rate-Limit")));
}

// ── Zwei Konten (2026-11-08): nach '/login' auf das zweite Konto zeigte die Anzeige
// stundenlang die Zahlen des ersten.

#[test]
fn the_tooltip_names_the_account_and_has_no_empty_line_without_one() {
    let mut s = snap(vec![], None, None);
    s.account = Some("b@x.de".into());
    let de = tooltip_text(&s, 0.0, DE);
    assert!(de.contains("Konto: b@x.de"), "{de}");
    assert!(tooltip_text(&s, 0.0, Lang::English).contains("Account: b@x.de"));
    let without = tooltip_text(&snap(vec![], None, None), 0.0, DE);
    assert!(!without.contains("Konto"), "ohne bekanntes Konto keine leere Zeile: {without}");
    assert!(!without.contains("\n\n"));
}

#[test]
fn an_account_switch_drops_the_numbers_of_the_old_account() {
    let now = at("2026-07-21T21:55:00+00:00");
    let mut s = UsageSnapshot { account: Some("a@x.de".into()), ..Default::default() };
    apply(&mut s, &sample(), Some(now), Some("Rate-Limit".into()), now, DE);
    assert_eq!(s.session_percent, Some(91));
    note_account(&mut s, Some("a@x.de"));
    assert_eq!(s.session_percent, Some(91), "dasselbe Konto behält seine Zahl");
    note_account(&mut s, Some("b@x.de"));
    assert_eq!(s.account.as_deref(), Some("b@x.de"));
    assert_eq!((s.session_percent, s.session_resets_at.as_deref(), s.ts), (None, None, None), "die Zahl gehört dem alten Konto");
    assert!(s.limits.is_empty() && s.session_severity.is_empty());
    assert_eq!(s.error.as_deref(), Some("Rate-Limit"), "der Fehler beschreibt den Abruf und bleibt");
}

#[test]
fn the_view_carries_the_account_for_the_tooltip() {
    let mut s = UsageSnapshot { account: Some("b@x.de".into()), ..Default::default() };
    assert_eq!(usage_view(&s, 0.0, DE).account.as_deref(), Some("b@x.de"));
    s.account = Some(String::new());
    assert_eq!(usage_view(&s, 0.0, DE).account, None);
}
