//! Chat-Titel, die mit dem Chat mitwachsen.
//!
//! Claude Code bildet den Titel einer Session aus der ersten Nachricht und behält ihn.
//! Wer sieben Agenten offen hat, erkennt am Titel aber nur dann, worum es geht, wenn er
//! dem Chat folgt. Darum schreibt das Lagebild-Modell (`briefing.rs`) bei jedem Lauf eine
//! Zeile `Titel: Bereich: Thema` mit, und hier wird entschieden, ob der Titel wechselt.
//!
//! * **Muster `Bereich: Thema`.** Der Bereich ist der Teil des Produkts, so wie der
//!   Entwickler ihn nennt („Kundenservice“, „Feature-Flags“), nicht das Repo. Ticket und
//!   PR stehen nicht im Titel: die Kopfzeile „B3 · ORPH-4264 · Titel“ zeigt sie schon.
//! * **Bremse.** Geprüft wird nur alle [`TitleRules::every`] getippten Nachrichten, das
//!   erste Mal ab [`TitleRules::first_at`]. Ein neuer Titel gilt erst, wenn
//!   [`TitleRules::confirm`] Prüfungen hintereinander ihn vorschlagen: eine Nebenfrage
//!   ist bei der nächsten Prüfung wieder weg. Damit „derselbe Vorschlag“ wörtlich gleich
//!   ist, bekommt das Modell den Titel und den vorgemerkten Vorschlag zum Abschreiben.
//! * **Bereichsliste.** Damit derselbe Bereich immer gleich heißt, bekommt das Modell die
//!   Bereiche der letzten Wochen (`areas.json` im Cache-Ordner). Es darf jederzeit einen
//!   neuen bilden; die Liste wächst mit jedem gesetzten Titel.
//!
//! Angezeigt wird der Titel über die Statuszeile (`deck-hooks/src/statusline.rs`): sie
//! trägt ihn als `session_name` in `live.json` ein, von dort kommt er auf die Kachel.
//! Claude Code selbst bekommt ihn nicht (`sessionTitle`), sonst stünde er oben rechts
//! im Eingabefeld.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value, json};

use crate::paths::{load_json, save_json};
use crate::text::{char_len, squash_ws, take_chars};

/// Wann geprüft wird und wann ein Wechsel gilt.
#[derive(Debug, Clone, Copy)]
pub struct TitleRules {
    /// Erste Prüfung ab so vielen getippten Nachrichten (davor gilt Claude Codes Titel).
    pub first_at: usize,
    /// Danach alle so viele getippte Nachrichten.
    pub every: usize,
    /// So viele Prüfungen hintereinander müssen denselben neuen Titel vorschlagen.
    pub confirm: u32,
}

impl Default for TitleRules {
    fn default() -> Self {
        TitleRules { first_at: 3, every: 3, confirm: 2 }
    }
}

/// Länger wird kein Titel (Zeichen); zwei Zeilen auf der Karte sind etwa 45.
pub const TITLE_MAX: usize = 60;

/// Stand der Titel-Entscheidung einer Session (Felder im Summary-Cache).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TitleState {
    /// Der geltende Titel des Decks; leer = noch keiner (Claude Codes Titel gilt).
    pub title: String,
    /// Vorgemerkter neuer Titel, noch nicht bestätigt.
    pub cand: String,
    /// So oft wurde `cand` hintereinander vorgeschlagen.
    pub hits: u32,
    /// Bei so vielen getippten Nachrichten war die letzte Prüfung (0 = noch keine).
    pub checked_at: usize,
}

impl TitleState {
    pub fn from_cache(c: &Map<String, Value>) -> TitleState {
        let s = |k: &str| c.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
        let n = |k: &str| c.get(k).and_then(Value::as_u64).unwrap_or(0);
        TitleState { title: s("title"), cand: s("title_cand"), hits: n("title_hits") as u32, checked_at: n("title_checked_at") as usize }
    }

