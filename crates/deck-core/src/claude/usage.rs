//! Usage-Antwort auswerten und für Menschen aufbereiten, ohne Netz, Token und Anzeige.
//!
//! JSON rein, Snapshot raus; Snapshot rein, `view::UsageView` raus. Das Abrufen (Token,
//! HTTP, gemeinsamer Poller, Backoff) macht die App. Zwei Regeln aus schmerzhafter
//! Erfahrung stehen hier:
//!
//! * **Ein Limit hinter seinem Reset ist keine Zahl mehr** (2026-08-05: das Deck zeigte
//!   48 % aus dem Fenster von gestern, live waren es 13 %). `expire_limits`.
//! * **Ein alter Wert wird matt gezeigt, nicht versteckt** (2026-08-18: 18 Minuten
//!   grüne „0 %“ aus einem eingefrorenen Cache). `is_stale`, gleiche Schwelle wie die
//!   Stand-Zeile im Tooltip.

use serde_json::Value;

use crate::i18n::Lang;
use crate::isotime::parse_iso;
use crate::field::{flag, number, text};
use crate::view::{LimitView, UsageView};

pub const GREEN: &str = "#6ee7a8";
pub const AMBER: &str = "#ffc48a";
pub const RED: &str = "#ff6b6b";
pub const GRAY: &str = "#8b8b99";

/// Ab diesem Datenalter (s) nennt der Tooltip den Stand und das Badge wird matt.
/// Darunter ist Alter kein Befund: der gemeinsame Poller cacht regulär 90 s.
pub const STALE_AFTER: f64 = 900.0;

/// Ein Limit der API (Session, Woche, modellbezogene Woche …).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Limit {
    pub kind: String,
    pub group: String,
    pub label: String,
    pub percent: Option<i64>,
    pub severity: String,
    pub resets_at: Option<String>,
    pub active: bool,
    /// Hinter dem Reset entwertet (`expire_limits`).
    pub stale: bool,
}

/// Der Stand, den der Poller hält (Python `UsagePoller._snap`).
#[derive(Debug, Clone, PartialEq)]
pub struct UsageSnapshot {
    /// `pending`, `ok` oder `error`.
    pub state: String,
    pub session_percent: Option<i64>,
    pub session_severity: String,
    pub session_resets_at: Option<String>,
    pub limits: Vec<Limit>,
    pub error: Option<String>,
    /// Zeit der DATEN (Unix-Sekunden), nicht des Lesens.
    pub ts: Option<f64>,
    /// Konto, zu dem die Zahlen gehören (E-Mail, klein); `None` = unbekannt.
    pub account: Option<String>,
}

impl Default for UsageSnapshot {
    fn default() -> Self {
        UsageSnapshot {
            state: "pending".into(),
            session_percent: None,
            session_severity: String::new(),
            session_resets_at: None,
            limits: Vec::new(),
            error: None,
            ts: None,
            account: None,
        }
    }
}

/// Sekunden → „X Tg. Y Std.“ / „X Std. Y Min.“ / „X Min.“.
fn fmt_span(seconds: f64, lang: Lang) -> String {
    let d = (seconds / 86_400.0).floor() as i64;
    let h = (seconds.rem_euclid(86_400.0) / 3600.0).floor() as i64;
    let m = (seconds.rem_euclid(3600.0) / 60.0).floor() as i64;
    let en = lang.is_english();
    match (d, h, m) {
        (d, 0, _) if d > 0 => if en { format!("{d}d") } else { format!("{d} Tg.") },
        (d, h, _) if d > 0 => if en { format!("{d}d {h}h") } else { format!("{d} Tg. {h} Std.") },
        (_, h, 0) if h > 0 => if en { format!("{h}h") } else { format!("{h} Std.") },
        (_, h, m) if h > 0 => if en { format!("{h}h {m}min") } else { format!("{h} Std. {m} Min.") },
        (_, _, m) => if en { format!("{m}min") } else { format!("{m} Min.") },
    }
}

