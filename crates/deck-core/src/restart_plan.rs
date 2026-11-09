//! Der Neustart-Plan: was nach dem Beenden von VS Code wieder aufgebaut wird.
//!
//! „VS Code neu starten“ schließt alle Fenster und öffnet sie wieder, und danach soll
//! jede Kachel ihren Chat zurückhaben. Möglich, weil der report-Hook je Slot
//! `session_id` UND `cwd` schreibt und `claude --resume <id>` genau diese Unterhaltung
//! wieder aufnimmt. Hier steht nur die Rechnung: aus den verbundenen Fenstern und den
//! Zustandsdateien eine Liste von Fenstern mit Ordner und darin Slots mit fertiger
//! Kommandozeile. Kein Fenster wird geschlossen, kein Prozess gestartet.
//!
//! Der Plan wird geschrieben, BEVOR das erste Fenster zugeht: stirbt die App mitten im
//! Lauf, setzt derselbe Knopf ihn fort (`needs_relaunch`).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::binding::{repo_from_title, strip_remote_tag, window_from_title};
use crate::ordering::apply_order;
use crate::paths::{load_json, save_json};
use crate::pystr::{strip, truthy};
use crate::slot_state::SlotState;

/// Eine Session-ID ist eine UUID. Keine Kosmetik: der Wert landet ungeschützt in einer
/// Shell-Zeile (`claude --resume "<id>"`), und eine Datei ist kein Beweis.
static UUID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$").expect("UUID")
});
/// Modellnamen wie `opus[1m]`.
static MODEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9._\-\[\]]+$").expect("MODEL"));
/// Ein Ordner ist ein Pfad, eine Gegenstelle eine Adresse mit Schema vor `//`.
static URI: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^[a-z][a-z0-9+.-]*://").expect("URI"));

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
    UUID.is_match(strip(sid))
}

/// Die Flag-Zeile für `claude`, fertig zitiert, in fester Reihenfolge. Was die Prüfungen
/// nicht besteht, fällt still weg: ein Agent ohne sein Modell ist ärgerlich, eine
/// Zeichenkette, die eine Shell-Zeile aufbricht, ist ein Loch.
pub fn flags(session_id: Option<&str>, model: Option<&str>, effort: Option<&str>, mode: Option<&str>) -> String {
    let mut parts = Vec::new();
    if let Some(sid) = session_id.filter(|s| valid_session(s)) {
        parts.push(format!("--resume \"{}\"", strip(sid)));
    }
    if let Some(m) = model.map(strip).filter(|m| MODEL.is_match(m)) {
        parts.push(format!("--model \"{m}\""));
    }
    if let Some(e) = effort {
        let lvl = strip(e).to_lowercase();
        let lvl = if lvl == "ultracode" { "xhigh".to_string() } else { lvl };
        if EFFORTS.contains(&lvl.as_str()) {
            parts.push(format!("--effort {lvl}"));
        }
    }
    if let Some(cli) = mode.and_then(|m| cli_mode(&strip(m).to_lowercase())) {
        parts.push(format!("--permission-mode {cli}"));
    }
    parts.join(" ")
}

