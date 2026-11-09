//! VS Code komplett neu starten und die Claude-Chats wieder aufnehmen (Python
//! `ops/vscode_restart.py`).
//!
//! Eine Zustandsmaschine, die die App im Takt weiterschiebt ([`Restarter::step`]), keine
//! Schleife mit Warten: ein Ablauf über eine Minute darf die Oberfläche nicht einfrieren.
//! Phasen: speichern → JEDES Agent-Terminal schließen → Fenster schließen → nachsehen,
//! bis kein Code-Prozess mehr läuft → je Fenster `Code.exe --new-window` → warten, bis
//! sich die Extensions melden → je Fenster die Chats der Reihe nach mit `--resume`.
//!
//! Die vier Regeln (CLAUDE.md, Verträge 8), alle hier oder im Plan:
//! 1. Jeder Slot kommt mit `session_id` UND `cwd` zurück (`create_agent(…, cwd)`): ein
//!    Ticket-Agent lief im worktree, mit der Repo-Wurzel fände `--resume` nichts.
//! 2. Eine ID ist kein Beweis: der Plan wird mit [`has_transcript`] gebaut.
//! 3. Ein geschlossenes Fenster ist kein beendeter VS Code: es wird NACHGESEHEN, nicht
//!    gewartet, und eine unlesbare Prozessliste gilt als „läuft noch“.
//! 4. Terminals vor Fenstern schließen: sonst stellt VS Code leere Hüllen mit denselben
//!    Namen wieder her und die Chats landen auf anderen Kacheln.
//!
//! Dazu die Pflicht der App: solange [`Restarter::busy`] gilt, ruhen ihre Aufräumarbeiten
//! (Bindungen vergessen, worktree-Sweep). Der Plan liegt VOR dem ersten Schließen auf der
//! Platte; derselbe Knopf setzt einen abgebrochenen Lauf fort.
//!
//! Sicherheitsnetz: alles, was schließt oder startet, geht über [`EditorBridge`] und
//! [`RestartHost`]. Die echte Fassung des Hosts (`restart_host.rs`) verweigert in Tests.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use deck_core::i18n::Lang;
use deck_core::restart_plan::{Plan, PlanSlot, PlanWindow, counts, is_usable, needs_relaunch};
use serde::Serialize;

pub use crate::restart_host::{PlanFile, SystemRestartHost, foreign_vscode_windows, has_transcript};

/// Was die App über den Broker zur Extension tut. Die App implementiert das.
pub trait EditorBridge: Send + Sync {
    /// „Alle speichern“ im Fenster (sonst öffnet das Schließen einen modalen Dialog).
    fn save_all(&self, window: &str) -> bool;
    fn close_agent(&self, slot: &str) -> bool;
    fn close_window(&self, window: &str) -> bool;
    /// Chat anlegen; `args` ist die fertige Flag-Zeile (`--resume "<id>" …`).
    fn create_agent(&self, window: &str, slot: &str, args: &str, cwd: &str) -> bool;
    /// Hat sich das Fenster gemeldet UND ist es an seinen Buchstaben gebunden?
    fn connected(&self, window: &str) -> bool;
    fn terminals(&self, window: &str) -> Vec<String>;
}

/// Die Umgebung: VS Code finden, Prozesse zählen, starten, eskalieren, Uhr.
pub trait RestartHost: Send + Sync {
    fn find_code_exe(&self) -> Option<PathBuf>;
    /// `None` = konnte nicht nachsehen (nie als „läuft nicht“ deuten).
    fn count_exe(&self, name: &str) -> Option<usize>;
    fn launch(&self, exe: &Path, folder: &str) -> bool;
    /// JEDES offene VS-Code-Hauptfenster per WM_CLOSE fragen; Rückgabe: wie viele.
    fn close_editor_windows(&self) -> usize;
    /// Notbremse: `exe` samt Kindprozessen beenden.
    fn kill_editors(&self, exe: &str) -> bool;
    /// Monotone Uhr in Sekunden.
    fn now(&self) -> f64;
}

/// Wo der Plan liegt (`restart_plan.json`).
pub trait PlanStore: Send + Sync {
    fn load(&self) -> Option<Plan>;
    fn save(&self, plan: &Plan);
    fn clear(&self);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Idle,
    Save,
    Kill,
    Shut,
    Wait,
    Launch,
    Hello,
    Resume,
    Done,
    Failed,
}

impl Phase {
    /// Name wie in der Python-Fassung (`save`, `wait` …), für Meldungen und Protokoll.
    pub fn name(self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Save => "save",
            Phase::Kill => "kill",
            Phase::Shut => "shut",
            Phase::Wait => "wait",
            Phase::Launch => "launch",
            Phase::Hello => "hello",
            Phase::Resume => "resume",
            Phase::Done => "done",
            Phase::Failed => "failed",
        }
    }
}

