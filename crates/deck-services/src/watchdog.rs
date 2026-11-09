//! Der Wächter: startet die App neu, wenn sie verschwunden ist, und protokolliert, WIE
//! sie verschwunden ist (Python `ops/watchdog.py`). Das Programm dazu ist
//! `src/bin/deck-watchdog.rs`; Herzschlag und Dateien siehe [`crate::heartbeat`].
//!
//! Ein Dashboard am Bildschirmrand muss einfach da sein. Bewusst zustandslos: ein Lauf
//! (oder ein Durchgang im Dauerbetrieb `--loop`) tut genau drei Dinge:
//! 1. Lebt die App? (PID aus dem Herzschlag lebt, ist die App-Exe, Herzschlag frisch.)
//! 2. Wenn nicht: aus dem Herzschlag ablesen, wie sie geendet hat, und das protokollieren.
//! 3. Starten, abgekoppelt (`DETACHED_PROCESS | CREATE_BREAKAWAY_FROM_JOB`): sonst hängt
//!    sie an der Prozessgruppe des Aufrufers, und ein Job-Objekt mit KILL_ON_JOB_CLOSE
//!    nähme sie lautlos mit.
//!
//! Nicht gestartet wird bei einem HÄNGENDEN Prozess (lebt, Herzschlag alt): zwei Decks
//! sind schlimmer als ein hängendes. Ebenso nicht, wenn die Prozessliste unlesbar ist,
//! und nicht nach einem sauberen Ende (der Nutzer hat sie geschlossen).

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use deck_win::effects::Processes;

use crate::fsutil::basename;
use crate::guard;
use crate::heartbeat::{BEAT_FRESH_S, Heartbeat, WATCHDOG_LOCK_FILE, WATCHDOG_LOG_FILE};
use crate::logfile::{LogFile, Rotate};

/// Befund, nach dem der Wächter absichtlich NICHTS tut.
pub const CLEAN_END: &str = "sauberes Ende";
/// So lange nach „keine App da“ noch einmal hinsehen: sie kann sich gerade selbst neu
/// starten; ohne Pause käme in genau dieses Fenster ein zweites Deck.
pub const RECHECK: Duration = Duration::from_secs(6);
/// Takt des Dauerbetriebs.
pub const LOOP_EVERY: Duration = Duration::from_secs(60);
/// Name der App neben dem Wächter (Rückfall, wenn kein Herzschlag sie nennt).
pub const APP_EXE: &str = "agent-deck.exe";

const DETACHED_PROCESS: u32 = 0x0000_0008;
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;

/// Was der Blick auf einen Prozess ergibt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    /// Lebt; Exe-Name ohne Pfad.
    Alive(String),
    Dead,
    /// Prozessliste nicht lesbar.
    Unknown,
}

/// Nachsehen, welcher Prozess hinter einer PID steckt.
pub trait ProcessProbe: Send + Sync {
    fn probe(&self, pid: u32) -> Probe;
}

impl<P: Processes + ?Sized> ProcessProbe for P {
    fn probe(&self, pid: u32) -> Probe {
        match self.list() {
            None => Probe::Unknown,
            Some(list) => list.into_iter().find(|p| p.pid == pid).map_or(Probe::Dead, |p| Probe::Alive(p.exe)),
        }
    }
}

/// Wie steht es um die App?
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppState {
    Running { pid: u32 },
    /// Kein App-Prozess mehr da: der Wächter darf starten.
    Gone { hint: String },
    /// Prozess lebt, Herzschlag alt: NICHT starten, nur vermerken.
    Hanging { pid: u32, hint: String },
    /// Prozessliste nicht lesbar: „weiß nicht“ ist kein „weg“.
    Unknown,
}

fn same_exe(a: &str, b: &str) -> bool {
    basename(a).eq_ignore_ascii_case(basename(b))
}

