//! „Läuft noch ohne Fenster“ als Dienst: Schnappschuss der Prozesse, Merker
//! fortschreiben und auf der Platte halten, Waisen gezielt beenden und nachprüfen.
//!
//! Die Regeln (wer gehört zu welchem Agenten, was ist geschützt, was trifft „Beenden“)
//! stehen in `deck_core::orphans` und sind dort getestet. Hier ist nur die Verdrahtung;
//! alles, was das System liest oder verändert, geht über [`ProcSource`]. Die App ruft
//! [`OrphanWatch::scan`] in einem eigenen Thread (alle 2 s und sofort, wenn eine
//! Agenten-Shell endet, siehe [`OrphanWatch::wait`]), nie im 400-ms-Takt der Oberfläche.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use deck_core::i18n::Lang;
use deck_core::orphans::{
    KillState, Key, ORPHANS_FILE, ProcDetails, ProcRow, Saved, ShellId, Tracker, kill_order, orphan_rows, parse_row_id, roots,
};
use deck_core::paths::{atomic_write, load_json};
use deck_core::view::OrphanView;
use deck_core::wt;

/// Auch ohne Änderung so oft (s) speichern, solange Agenten leben: der letzte Blick auf
/// eine Shell ist nach einem Neustart des Decks die beste Schätzung fürs Schließen.
const SAVE_EVERY_S: f64 = 60.0;
/// Fehlertext von [`ProcSource::terminate`] für „kein Zugriff“; die Zeile übersetzt ihn.
pub const ACCESS_DENIED: &str = "access-denied";
/// So lange (s) wartet „Beenden“ darauf, dass alle Prozesse weg und die Ports frei sind.
const VERIFY_S: f64 = 3.0;

/// Was der Dienst vom System braucht. Tests reichen eine Attrappe herein; die echte
/// Fassung ist [`Win32Source`].
pub trait ProcSource: Send + Sync {
    /// Alle Prozesse; `None` = nicht lesbar (dann bleibt der Merker, wie er ist).
    fn snapshot(&self) -> Option<Vec<ProcRow>>;
    /// Kommandozeile und Sitzung; `None` = nicht mehr derselbe Prozess.
    fn details(&self, pid: u32, start: u64) -> Option<ProcDetails>;
    /// `(PID, Port)` aller lauschenden TCP-Sockets.
    fn listening(&self) -> Option<Vec<(u32, u16)>>;
    /// Genau diesen Prozess beenden. `Ok(true)` = beendet, `Ok(false)` = lief schon nicht mehr.
    fn terminate(&self, pid: u32, start: u64) -> Result<bool, String>;
    /// Läuft genau dieser Prozess noch?
    fn running(&self, pid: u32, start: u64) -> bool;
    /// Warten, bis einer dieser Prozesse endet (`true`) oder die Zeit um ist (`false`).
    fn wait_any_exit(&self, procs: &[Key], timeout: Duration) -> bool;
}

/// Die echte Fassung über `deck_win`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Win32Source;

impl ProcSource for Win32Source {
    fn snapshot(&self) -> Option<Vec<ProcRow>> {
        let list = deck_win::procs::processes()?;
        Some(list.into_iter().map(|p| ProcRow { pid: p.pid, ppid: p.parent_pid, exe: p.exe, start: p.start }).collect())
    }
    fn details(&self, pid: u32, start: u64) -> Option<ProcDetails> {
        let d = deck_win::proc_scan::details(pid, start)?;
        Some(ProcDetails { cmdline: d.cmdline, image: d.image, session: d.session })
    }
    fn listening(&self) -> Option<Vec<(u32, u16)>> {
        deck_win::tcp_listen::listening()
    }
    fn terminate(&self, pid: u32, start: u64) -> Result<bool, String> {
        match deck_win::proc_scan::terminate_exact(pid, start) {
            Ok(t) => Ok(t == deck_win::proc_scan::Terminated::Killed),
            Err(e) if deck_win::proc_scan::is_access_denied(&e) => Err(ACCESS_DENIED.into()),
            Err(e) => Err(e.to_string()),
        }
    }
    fn running(&self, pid: u32, start: u64) -> bool {
        deck_win::procs::process_start(pid) == Some(start)
    }
    fn wait_any_exit(&self, procs: &[Key], timeout: Duration) -> bool {
        deck_win::proc_scan::wait_any_exit(procs, timeout.as_millis().min(u128::from(u32::MAX)) as u32)
    }
}

