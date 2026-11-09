//! Reine Status-Interpretation: wann gilt eine Meldung als frisch, wann fällt ein
//! „denkt“ auf idle zurück, wann ist eine Verbindung verloren, wie löst sich die
//! xhigh/ultracode-Kollision auf und wann übernimmt das Deck einen per Hook gemeldeten
//! Permission-Modus. Keine Anzeige, keine Seiteneffekte.

use crate::pystr::PyNum;
use crate::slot_state::SlotState;

/// Status, die eine Kachel zeigen kann (die Schlüssel von `GLOW_STYLE` in Python).
/// Alles andere gilt als idle.
pub const VALID_STATUSES: [&str; 7] = ["idle", "done", "thinking", "running", "waiting", "unknown", "none"];

/// Rangfolge für den Griff: wichtig → unwichtig. `lost` meldet kein Hook, das Panel
/// errechnet ihn. `unknown` steht hinter `idle`: eine Kachel, über die nichts bekannt
/// ist, lässt den Griff nicht aufleuchten, ist aber auch nicht „kein Agent“.
pub const DECK_PRIORITY: [&str; 7] = ["waiting", "done", "lost", "thinking", "idle", "unknown", "none"];

fn ts_of(st: &SlotState) -> f64 {
    st.get("ts").and_then(PyNum::from_value).map_or(0.0, PyNum::as_f64)
}

/// Meldung frisch = existiert und ist nicht älter als `stale_s` Sekunden. Ein leerer
/// Zustand zählt wie keiner: er entsteht aus einer halb geschriebenen Datei.
pub fn is_fresh(st: Option<&SlotState>, now: f64, stale_s: f64) -> bool {
    match st {
        Some(st) if !st.is_empty() => now - ts_of(st) <= stale_s,
        _ => false,
    }
}

/// Ausgangsstatus einer Kachel, bevor `normalize_status` greift.
///
/// Ohne Meldung gilt eine Kachel als `idle`: sie wird gezeichnet, den Agenten gibt es
/// also. Bei einem REMOTE-Fenster stimmt das nicht: dessen Hooks schreiben in den
/// Zustandsordner der Gegenstelle, hier käme auch eine Rückfrage nie an. `idle` wäre da
/// eine beruhigende Behauptung statt einer Beobachtung, also `unknown`. Eine echte
/// Meldung gewinnt weiterhin, auch bei remote.
pub fn initial_status(st: Option<&SlotState>, remote: bool) -> String {
    match st {
        Some(st) if !st.is_empty() => match st.get("status") {
            Some(serde_json::Value::String(s)) if !s.is_empty() => s.clone(),
            // Ein nicht-leerer Nicht-Text ist in Python kein gültiger Status → idle.
            _ => "idle".into(),
        },
        _ => if remote { "unknown" } else { "idle" }.into(),
    }
}

/// Status für die Anzeige: `thinking`/`running` ohne frische Meldung gilt als
/// eingeschlafen (idle), ein unbekannter Status ebenfalls.
pub fn normalize_status(status: &str, fresh: bool, valid: &[&str]) -> String {
    if matches!(status, "thinking" | "running") && !fresh {
        return "idle".into();
    }
    if !valid.contains(&status) {
        return "idle".into();
    }
    status.into()
}

/// Rot = Verbindung zum Fenster verloren. Nur für frische, aktive Agenten, damit alte
/// Restdateien beim Start nicht fälschlich rot werden.
pub fn is_lost(status: &str, fresh: bool, connected: bool) -> bool {
    status != "none" && fresh && !connected
}

/// `running` und `thinking` sind derselbe Zustand (gleiche Farbe, gleicher Rang).
fn canonical(key: &str) -> &str {
    if key == "running" { "thinking" } else { key }
}

fn deck_rank(key: &str) -> usize {
    let key = canonical(key);
    DECK_PRIORITY.iter().position(|k| *k == key).unwrap_or(DECK_PRIORITY.len())
}