// Zeiten in Sekunden. Großzügig: jede Zahl ist eine Wette auf einen fremden Rechner,
// und ein zu früher Abbruch ist teurer als ein paar Sekunden Geduld.
pub const SAVE_SETTLE: f64 = 1.5;
pub const KILL_SETTLE: f64 = 1.2;
pub const SHUT_SETTLE: f64 = 2.0;
/// Danach JEDES verbliebene VS-Code-Fenster per WM_CLOSE fragen, auch fremde.
pub const SOFT_CLOSE_S: f64 = 12.0;
/// Danach die Notbremse.
pub const FORCE_KILL_S: f64 = 30.0;
/// Danach aufgeben: der Zustand ist nicht herstellbar.
pub const DEAD_TIMEOUT: f64 = 45.0;
pub const LAUNCH_GAP: f64 = 2.0;
/// VS Code starten und Extension verbinden (kalter Start!).
pub const HELLO_TIMEOUT: f64 = 120.0;
/// Bis ein angeforderter Chat als Terminal auftaucht.
pub const SLOT_TIMEOUT: f64 = 40.0;
/// Danach, bevor im selben Fenster der nächste angelegt wird (Split des vorigen).
pub const SLOT_SETTLE: f64 = 1.5;
pub const TICK: f64 = 0.5;

/// Fortschritt: (Text, `None` läuft / `Some(true)` fertig / `Some(false)` Fehler).
pub type Reporter = Box<dyn FnMut(&str, Option<bool>) + Send>;
/// Zusätzliches Protokoll der App.
pub type RestartLog = Box<dyn Fn(&str) + Send>;

/// Der Ablauf. Lebt so lange wie die App, beschäftigt nur zwischen `start` und Ende.
pub struct Restarter {
    bridge: Box<dyn EditorBridge>,
    host: Box<dyn RestartHost>,
    store: Box<dyn PlanStore>,
    lang: Lang,
    log: Option<RestartLog>,
    phase: Phase,
    plan: Plan,
    report: Option<Reporter>,
    exe: Option<PathBuf>,
    t0: f64,
    at: f64,
    todo: Vec<PlanWindow>,
    pending: BTreeMap<String, Vec<PlanSlot>>,
    inflight: BTreeMap<String, (usize, f64)>,
    ready_at: BTreeMap<String, f64>,
    lost: Vec<String>,
    made: usize,
    esc: u8,
    forced: bool,
}

impl Restarter {
    pub fn new(bridge: Box<dyn EditorBridge>, host: Box<dyn RestartHost>, store: Box<dyn PlanStore>, lang: Lang) -> Restarter {
        Restarter {
            bridge,
            host,
            store,
            lang,
            log: None,
            phase: Phase::Idle,
            plan: Plan::default(),
            report: None,
            exe: None,
            t0: 0.0,
            at: 0.0,
            todo: Vec::new(),
            pending: BTreeMap::new(),
            inflight: BTreeMap::new(),
            ready_at: BTreeMap::new(),
            lost: Vec::new(),
            made: 0,
            esc: 0,
            forced: false,
        }
    }

    /// Jede Meldung zusätzlich ins Protokoll der App.
    pub fn set_log(&mut self, log: RestartLog) {
        self.log = Some(log);
    }