/// Zustand aus Herzschlag und Prozessliste. Steckt hinter der PID inzwischen ein anderes
/// Programm (PIDs werden neu vergeben), ist die App in Wahrheit weg.
pub fn app_state(beat: Option<&Heartbeat>, probe: &dyn ProcessProbe, now: f64) -> AppState {
    let Some(b) = beat.filter(|b| b.pid != 0) else { return AppState::Gone { hint: String::new() } };
    match probe.probe(b.pid) {
        Probe::Unknown => AppState::Unknown,
        Probe::Dead => AppState::Gone { hint: String::new() },
        Probe::Alive(exe) if !same_exe(&exe, &b.exe) => {
            AppState::Gone { hint: format!("PID {} wurde an einen Fremdprozess vergeben ({exe})", b.pid) }
        }
        Probe::Alive(_) if now - b.ts <= BEAT_FRESH_S => AppState::Running { pid: b.pid },
        Probe::Alive(_) => AppState::Hanging { pid: b.pid, hint: format!("Prozess {} lebt, Lebenszeichen: {} s alt", b.pid, (now - b.ts) as i64) },
    }
}

/// Wie hat der letzte Lauf geendet? Trennt das geordnete Ende vom Abschuss von außen,
/// der wichtigste Unterschied bei der Suche.
pub fn last_end(beat: Option<&Heartbeat>) -> String {
    match beat.map(|b| b.ended.as_deref()) {
        None => "kein Herzschlag vorhanden (erster Lauf?)".into(),
        Some(Some("exit")) => format!("{CLEAN_END} (App hat sich selbst beendet: geschlossen)"),
        Some(Some("restart")) => "Selbst-Neustart, aber kein Kind da: Neustart ist fehlgeschlagen".into(),
        Some(Some("panic")) => "unbehandelter Fehler (panic), Protokoll der App ansehen".into(),
        Some(Some(other)) => format!("unbekanntes Ende ({other})"),
        Some(None) => "ABGESCHOSSEN oder hart abgestürzt: kein Ende vermerkt (TerminateProcess von außen, \
                       Job-Objekt, Abmeldung)"
            .into(),
    }
}

/// Startet die App.
pub trait Launcher: Send + Sync {
    fn start_detached(&self, exe: &Path) -> bool;
}

/// Die echte Fassung: abgekoppelt vom Job des Aufrufers; ist Breakaway nicht erlaubt
/// (Job ohne BREAKAWAY_OK), ohne dieses Flag: lieber angebunden als gar nicht.
#[derive(Debug, Default, Clone, Copy)]
pub struct DetachedLauncher;

impl Launcher for DetachedLauncher {
    fn start_detached(&self, exe: &Path) -> bool {
        if guard::check("App starten").is_err() {
            return false;
        }
        let dir = exe.parent().unwrap_or(Path::new("."));
        let flags = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
        [flags | CREATE_BREAKAWAY_FROM_JOB, flags]
            .into_iter()
            .any(|f| Command::new(exe).current_dir(dir).creation_flags(f).spawn().is_ok())
    }
}

/// Ein Wächter mit seinen Abhängigkeiten.
pub struct Watchdog {
    pub app_dir: PathBuf,
    /// Ausdrücklich genannte App (`--app`); sonst die aus dem Herzschlag, sonst
    /// `agent-deck.exe` neben dem Wächter.
    pub app_exe: Option<PathBuf>,
    pub probe: Arc<dyn ProcessProbe>,
    pub launcher: Arc<dyn Launcher>,
    pub clock: Arc<dyn Fn() -> f64 + Send + Sync>,
    pub sleep: Arc<dyn Fn(Duration) + Send + Sync>,
    pub log: LogFile,
}

impl Watchdog {
    pub fn system(app_dir: PathBuf, app_exe: Option<PathBuf>) -> Watchdog {
        let log = LogFile::new(app_dir.join(WATCHDOG_LOG_FILE), 512 * 1024, Rotate::Rename);
        Watchdog {
            app_dir,
            app_exe,
            probe: Arc::new(deck_win::effects::Win32Processes),
            launcher: Arc::new(DetachedLauncher),
            clock: Arc::new(crate::fsutil::now_unix),
            sleep: Arc::new(std::thread::sleep),
            log,
        }
    }

    fn state(&self) -> (AppState, Option<Heartbeat>) {
        let beat = crate::heartbeat::read(&self.app_dir);
        (app_state(beat.as_ref(), self.probe.as_ref(), (self.clock)()), beat)
    }