/// Restzeit bis zum Reset; leer/kaputt → leer, Vergangenheit → „jetzt“.
pub fn fmt_reset(iso: Option<&str>, now: f64, lang: Lang) -> String {
    let Some(at) = iso.and_then(parse_iso) else { return String::new() };
    let delta = at - now;
    if delta <= 0.0 {
        return lang.l("jetzt", "now").into();
    }
    fmt_span(delta, lang)
}

/// Datenalter als „vor X Std. Y Min.“; leer ohne Zeitstempel oder unter `min_seconds`.
pub fn fmt_age(ts: Option<f64>, now: f64, min_seconds: f64, lang: Lang) -> String {
    let Some(ts) = ts else { return String::new() };
    let age = now - ts;
    if age < min_seconds {
        return String::new();
    }
    let span = fmt_span(age, lang);
    if lang.is_english() { format!("{span} ago") } else { format!("vor {span}") }
}

/// Ampelfarbe: zuerst die severity der API, sonst per Schwelle (<50 grün, <80 amber,
/// sonst rot); ohne Wert grau.
pub fn severity_color(severity: &str, percent: Option<i64>) -> &'static str {
    match severity {
        "normal" => return GREEN,
        "warning" => return AMBER,
        "critical" => return RED,
        _ => {}
    }
    match percent {
        None => GRAY,
        Some(p) if p < 50 => GREEN,
        Some(p) if p < 80 => AMBER,
        Some(_) => RED,
    }
}

/// Steht im Snapshot ein Wert, der nachweislich nicht mehr frisch ist? Prüffaden ist
/// allein das ALTER, nicht der Fehlertext: so fällt auch ein still gestorbener Abruf auf.
pub fn is_stale(snap: &UsageSnapshot, now: f64) -> bool {
    !fmt_age(snap.ts, now, STALE_AFTER, Lang::German).is_empty()
}

/// `str.title()`: Großbuchstabe nach jedem Nicht-Buchstaben, sonst klein.
fn title_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_cased = false;
    for c in s.chars() {
        let cased = c.is_lowercase() || c.is_uppercase();
        if cased && !prev_cased {
            out.extend(c.to_uppercase());
        } else if cased {
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
        prev_cased = cased;
    }
    out
}

/// Name eines API-Limits für die Anzeige.
fn limit_label(lim: &serde_json::Map<String, Value>, lang: Lang) -> String {
    let kind = lim.get("kind").and_then(Value::as_str).unwrap_or("").to_lowercase();
    if kind == "session" {
        return "Session".into();
    }
    if kind.contains("opus") {
        return lang.l("Opus (Woche)", "Opus (week)").into();
    }
    if kind.contains("sonnet") {
        return lang.l("Sonnet (Woche)", "Sonnet (week)").into();
    }
    if kind == "weekly_scoped" {
        let model = lim
            .get("scope")
            .and_then(Value::as_object)
            .and_then(|s| s.get("model"))
            .and_then(Value::as_object)
            .and_then(|m| text(m.get("display_name")));
        return match model {
            Some(m) => format!("{m} {}", lang.l("(Woche)", "(week)")),
            None => lang.l("Woche (Modell)", "Week (model)").into(),
        };
    }
    if kind.starts_with("weekly") {
        return lang.l("Woche", "Week").into();
    }
    let t = title_case(&kind.replace('_', " "));
    if t.is_empty() { "Limit".into() } else { t }
}

/// Auf ganze Prozent gerundet; keine Zahl → `None`.
fn pct(v: Option<&Value>) -> Option<i64> {
    number(v).map(|f| f.round() as i64)
}

fn opt_str(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str).map(String::from)
}