struct Inner {
    tracker: Tracker,
    kills: KillState,
    /// Lauschende Ports je PID, nur gelesen, solange es Waisen gibt.
    ports: HashMap<u32, Vec<u16>>,
    last_save: f64,
}

/// Der Dienst. Teilbar zwischen Scan-Thread, Takt und Befehlen.
pub struct OrphanWatch {
    src: Arc<dyn ProcSource>,
    state_dir: PathBuf,
    file: PathBuf,
    inner: Mutex<Inner>,
}

/// Die Agenten laut `.wtpid`-Dateien im State-Ordner.
pub fn read_shells(state_dir: &Path) -> Vec<ShellId> {
    let Ok(rd) = fs::read_dir(state_dir) else { return Vec::new() };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let Ok(name) = e.file_name().into_string() else { continue };
        let Some(slot) = name.strip_suffix(wt::PID_SUFFIX) else { continue };
        if let Some((pid, created)) = load_json(&e.path()).as_ref().and_then(wt::parse_pidfile) {
            out.push(ShellId { slot: slot.to_string(), pid, created });
        }
    }
    out
}

impl OrphanWatch {
    /// `app_dir` hält `orphans.json`, `state_dir` die `.wtpid`-Dateien.
    pub fn new(src: Arc<dyn ProcSource>, app_dir: &Path, state_dir: &Path) -> OrphanWatch {
        let file = app_dir.join(ORPHANS_FILE);
        let saved = fs::read(&file).map(|b| Saved::parse(&b)).unwrap_or_default();
        OrphanWatch {
            src,
            state_dir: state_dir.to_path_buf(),
            file,
            inner: Mutex::new(Inner { tracker: Tracker::from_saved(saved), kills: KillState::default(), ports: HashMap::new(), last_save: 0.0 }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Einmal hinschauen. `true` = es gibt etwas Neues für die Oberfläche.
    pub fn scan(&self, now: f64) -> bool {
        let shells = read_shells(&self.state_dir);
        let Some(procs) = self.src.snapshot() else { return false };
        let mut g = self.lock();
        let before = (g.tracker.orphans().len(), g.ports.clone());
        let src = self.src.clone();
        let changed = g.tracker.observe(now, &shells, &procs, &mut |pid, start| src.details(pid, start));
        g.ports = if g.tracker.orphans().is_empty() { HashMap::new() } else { self.ports() };
        // Fehler einer Zeile, die es nicht mehr gibt, vergessen.
        let live: Vec<Key> = roots(&g.tracker);
        g.kills.errors.retain(|k, _| live.contains(k));
        let has_agents = !g.tracker.live_shells().is_empty();
        if changed || (has_agents && now - g.last_save >= SAVE_EVERY_S) {
            let _ = atomic_write(&self.file, &g.tracker.to_saved(now).to_bytes());
            g.last_save = now;
        }
        changed || before != (g.tracker.orphans().len(), g.ports.clone())
    }

    fn ports(&self) -> HashMap<u32, Vec<u16>> {
        let mut out: HashMap<u32, Vec<u16>> = HashMap::new();
        for (pid, port) in self.src.listening().unwrap_or_default() {
            out.entry(pid).or_default().push(port);
        }
        out
    }

    /// Bis zu `timeout` warten; endet vorher eine Agenten-Shell, sofort zurück.
    pub fn wait(&self, timeout: Duration) {
        let shells = self.lock().tracker.live_shells();
        if shells.is_empty() {
            std::thread::sleep(timeout);
        } else {
            self.src.wait_any_exit(&shells, timeout);
        }
    }

    /// Die Zeilen für die Oberfläche.
    pub fn rows(&self, now: f64, lang: Lang) -> Vec<OrphanView> {
        let g = self.lock();
        orphan_rows(&g.tracker, &g.ports, &g.kills, now, lang)
    }

    /// Kopie des Merkers (Diagnose, Tests).
    pub fn tracker(&self) -> Tracker {
        self.lock().tracker.clone()
    }

    /// Eine Zeile als „wird beendet“ zeigen, bevor der Auftrag im Hintergrund anläuft.
    pub fn mark_busy(&self, id: &str) {
        if let Some(k) = parse_row_id(id) {
            self.lock().kills.busy.insert(k);
        }
    }

    /// Eine Zeile beenden (blockiert bis zum Nachweis, höchstens ein paar Sekunden).
    /// `Err` = Text für die Zeile; er steht dann auch dort.
    pub fn kill(&self, id: &str, now: f64, lang: Lang) -> Result<(), String> {
        let root = parse_row_id(id).ok_or_else(|| lang.l("unbekannte Zeile", "unknown row").to_string())?;
        {
            let mut g = self.lock();
            g.kills.busy.insert(root);
            g.kills.errors.remove(&root);
        }
        let result = self.kill_row(root, now, lang);
        let mut g = self.lock();
        g.kills.busy.remove(&root);
        if let Err(e) = &result {
            g.kills.errors.insert(root, e.clone());
        }
        result
    }

    /// Alle Zeilen beenden; Fehler stehen an ihren Zeilen.
    pub fn kill_all(&self, now: f64, lang: Lang) -> Result<(), String> {
        self.scan(now);
        let all = roots(&self.lock().tracker);
        let failed = all.iter().filter(|k| self.kill(&deck_core::orphans::row_id(**k), now, lang).is_err()).count();
        if failed == 0 { Ok(()) } else { Err(format!("{failed} {}", lang.l("nicht beendet", "not stopped"))) }
    }

    fn kill_row(&self, root: Key, now: f64, lang: Lang) -> Result<(), String> {
        // Frisch hinschauen: was seit dem letzten Blick unter der Wurzel gestartet ist,
        // gehört dazu, und eine inzwischen geänderte Lage zählt.
        self.scan(now);
        let (order, ports) = {
            let g = self.lock();
            match kill_order(&g.tracker, root) {
                Some(o) => {
                    let ports: Vec<u16> = o.iter().flat_map(|k| g.ports.get(&k.0).cloned().unwrap_or_default()).collect();
                    (o, ports)
                }
                // Nicht (mehr) anzubieten: schon weg ist Erfolg, alles andere nicht.
                None if !self.src.running(root.0, root.1) => return Ok(()),
                None => return Err(lang.l("kein Ziel mehr (Fenster offen oder geschützt)", "no longer a target").into()),
            }
        };
        let mut errors = Vec::new();
        for (pid, start) in &order {
            match self.src.terminate(*pid, *start) {
                Ok(_) => {}
                Err(e) if e == ACCESS_DENIED => {
                    errors.push(format!("PID {pid}: {}", lang.l("kein Zugriff (läuft erhöht?)", "access denied (elevated?)")));
                }
                Err(e) => errors.push(format!("PID {pid}: {e}")),
            }
        }
        let deadline = Instant::now() + Duration::from_secs_f64(VERIFY_S);
        let (mut alive, mut busy_ports);
        loop {
            alive = order.iter().filter(|(p, s)| self.src.running(*p, *s)).map(|(p, _)| *p).collect::<Vec<_>>();
            busy_ports = self.src.listening().unwrap_or_default().into_iter().filter(|(_, port)| ports.contains(port)).collect::<Vec<_>>();
            if (alive.is_empty() && busy_ports.is_empty()) || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        self.scan(now);
        if !alive.is_empty() {
            let pids: Vec<String> = alive.iter().map(u32::to_string).collect();
            errors.push(format!("{} {}", lang.l("läuft noch: PID", "still running: PID"), pids.join(", ")));
        }
        for (pid, port) in busy_ports {
            errors.push(format!("{} :{port} (PID {pid})", lang.l("Port noch belegt", "port still in use")));
        }
        if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
    }
}

#[cfg(test)]
#[path = "orphans_tests.rs"]
mod tests;