    /// Welche Exe gestartet wird.
    pub fn target(&self, beat: Option<&Heartbeat>) -> Option<PathBuf> {
        if let Some(p) = &self.app_exe {
            return Some(p.clone());
        }
        if let Some(p) = beat.map(|b| PathBuf::from(&b.exe)).filter(|p| p.is_file()) {
            return Some(p);
        }
        std::env::current_exe().ok()?.parent().map(|d| d.join(APP_EXE)).filter(|p| p.is_file())
    }

    /// Ein Durchgang: prüfen und ggf. starten. 0 = alles gut bzw. gestartet.
    pub fn once(&self) -> i32 {
        match self.state().0 {
            AppState::Running { .. } => return 0,
            AppState::Hanging { hint, .. } => {
                self.log.note(&format!("Wächter: App hängt: {hint}. Es wird KEINE zweite gestartet."));
                return 1;
            }
            AppState::Unknown => {
                self.log.note("Wächter: Prozessliste nicht lesbar, nichts gestartet.");
                return 1;
            }
            AppState::Gone { .. } => {}
        }
        let finding = last_end(crate::heartbeat::read(&self.app_dir).as_ref());
        if finding.starts_with(CLEAN_END) {
            return 0; // bewusst geschlossen: in Ruhe lassen
        }
        (self.sleep)(RECHECK); // läuft gerade ein Selbst-Neustart?
        let (state, beat) = self.state();
        if !matches!(state, AppState::Gone { .. }) {
            return 0;
        }
        self.log.note(&format!("Wächter: keine App da. Vorgänger-Ende: {finding}"));
        let Some(exe) = self.target(beat.as_ref()) else {
            self.log.note("Wächter: App-Programm nicht gefunden, nichts gestartet.");
            return 1;
        };
        let ok = self.launcher.start_detached(&exe);
        self.log.note(&if ok { format!("Wächter: App neu gestartet ({}).", exe.display()) } else { "Wächter: Start FEHLGESCHLAGEN.".into() });
        if ok { 0 } else { 1 }
    }

    /// Nur berichten (`--status`): (Exit-Code, Text).
    pub fn status(&self) -> (i32, String) {
        let (state, beat) = self.state();
        match state {
            AppState::Running { pid } => (0, format!("App läuft (PID {pid}).")),
            AppState::Hanging { hint, .. } => (1, format!("App hängt: {hint}. Ein Wächter-Lauf würde KEINE zweite starten.")),
            AppState::Unknown => (1, "Prozessliste nicht lesbar.".into()),
            AppState::Gone { hint } => {
                let extra = if hint.is_empty() { String::new() } else { format!(" [{hint}]") };
                (1, format!("App läuft NICHT. Vorgänger: {}{extra}", last_end(beat.as_ref())))
            }
        }
    }

    /// Bin ich der einzige Dauerwächter? Übernimmt das Lock, wenn der Eintrag tot ist oder
    /// die PID inzwischen einem anderen Programm gehört.
    pub fn loop_lock(&self) -> bool {
        let path = self.app_dir.join(WATCHDOG_LOCK_FILE);
        let me = std::process::id();
        let other: u32 = std::fs::read_to_string(&path).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
        let my_exe = std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        if other != 0 && other != me && matches!(self.probe.probe(other), Probe::Alive(exe) if same_exe(&exe, &my_exe)) {
            return false;
        }
        // Best effort: lieber wachen als gar nicht wachen.
        let _ = deck_core::paths::atomic_write(&path, me.to_string().as_bytes());
        true
    }

    /// Dauerbetrieb: im Takt nachsehen. Ein Fehler in einem Durchgang beendet nie das Wachen.
    pub fn run_loop(&self, every: Duration) -> i32 {
        if !self.loop_lock() {
            return 0;
        }
        self.log.note(&format!("Wächter: Dauerbetrieb gestartet (PID {}, Takt {} s).", std::process::id(), every.as_secs()));
        loop {
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.once())).is_err() {
                self.log.note("Wächter: Fehler im Durchgang (weiter im Takt).");
            }
            (self.sleep)(every);
        }
    }
}

#[cfg(test)]
#[path = "watchdog_tests.rs"]
mod tests;