    pub fn fields(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("title", self.title.clone().into()),
            ("title_cand", self.cand.clone().into()),
            ("title_hits", self.hits.into()),
            ("title_checked_at", self.checked_at.into()),
        ]
    }
}

static TICKET_OR_PR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b[A-Z][A-Z0-9]+-\d+\b|\b(?i:PR)\s*#?\d+\b|#\d+\b|\$\d+\b").expect("regex"));

/// Vorschlag des Modells säubern: Anführungszeichen, Markdown, Ticket- und PR-Nummern,
/// Gedankenstriche raus. `None`, wenn danach kein `Bereich: Thema` übrig ist.
pub fn clean(raw: &str) -> Option<String> {
    let t = raw.replace(['`', '*', '"', '„', '“', '”'], "");
    let t = TICKET_OR_PR.replace_all(&t, " ").replace("()", "").replace("( )", "");
    let t = t.replace(" — ", ", ").replace(" – ", ", ").replace(['—', '–'], "-");
    let t = squash_ws(&t);
    let t = t.trim().trim_end_matches(['.', ',', ';']).trim();
    let (area, topic) = t.split_once(':')?;
    let (area, topic) = (area.trim().trim_matches(['·', ',', '-']).trim(), topic.trim().trim_matches(['·', ',', '-']).trim());
    if area.is_empty() || topic.is_empty() || char_len(area) > 30 {
        return None;
    }
    let t = format!("{area}: {topic}");
    Some(if char_len(&t) > TITLE_MAX { format!("{}…", take_chars(&t, TITLE_MAX - 1).trim_end()) } else { t })
}

/// Vergleichsform: Groß/klein und Leerraum zählen nicht.
fn same(a: &str, b: &str) -> bool {
    let n = |s: &str| squash_ws(s).to_lowercase();
    !a.is_empty() && n(a) == n(b)
}

/// Der Bereich eines Titels (vor dem ersten Doppelpunkt).
pub fn area_of(title: &str) -> Option<&str> {
    title.split_once(':').map(|(a, _)| a.trim()).filter(|a| !a.is_empty())
}

/// Neuen Stand ausrechnen. `proposal` ist schon gesäubert (`clean`), `prompts` die Zahl
/// der getippten Nachrichten. Kein Prüfzeitpunkt oder kein Vorschlag: unverändert.
pub fn decide(state: &TitleState, proposal: Option<&str>, prompts: usize, rules: TitleRules) -> TitleState {
    if prompts < rules.first_at || (state.checked_at > 0 && prompts < state.checked_at + rules.every) {
        return state.clone();
    }
    let Some(p) = proposal.filter(|p| !p.is_empty()) else { return state.clone() };
    let mut next = TitleState { checked_at: prompts, ..state.clone() };
    if state.title.is_empty() {
        // Der erste eigene Titel ersetzt den aus der ersten Nachricht sofort.
        next.title = p.to_string();
        next.cand.clear();
        next.hits = 0;
    } else if same(p, &state.title) {
        next.cand.clear();
        next.hits = 0;
    } else {
        next.hits = if same(p, &state.cand) { state.hits + 1 } else { 1 };
        next.cand = p.to_string();
        if next.hits >= rules.confirm {
            next.title = p.to_string();
            next.cand.clear();
            next.hits = 0;
        }
    }
    next
}

/// Zahl der getippten Nachrichten im Transcript (ohne Unteragenten, eingespielte
/// Meldungen und `/compact`-Zusammenfassungen): der Takt der Prüfungen.
pub fn typed_prompts(records: &[Value]) -> usize {
    records
        .iter()
        .filter(|r| r.get("isSidechain").and_then(Value::as_bool) != Some(true))
        .filter_map(crate::claude::summarize::turn_of)
        .filter(|(role, _)| role == "user")
        .count()
}