/// Alle Kachel-Status zu einem Gesamtzustand verdichten (Farbe des Griffs):
/// Rückfrage > ungelesen > getrennt > denkt > idle. Keine Kachel → `none`.
pub fn dominant_status<S: AsRef<str>>(keys: &[S]) -> &'static str {
    let ks: Vec<&str> = keys.iter().map(|k| canonical(k.as_ref())).collect();
    DECK_PRIORITY.iter().find(|k| ks.contains(k)).copied().unwrap_or("none")
}

/// Wird der Gesamtzustand dringlicher? Nur dann blitzt der Griff. Ein Wechsel auf einen
/// ruhigeren Zustand (ungelesen → idle, weil du gelesen hast) ist deine eigene Geste.
pub fn escalated(prev: &str, key: &str) -> bool {
    key != prev && deck_rank(key) < deck_rank(prev)
}

/// Effort-Kollision auflösen: die statusLine meldet für xhigh UND ultracode nur
/// `xhigh`. Das gemerkte Effort gewinnt bei leer/`xhigh`; meldet die statusLine ein
/// konkretes anderes Level (etwa nach einem Modellwechsel), gewinnt dieser echte Wert.
pub fn resolve_effort(live_eff: &str, remembered: Option<&str>) -> String {
    match remembered {
        Some(r) if !r.is_empty() && matches!(live_eff, "" | "xhigh") => r.into(),
        _ => live_eff.into(),
    }
}

/// Anzahl Shift+Tab vom angenommenen aktuellen zum Ziel-Modus (zyklisch).
/// `remembered` = gemerkter Modus-Index (fehlt → Start-Modus wie bei einem frischen
/// Chat). Liefert `(schritte, ziel_index)`, oder `None`, wenn das Ziel nicht im Zyklus
/// liegt.
pub fn mode_steps<S: AsRef<str>>(
    remembered: Option<usize>,
    target: &str,
    cycle: &[S],
    start: &str,
) -> Option<(usize, usize)> {
    let idx = |name: &str| cycle.iter().position(|m| m.as_ref() == name);
    let tgt = idx(target)?;
    let cur = remembered.unwrap_or_else(|| idx(start).unwrap_or(0)) as i64;
    let n = cycle.len() as i64;
    Some((((tgt as i64 - cur).rem_euclid(n)) as usize, tgt))
}

/// Shift+Tab-Reihenfolge einer Session. `bypass` kommt nur vor, wenn die Session im
/// Bypass-Modus gestartet wurde, und zwar direkt nach `plan` (Claude Code 2.1.319:
/// default → acceptEdits → plan → bypassPermissions → auto). Ohne `plan` fehlt der Anker:
/// dann lieber unverändert als an falscher Stelle.
pub fn mode_cycle<S: AsRef<str>>(cycle: &[S], bypass: bool) -> Vec<String> {
    let mut out: Vec<String> = cycle.iter().map(|s| s.as_ref().to_string()).collect();
    if !bypass || out.iter().any(|m| m == "bypass") {
        return out;
    }
    if let Some(i) = out.iter().position(|m| m == "plan") {
        out.insert(i + 1, "bypass".into());
    }
    out
}

/// Ist-Permission-Modus aus einem Hook-Event übernehmen: `(modus_index, ts)` bei einem
/// NEUEREN Event mit gültigem Modus, sonst `None`. So folgt die Annahme des Decks dem
/// zuletzt gemeldeten echten Modus.
pub fn adopt_hook_mode<S: AsRef<str>>(prev_ts: f64, st: &SlotState, cycle: &[S]) -> Option<(usize, f64)> {
    let m = st.get("mode")?.as_str()?;
    let ts = ts_of(st);
    let i = cycle.iter().position(|c| c.as_ref() == m)?;
    (ts > prev_ts).then_some((i, ts))
}