/// Rohe API-Antwort → Limits. Bevorzugt das moderne `limits`-Array, sonst die älteren
/// Felder `five_hour` / `seven_day`.
pub fn parse_usage(data: &Value, lang: Lang) -> Vec<Limit> {
    if let Some(raw) = data.get("limits").and_then(Value::as_array).filter(|a| !a.is_empty()) {
        return raw
            .iter()
            .filter_map(Value::as_object)
            .map(|lim| Limit {
                kind: opt_str(lim.get("kind")).unwrap_or_default(),
                group: opt_str(lim.get("group")).unwrap_or_default(),
                label: limit_label(lim, lang),
                percent: pct(lim.get("percent")),
                severity: opt_str(lim.get("severity")).unwrap_or_default(),
                resets_at: opt_str(lim.get("resets_at")),
                active: flag(lim.get("is_active")),
                stale: false,
            })
            .collect();
    }
    let mut limits = Vec::new();
    let old = |key: &str, kind: &str, group: &str, label: String, active: bool| {
        let o = data.get(key).and_then(Value::as_object)?;
        let util = o.get("utilization").filter(|v| !v.is_null())?;
        Some(Limit {
            kind: kind.into(),
            group: group.into(),
            label,
            percent: pct(Some(util)),
            severity: String::new(),
            resets_at: opt_str(o.get("resets_at")),
            active,
            stale: false,
        })
    };
    limits.extend(old("five_hour", "session", "session", "Session".into(), true));
    limits.extend(old("seven_day", "weekly_all", "weekly", lang.l("Woche", "Week").into(), false));
    limits
}

/// Das Session-Limit aus einer Liste.
pub fn session_of(limits: &[Limit]) -> Option<&Limit> {
    limits.iter().find(|l| l.group == "session" || l.kind == "session")
}

/// Limits, deren Reset VORBEI ist, verlieren Prozent und Ampel (`stale`). Prüffaden ist
/// der Reset-Zeitpunkt, nicht das Datenalter; ohne bekannten Reset bleibt das Limit.
pub fn expire_limits(limits: &[Limit], now: f64) -> Vec<Limit> {
    limits
        .iter()
        .map(|lim| match lim.resets_at.as_deref().and_then(parse_iso) {
            Some(at) if at - now <= 0.0 => Limit { percent: None, severity: String::new(), stale: true, ..lim.clone() },
            _ => lim.clone(),
        })
        .collect()
}

/// Im Hover: Session und Wochen-Gesamt immer, modellbezogene nur mit Signal.
pub fn keep_in_tooltip(lim: &Limit) -> bool {
    lim.group == "session" || lim.kind == "weekly_all" || lim.percent.is_some_and(|p| p != 0) || lim.active
}

/// Geparste Antwort in den Snapshot legen, abgelaufene Limits entwertet. `ts` ist die
/// Zeit der DATEN (beim gemeinsamen Poller dessen `data_ts`), `error` bleibt auch neben
/// Daten stehen: wer ihn verschluckt, lässt einen eingefrorenen Wert als frisch gelten.
pub fn apply(snap: &mut UsageSnapshot, data: &Value, ts: Option<f64>, error: Option<String>, now: f64, lang: Lang) {
    let limits = expire_limits(&parse_usage(data, lang), now);
    let sess = session_of(&limits).cloned();
    snap.state = if error.is_some() { "error" } else { "ok" }.into();
    snap.session_percent = sess.as_ref().and_then(|s| s.percent);
    snap.session_severity = sess.as_ref().map(|s| s.severity.clone()).unwrap_or_default();
    snap.session_resets_at = sess.and_then(|s| s.resets_at);
    snap.limits = limits;
    snap.error = error;
    snap.ts = ts;
}

/// Fehler vermerken; ein harter Fehler (Token weg) blendet zusätzlich die Zahl aus.
pub fn fail(snap: &mut UsageSnapshot, msg: &str, hard: bool) {
    snap.state = "error".into();
    snap.error = Some(msg.into());
    if hard {
        snap.session_percent = None;
        snap.session_severity = String::new();
        snap.limits.clear();
    }
}

/// Konto der nächsten Daten vermerken (Python `UsagePoller._note_account`). Bei einem
/// Wechsel gehören die Zahlen nicht mehr hierher: ohne das bliebe bei einem Fehler direkt
/// nach dem Wechsel (429, gemeinsamer Poller gerade belegt) die Zahl des ALTEN Kontos
/// als „letzter Wert“ stehen. Der Fehlertext bleibt, er beschreibt den Abruf.
pub fn note_account(snap: &mut UsageSnapshot, account: Option<&str>) {
    if snap.account.as_deref() == account {
        return;
    }
    snap.account = account.map(String::from);
    snap.session_percent = None;
    snap.session_severity = String::new();
    snap.session_resets_at = None;
    snap.limits.clear();
    snap.ts = None;
}