/// Bereiche, die das Modell vorgeschlagen bekommt: höchstens so viele …
const AREAS_MAX: usize = 40;
/// … und nur aus den letzten so vielen Tagen.
const AREAS_DAYS: f64 = 30.0;

fn areas_path(dir: &Path) -> PathBuf {
    dir.join("areas.json")
}

/// Die Bereiche der letzten Wochen, zuletzt benutzte zuerst.
pub fn recent_areas(dir: &Path, now: f64) -> Vec<String> {
    let Some(Value::Object(m)) = load_json(&areas_path(dir)) else { return Vec::new() };
    let mut v: Vec<(String, f64)> = m
        .into_iter()
        .filter_map(|(k, ts)| ts.as_f64().map(|t| (k, t)))
        .filter(|(_, t)| now - t <= AREAS_DAYS * 86_400.0)
        .collect();
    v.sort_by(|a, b| b.1.total_cmp(&a.1));
    v.into_iter().take(AREAS_MAX).map(|(k, _)| k).collect()
}

/// Bereich eines gesetzten Titels in die Liste eintragen. Ein gleichnamiger Bereich in
/// anderer Schreibweise wird ersetzt, damit die Liste nicht doppelt führt.
pub fn remember_area(dir: &Path, title: &str, now: f64) {
    let Some(area) = area_of(title) else { return };
    let mut m = match load_json(&areas_path(dir)) {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    };
    m.retain(|k, _| !same(k, area));
    m.insert(area.to_string(), json!(now));
    let _ = save_json(&areas_path(dir), &Value::Object(m));
}

/// Der Teil des Lagebild-Prompts, der den Titel verlangt.
pub fn instruction(lang: &str, state: &TitleState, areas: &[String]) -> String {
    let en = lang == "english";
    let mut out = String::from(if en {
        "Then a fifth line:\nTitle: Area: Topic. Area is the part of the product or the tool this is about, the way the developer would name it (for example Customer service, Matrix bot, Feature flags), not the repo name. Topic says what it is about in that area, 2 to 5 words, no work step, no status. At most 45 characters. No ticket or PR numbers, no dashes. What counts is what the chat mainly works on, not the latest side question."
    } else {
        "Danach eine fünfte Zeile:\nTitel: Bereich: Thema. Bereich ist der Teil des Produkts oder das Werkzeug, um das es geht, so wie der Entwickler ihn nennt (zum Beispiel Kundenservice, Matrixbot, Feature-Flags), nicht der Repo-Name. Thema sagt, worum es in diesem Bereich geht, 2 bis 5 Wörter, kein Arbeitsschritt, kein Status. Höchstens 45 Zeichen. Keine Ticket- oder PR-Nummern, keine Gedankenstriche. Maßgeblich ist, woran der Chat hauptsächlich arbeitet, nicht die letzte Nebenfrage."
    });
    if !areas.is_empty() {
        out += &if en {
            format!(" Areas already in use: {}. If one fits, use exactly that name; otherwise form a new one.", areas.join(", "))
        } else {
            format!(" Schon benutzte Bereiche: {}. Passt einer, nimm genau diesen Namen; sonst bilde einen neuen.", areas.join(", "))
        };
    }
    if !state.title.is_empty() {
        out += &if en {
            format!(" Current title: {}. If it still fits, copy it unchanged.", state.title)
        } else {
            format!(" Aktueller Titel: {}. Passt er noch, schreib ihn unverändert ab.", state.title)
        };
    }
    if !state.cand.is_empty() {
        out += &if en {
            format!(" Pending new title: {}. If the chat is still about that, copy it unchanged.", state.cand)
        } else {
            format!(" Vorgemerkter neuer Titel: {}. Geht es immer noch darum, schreib ihn unverändert ab.", state.cand)
        };
    }
    out.push_str("\n\n");
    out
}

#[cfg(test)]
#[path = "title_tests.rs"]
mod tests;
