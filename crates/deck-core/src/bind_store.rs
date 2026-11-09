//! Persistenz rund um die Fenster-Bindung: welches Fenster (A, B, C …) auf welches Repo
//! zeigt (bindings.json), welches Effort je Slot zuletzt gesetzt war (slot_effort.json),
//! zugewiesene Tickets, Panel-Einstellungen und die gezogenen Reihenfolgen.
//!
//! Alle Dateien werden selbstheilend geladen (kaputt/fehlt → leer, Müll-Einträge
//! fliegen raus) und atomar geschrieben. Ein Schreibfehler ist nicht tödlich: das Deck
//! läuft weiter, nur überlebt die Änderung den nächsten Start nicht.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::binding::is_placeholder_ws;
use crate::paths::{
    BINDINGS_FILE, EFFORT_FILE, ORDER_FILE, SETTINGS_FILE, TICKETS_FILE, WIN_ORDER_FILE, load_json, save_json,
};

/// Die Laufzeitdaten der Bindung. Die Felder mutiert der Aufrufer direkt und ruft danach
/// das passende `save_*`.
#[derive(Debug, Clone, Default)]
pub struct BindStore {
    dir: PathBuf,
    /// Fenster-Buchstabe → Repo-Name.
    pub bindings: BTreeMap<String, String>,
    /// Slot → gemerktes Effort. Nur so ist ultracode von xhigh unterscheidbar: die
    /// statusLine meldet für beide `xhigh`.
    pub effort: BTreeMap<String, String>,
    /// Slot → zugewiesene Ticket-ID (Anzeige auf der Karte).
    pub tickets: BTreeMap<String, String>,
    /// Panel-weite Einstellungen (`terminal_home`, `jira_prefix`, `jump_hotkey` …).
    pub settings: Map<String, Value>,
    /// Fenster-Buchstabe → vom Nutzer gezogene Kachel-Reihenfolge.
    pub order: BTreeMap<String, Vec<String>>,
    /// Gezogene Reihenfolge der Repo-Blöcke, als REPO-Namen (der Buchstabe wird beim
    /// Schließen frei und neu vergeben). Darf mehr Repos nennen, als offen sind.
    pub win_order: Vec<String>,
}

impl BindStore {
    /// Alle Dateien aus `dir` (dem App-Ordner) laden.
    pub fn load(dir: &Path) -> BindStore {
        let mut s = BindStore { dir: dir.to_path_buf(), ..Default::default() };
        s.bindings = s.load_bindings();
        s.effort = string_map(load_json(&s.path(EFFORT_FILE)), |v| v.as_str().map(String::from));
        s.tickets = string_map(load_json(&s.path(TICKETS_FILE)), |v| {
            v.as_str().filter(|t| !t.trim().is_empty()).map(String::from)
        });
        s.settings = match load_json(&s.path(SETTINGS_FILE)) {
            Some(Value::Object(m)) => m,
            _ => Map::new(),
        };
        s.order = s.load_order();
        s.win_order = s.load_win_order();
        s
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn save(&self, name: &str, value: Value) {
        let _ = save_json(&self.path(name), &value);
    }

    /// Erst-Start, kaputt oder kein Objekt → leer. Platzhalter-
    /// Bindungen (leer, `unknown`) fliegen beim Laden raus, und die bereinigte Datei wird
    /// zurückgeschrieben: ein altes Phantom ist nach dem Neustart weg.
    fn load_bindings(&self) -> BTreeMap<String, String> {
        let raw: Vec<(String, Value)> = match load_json(&self.path(BINDINGS_FILE)) {
            Some(Value::Object(m)) => m.into_iter().collect(),
            _ => Vec::new(),
        };
        let total = raw.len();
        let clean: BTreeMap<String, String> = raw
            .into_iter()
            .filter_map(|(w, v)| match v {
                Value::String(s) if !is_placeholder_ws(Some(&s)) => Some((w, s)),
                _ => None,
            })
            .collect();
        if clean.len() != total {
            self.save(BINDINGS_FILE, to_object(&clean));
        }
        clean
    }

    /// Selbstheilend: nur Buchstaben mit einer Liste, darin nur nicht-leere Texte.
    fn load_order(&self) -> BTreeMap<String, Vec<String>> {
        let Some(Value::Object(m)) = load_json(&self.path(ORDER_FILE)) else {
            return BTreeMap::new();
        };
        m.into_iter()
            .filter_map(|(w, seq)| match seq {
                Value::Array(a) => Some((
                    w,
                    a.into_iter().filter_map(|s| s.as_str().filter(|s| !s.is_empty()).map(String::from)).collect(),
                )),
                _ => None,
            })
            .collect()
    }

    /// Selbstheilend: nur nicht-leere Texte, Doppelte raus.
    fn load_win_order(&self) -> Vec<String> {
        let Some(Value::Array(a)) = load_json(&self.path(WIN_ORDER_FILE)) else {
            return Vec::new();
        };
        let mut out: Vec<String> = Vec::new();
        for w in a {
            if let Value::String(s) = w
                && !s.is_empty()
                && !out.contains(&s)
            {
                out.push(s);
            }
        }
        out
    }

    pub fn save_bindings(&self) {
        self.save(BINDINGS_FILE, to_object(&self.bindings));
    }

    pub fn save_effort(&self) {
        self.save(EFFORT_FILE, to_object(&self.effort));
    }

    pub fn save_tickets(&self) {
        self.save(TICKETS_FILE, to_object(&self.tickets));
    }

    pub fn save_settings(&self) {
        self.save(SETTINGS_FILE, Value::Object(self.settings.clone()));
    }

    pub fn save_order(&self) {
        let m: Map<String, Value> = self
            .order
            .iter()
            .map(|(w, seq)| (w.clone(), Value::from(seq.clone())))
            .collect();
        self.save(ORDER_FILE, Value::Object(m));
    }

    pub fn save_win_order(&self) {
        self.save(WIN_ORDER_FILE, Value::from(self.win_order.clone()));
    }
}

fn string_map(raw: Option<Value>, keep: impl Fn(&Value) -> Option<String>) -> BTreeMap<String, String> {
    match raw {
        Some(Value::Object(m)) => m.into_iter().filter_map(|(k, v)| keep(&v).map(|s| (k, s))).collect(),
        _ => BTreeMap::new(),
    }
}

fn to_object(m: &BTreeMap<String, String>) -> Value {
    Value::Object(m.iter().map(|(k, v)| (k.clone(), Value::from(v.clone()))).collect())
}