/// Fußzeile „Stand …“ bzw. „letzter Wert“; leer, wenn frisch und ohne Fehler.
fn as_of(snap: &UsageSnapshot, now: f64, lang: Lang) -> String {
    let age = fmt_age(snap.ts, now, STALE_AFTER, lang);
    if !age.is_empty() {
        return format!("{} {age}", lang.l("Stand", "as of"));
    }
    if snap.error.as_deref().is_some_and(|e| !e.is_empty()) {
        return lang.l("letzter Wert", "last value").into();
    }
    String::new()
}

/// Mehrzeiliger Hover-Text (Python `tooltip_text`). Statt der Gedankenstriche der
/// Python-Fassung trennt `·` (Konvention dieses Repos für Oberflächentexte).
pub fn tooltip_text(snap: &UsageSnapshot, now: f64, lang: Lang) -> String {
    let mut head = lang.l("Claude · Nutzung", "Claude · usage").to_string();
    // Wessen Zahl das ist: seit es zwei Konten gibt die erste Frage. Ohne bekanntes
    // Konto keine leere Zeile.
    if let Some(acc) = snap.account.as_deref().filter(|a| !a.is_empty()) {
        head += &format!("
{}: {acc}", lang.l("Konto", "Account"));
    }
    let limits: Vec<&Limit> = snap.limits.iter().filter(|l| keep_in_tooltip(l)).collect();
    if limits.is_empty() {
        let err = snap.error.as_deref().filter(|e| !e.is_empty());
        return format!("{head}\n{}", err.unwrap_or(lang.l("warte auf Daten…", "waiting for data…")));
    }
    let mut lines = vec![head];
    for lim in limits {
        let p = lim.percent.map_or("— %".into(), |p| format!("{p} %"));
        let mut line = format!("{}: {p}", lim.label);
        let reset = fmt_reset(lim.resets_at.as_deref(), now, lang);
        if !reset.is_empty() {
            line += &format!("  ·  {} {reset}", lang.l("Reset in", "resets in"));
        }
        lines.push(line);
    }
    let note = as_of(snap, now, lang);
    if !note.is_empty() {
        match snap.error.as_deref().filter(|e| !e.is_empty()) {
            Some(err) => lines.push(format!("({note} · {err})")),
            None => lines.push(format!("({note})")),
        }
    }
    lines.join("\n")
}

fn limit_view(lim: Option<&Limit>, label: &str, now: f64, lang: Lang) -> LimitView {
    match lim {
        Some(l) => LimitView {
            label: l.label.clone(),
            percent: l.percent.map(|p| p as f64),
            color: severity_color(&l.severity, l.percent).into(),
            resets: fmt_reset(l.resets_at.as_deref(), now, lang),
        },
        None => LimitView { label: label.into(), percent: None, color: GRAY.into(), resets: String::new() },
    }
}

/// Das Anzeigemodell der Leiste: Session und Woche immer (unbekannt = „—“), weitere
/// Limits mit Signal nur im Tooltip. `color` ist die unverdunkelte Ampel; ist der Wert
/// alt (`stale`), zeichnet die Oberfläche matt.
pub fn usage_view(snap: &UsageSnapshot, now: f64, lang: Lang) -> UsageView {
    let session = session_of(&snap.limits);
    let week = snap.limits.iter().find(|l| l.kind == "weekly_all");
    let extra = snap
        .limits
        .iter()
        .filter(|l| keep_in_tooltip(l))
        .filter(|l| !session.is_some_and(|s| std::ptr::eq(*l, s)) && !week.is_some_and(|w| std::ptr::eq(*l, w)))
        .map(|l| limit_view(Some(l), &l.label, now, lang))
        .collect();
    UsageView {
        primary: vec![
            limit_view(session, "Session", now, lang),
            limit_view(week, lang.l("Woche", "Week"), now, lang),
        ],
        extra,
        stale: is_stale(snap, now),
        account: snap.account.clone().filter(|a| !a.is_empty()),
        error: snap.error.clone().filter(|e| !e.is_empty()),
        as_of: as_of(snap, now, lang),
    }
}