/// Ein Slot im Plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PlanSlot {
    pub slot: String,
    /// Nötig, nicht bequem: Claude findet eine Session über ihr Projektverzeichnis, und
    /// bei einem Ticket-Agenten ist das der worktree.
    pub cwd: String,
    pub args: String,
    /// Keine Session zum Aufnehmen: der Slot kommt als frischer Agent zurück.
    pub fresh: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PlanWindow {
    pub window: String,
    pub workspace: String,
    pub folder: String,
    pub slots: Vec<PlanSlot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Plan {
    pub ts: f64,
    pub windows: Vec<PlanWindow>,
    pub skipped: Vec<String>,
}

/// Was der Broker über ein verbundenes Fenster weiß.
#[derive(Debug, Clone, Default)]
pub struct ClientInfo {
    pub window: Option<String>,
    pub workspace: Option<String>,
    pub folder: Option<String>,
    pub slots: Vec<String>,
}

/// Prüft, ob es zu (Session-ID, cwd) noch ein Transcript gibt. Eine ID allein ist kein
/// Beweis: `--resume` auf eine verschwundene Unterhaltung startet Claude gar nicht.
pub type CanResume<'a> = &'a dyn Fn(&str, &str) -> bool;

fn text(st: &SlotState, key: &str) -> Option<String> {
    st.get(key).and_then(Value::as_str).map(String::from)
}

/// Ein Slot im Plan: Name, Arbeitsverzeichnis, Flag-Zeile.
pub fn slot_entry(
    slot: &str,
    state: Option<&SlotState>,
    model: Option<&str>,
    effort: Option<&str>,
    can_resume: Option<CanResume>,
) -> PlanSlot {
    let empty = SlotState::new();
    let st = state.unwrap_or(&empty);
    let mut sid = text(st, "session_id");
    let cwd = st.get("cwd").filter(|v| truthy(v)).and_then(Value::as_str).unwrap_or("").to_string();
    if let (Some(s), Some(check)) = (&sid, can_resume)
        && valid_session(s)
        && !check(s, &cwd)
    {
        sid = None;
    }
    let eff = effort.filter(|e| !e.is_empty()).map(String::from).or_else(|| text(st, "effort"));
    PlanSlot {
        slot: slot.into(),
        args: flags(sid.as_deref(), model, eff.as_deref(), text(st, "mode").as_deref()),
        fresh: !sid.as_deref().is_some_and(valid_session),
        cwd,
    }
}

/// Aus dem Ist-Zustand einen Plan machen. Die Reihenfolge zählt: die Extension vergibt
/// Slot-Namen fortlaufend, die Chats müssen also in genau der gezogenen Reihenfolge
/// wieder entstehen. Fenster OHNE Ordner landen in `skipped` (ein leeres VS-Code-Fenster
/// kann man nicht wieder aufmachen).
pub fn build(
    clients: &[ClientInfo],
    states: &HashMap<String, SlotState>,
    order: &BTreeMap<String, Vec<String>>,
    model: Option<&str>,
    slot_effort: &BTreeMap<String, String>,
    can_resume: Option<CanResume>,
    now: f64,
) -> Plan {
    let mut plan = Plan { ts: now, ..Default::default() };
    for cl in clients {
        let win = strip(cl.window.as_deref().unwrap_or("")).to_string();
        let folder = strip(cl.folder.as_deref().unwrap_or("")).to_string();
        if win.is_empty() {
            continue; // verbunden, aber ohne Buchstaben → keine Kachel
        }
        if folder.is_empty() {
            plan.skipped.push(win);
            continue;
        }
        let saved = order.get(&win).map(Vec::as_slice).unwrap_or(&[]);
        let slots = apply_order(saved, &cl.slots)
            .iter()
            .map(|s| slot_entry(s, states.get(s), model, slot_effort.get(s).map(String::as_str), can_resume))
            .collect();
        plan.windows.push(PlanWindow { window: win, workspace: cl.workspace.clone().unwrap_or_default(), folder, slots });
    }
    plan
}

/// Argumente für Code.exe. Ein Remote-Fenster öffnet VS Code nur über `--folder-uri`;
/// als nackter Parameter würde die Adresse als DATEINAME gelesen (ein Fehler, der wie
/// ein Erfolg aussieht).
pub fn open_args(folder: &str) -> Vec<String> {
    let f = strip(folder).to_string();
    if URI.is_match(&f) {
        vec!["--new-window".into(), "--folder-uri".into(), f]
    } else {
        vec!["--new-window".into(), f]
    }
}

/// Offene VS-Code-Fenster, die das Deck NICHT kennt, benannt nach ihrem Repo. Ein
/// einziges fremdes Fenster hält VS Code am Leben, und der Neustart hätte alle Chats
/// zugemacht, ohne einen wieder aufmachen zu können. Verglichen ohne Groß-/Klein-
/// schreibung und ohne Remote-Anhängsel, gemeldet MIT.
pub fn foreign_repos<S: AsRef<str>, K: AsRef<str>>(titles: &[S], known: &[K]) -> Vec<String> {
    let have: HashSet<String> = known
        .iter()
        .map(|k| k.as_ref())
        .filter(|k| !k.is_empty())
        .map(|k| strip(&strip_remote_tag(k)).to_lowercase())
        .collect();
    let mut out: Vec<String> = Vec::new();
    for title in titles {
        let low = strip(&repo_from_title(title.as_ref())).to_lowercase();
        if low.is_empty() || have.contains(&low) || out.iter().any(|r| strip_remote_tag(r).to_lowercase() == low) {
            continue;
        }
        out.push(window_from_title(title.as_ref()));
    }
    out
}

/// (Fenster, Slots mit Session, Slots ohne) für die Rückmeldung.
pub fn counts(plan: &Plan) -> (usize, usize, usize) {
    let slots: Vec<&PlanSlot> = plan.windows.iter().flat_map(|w| &w.slots).collect();
    let fresh = slots.iter().filter(|s| s.fresh).count();
    (plan.windows.len(), slots.len() - fresh, fresh)
}

/// Taugt das als Plan? (Ein Objekt mit mindestens einem Fenster samt Ordner.)
pub fn is_usable(plan: &Value) -> bool {
    let Some(wins) = plan.get("windows").and_then(Value::as_array) else { return false };
    wins.iter().any(|w| w.is_object() && w.get("folder").is_some_and(truthy) && w.get("window").is_some_and(truthy))
}

/// Ein liegengebliebener Plan, dessen Fenster ALLE zu sind: ein früherer Lauf ist
/// zwischen „geschlossen“ und „wieder da“ abgebrochen, der Knopf soll fortsetzen. Ist
/// eines wieder verbunden, gilt der Lauf als erledigt.
pub fn needs_relaunch<S: AsRef<str>>(plan: &Value, connected: &[S]) -> bool {
    if !is_usable(plan) {
        return false;
    }
    let live: HashSet<String> = connected.iter().map(|w| w.as_ref().to_uppercase()).collect();
    let wins = plan.get("windows").and_then(Value::as_array).cloned().unwrap_or_default();
    !wins.iter().filter(|w| w.is_object()).any(|w| {
        let name = w.get("window").and_then(Value::as_str).unwrap_or("");
        live.contains(&name.to_uppercase())
    })
}

/// Atomar ablegen. Fehler sind nicht tödlich: nur das Fortsetzen ginge verloren.
pub fn save(plan: &Plan, path: &Path) {
    if let Ok(v) = serde_json::to_value(plan) {
        let _ = save_json(path, &v);
    }
}

/// Liegengebliebenen Plan lesen; fehlt, kaputt oder unbrauchbar → `None`.
pub fn load(path: &Path) -> Option<Plan> {
    let raw = load_json(path)?;
    if !is_usable(&raw) {
        return None;
    }
    serde_json::from_value(raw).ok()
}

/// Plan-Datei entfernen (nach einem geglückten Lauf); fehlt sie: still ok.
pub fn clear(path: &Path) {
    let _ = std::fs::remove_file(path);
}