    pub fn set_lang(&mut self, lang: Lang) {
        self.lang = lang;
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Angelegte Chats im laufenden bzw. letzten Lauf.
    pub fn made(&self) -> usize {
        self.made
    }

    /// Läuft ein Neustart? Solange das gilt, MUSS die App ihre Aufräumarbeiten aussetzen.
    pub fn busy(&self) -> bool {
        !matches!(self.phase, Phase::Idle | Phase::Done | Phase::Failed)
    }

    fn l(&self, de: String, en: String) -> String {
        if self.lang.is_english() { en } else { de }
    }

    fn say(&mut self, text: &str, ok: Option<bool>) {
        if let Some(log) = &self.log {
            log(&format!("restart[{}] {text}", self.phase.name()));
        }
        // Ein geschlossener Dialog darf den Lauf nicht abbrechen: wirft der Rückruf, wird
        // er vergessen.
        if let Some(mut r) = self.report.take() {
            let ok_call = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| r(text, ok))).is_ok();
            if ok_call {
                self.report = Some(r);
            }
        }
    }

    fn fail(&mut self, text: &str) {
        self.phase = Phase::Failed;
        self.say(text, Some(false));
    }

    fn enter(&mut self, phase: Phase, delay: f64) {
        self.phase = phase;
        self.t0 = self.host.now();
        self.at = self.t0 + delay;
    }

    fn letters(&self) -> Vec<String> {
        self.plan.windows.iter().map(|w| w.window.clone()).filter(|w| !w.is_empty()).collect()
    }

    /// Einen Lauf beginnen. `snapshot` baut den Plan aus dem Ist-Zustand (die App kennt
    /// Broker, Zustände, Reihenfolge); gefragt wird nur, wenn kein abgebrochener Lauf
    /// fortzusetzen ist. `false` = nichts zu tun (Begründung schon gemeldet).
    pub fn start(&mut self, snapshot: &dyn Fn() -> Plan, report: Option<Reporter>) -> bool {
        if self.busy() {
            // Die Meldung geht an den NEUEN Rückruf; der alte gehört vielleicht einem
            // geschlossenen Dialog, und ein stummer Knopf wird ein drittes Mal gedrückt.
            if let Some(mut r) = report {
                let p = self.phase.name();
                let text = self.l(format!("Ein Neustart läuft bereits ({p})."), format!("A restart is already running ({p})."));
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| r(&text, None)));
            }
            return false;
        }
        self.report = report;
        self.exe = self.host.find_code_exe();
        self.lost.clear();
        self.made = 0;
        self.esc = 0;
        self.forced = false;
        self.pending.clear();
        self.inflight.clear();
        self.ready_at.clear();
        if self.exe.is_none() {
            let t = self.l("VS Code nicht gefunden (Code.exe).".into(), "Could not find VS Code (Code.exe).".into());
            self.fail(&t);
            return false;
        }
        // Liegengebliebener Plan, dessen Fenster alle zu sind: ein früherer Lauf brach nach
        // dem Schließen ab. Fortsetzen statt neu planen (ein frischer Plan wäre jetzt leer
        // und überschriebe den alten).
        if let Some(old) = self.store.load() {
            self.plan = old;
            let live: Vec<String> = self.letters().into_iter().filter(|w| self.bridge.connected(w)).collect();
            if needs_relaunch(&serde_json::to_value(&self.plan).unwrap_or_default(), &live) {
                self.todo = self.plan.windows.clone();
                let n = self.todo.len();
                let t = self.l(format!("Abgebrochener Neustart gefunden, {n} Fenster werden geöffnet."), format!("Found an interrupted restart, reopening {n} window(s)."));
                self.say(&t, None);
                self.enter(Phase::Launch, 0.0);
                return true;
            }
        }
        self.plan = snapshot();
        if !is_usable(&serde_json::to_value(&self.plan).unwrap_or_default()) {
            let t = self.l("Kein verbundenes VS-Code-Fenster mit Ordner.".into(), "No connected VS Code window with a folder.".into());
            self.fail(&t);
            return false;
        }
        self.store.save(&self.plan);
        let (wins, keep, fresh) = counts(&self.plan);
        let mut msg = self.l(format!("{wins} Fenster, {keep} Session(s) gemerkt."), format!("{wins} window(s), remembered {keep} session(s)."));
        if fresh > 0 {
            msg += &self.l(format!(" {fresh} Chat(s) ohne Session starten leer."), format!(" {fresh} chat(s) without a session start empty."));
        }
        if !self.plan.skipped.is_empty() {
            let s = self.plan.skipped.join(", ");
            msg += &self.l(format!(" Ohne Ordner übersprungen: {s}."), format!(" Skipped (no folder): {s}."));
        }
        self.say(&msg, None);
        self.enter(Phase::Save, 0.0);
        true
    }

    /// Einen Schritt tun (aus dem Takt der App). Wirft nie: ein Fehler hier nähme sonst
    /// den Poll mit. (Im Release-Build mit `panic = abort` gilt das nur für Fehler, die
    /// als Rückgabewert kommen; die Traits liefern darum Wahrheitswerte, keine Panics.)
    pub fn step(&mut self) {
        if !self.busy() || self.host.now() < self.at {
            return;
        }
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match self.phase {
            Phase::Save => self.do_save(),
            Phase::Kill => self.do_kill(),
            Phase::Shut => self.do_shut(),
            Phase::Wait => self.do_wait(),
            Phase::Launch => self.do_launch(),
            Phase::Hello => self.do_hello(),
            Phase::Resume => self.do_resume(),
            _ => {}
        }));
        if res.is_err() {
            let t = self.l("Neustart abgebrochen (Fehler im Ablauf, siehe Protokoll).".into(), "Restart aborted (error in the sequence, see log).".into());
            self.fail(&t);
        }
    }
}

#[path = "restart_phases.rs"]
mod phases;

#[cfg(test)]
#[path = "restart_fake.rs"]
mod fake;

#[cfg(test)]
#[path = "restart_tests.rs"]
mod tests;
