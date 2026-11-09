//! Die Flag-Zeile für `claude` beim Start eines Agenten (`--model`, `--effort`,
//! `--permission-mode`, auf Wunsch `--resume`).
//!
//! Die Zeile geht als Text in eine Shell (Startskript der Terminal-Agenten). Darum kommt nur hinein, was die Prüfungen hier
//! besteht; alles andere fällt still weg.

use std::sync::LazyLock;

use regex::Regex;


/// Eine Session-ID ist eine UUID. Keine Kosmetik: der Wert landet ungeschützt in einer
/// Shell-Zeile (`claude --resume "<id>"`), und eine Datei ist kein Beweis.
static UUID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$").expect("UUID")
});
/// Modellnamen wie `opus[1m]`.
static MODEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9._\-\[\]]+$").expect("MODEL"));

/// Gültige `--effort`-Level; `ultracode` ist keins, sondern xhigh plus ein Schalter,
/// den die CLI nicht kennt.
const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

/// Deck-Modusname → `--permission-mode`. Bewusst nur die vier, die das Deck selbst
/// setzt: Berechtigungen per Automatik AUFZUWEITEN (bypass) wäre der falsche Fehler.
fn cli_mode(mode: &str) -> Option<&'static str> {
    match mode {
        "manual" => Some("default"),
        "accept" => Some("acceptEdits"),
        "plan" => Some("plan"),
        "auto" => Some("auto"),
        _ => None,
    }
}

/// Ist das eine brauchbare Session-ID (UUID, Rand-Leerraum egal)?
pub fn valid_session(sid: &str) -> bool {
    UUID.is_match(sid.trim())
}

/// Die Flag-Zeile für `claude`, fertig zitiert, in fester Reihenfolge. Was die Prüfungen
/// nicht besteht, fällt still weg: ein Agent ohne sein Modell ist ärgerlich, eine
/// Zeichenkette, die eine Shell-Zeile aufbricht, ist ein Loch.
pub fn flags(session_id: Option<&str>, model: Option<&str>, effort: Option<&str>, mode: Option<&str>) -> String {
    let mut parts = Vec::new();
    if let Some(sid) = session_id.filter(|s| valid_session(s)) {
        parts.push(format!("--resume \"{}\"", sid.trim()));
    }
    if let Some(m) = model.map(str::trim).filter(|m| MODEL.is_match(m)) {
        parts.push(format!("--model \"{m}\""));
    }
    if let Some(e) = effort {
        let lvl = e.trim().to_lowercase();
        let lvl = if lvl == "ultracode" { "xhigh".to_string() } else { lvl };
        if EFFORTS.contains(&lvl.as_str()) {
            parts.push(format!("--effort {lvl}"));
        }
    }
    if let Some(cli) = mode.and_then(|m| cli_mode(&m.trim().to_lowercase())) {
        parts.push(format!("--permission-mode {cli}"));
    }
    parts.join(" ")
}
