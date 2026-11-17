//! Takt und Gesundheit der Abruf-Dienste (Titan, Sprint): ein Thread je Dienst, der in
//! seinem Takt abruft, früher aufwacht, wenn jemand anstößt (neue Einstellung), und
//! endet, wenn der Dienst angehalten wird. Dazu das Protokoll der Zustandswechsel:
//! nur „geht nicht mehr“ und „geht wieder“, nie jeder Abruf.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use super::hidden_cmd::{short_error, Output};
use crate::logfile::LogFile;

/// Rückruf an die App: „neuer Stand, bitte neu zeichnen“.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// `generation` zählt jedes `stop`: ein Thread läuft nur, solange seine Generation gilt.
#[derive(Default)]
struct Ctl {
    generation: u64,
    kick: bool,
}

/// Der Wecker eines Dienst-Threads.
#[derive(Default)]
pub struct Ticker {
    ctl: Mutex<Ctl>,
    cv: Condvar,
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Ticker {
    /// Sofort die nächste Runde.
    pub fn kick(&self) {
        lock(&self.ctl).kick = true;
        self.cv.notify_all();
    }

    /// Laufenden Thread beenden (er endet nach seinem aktuellen Abruf).
    pub fn stop(&self) {
        lock(&self.ctl).generation += 1;
        self.cv.notify_all();
    }

    /// Bis `delay` warten oder bis angestoßen; `false` = angehalten.
    fn wait(&self, generation: u64, delay: Duration) -> bool {
        let mut ctl = lock(&self.ctl);
        if ctl.generation == generation && !ctl.kick {
            ctl = self.cv.wait_timeout_while(ctl, delay, |c| c.generation == generation && !c.kick).map_or_else(|e| e.into_inner().0, |(g, _)| g);
        }
        ctl.kick = false;
        ctl.generation == generation
    }
}

/// Thread starten: `round` ruft ab und nennt die Pause bis zur nächsten Runde. Eine
/// Panik in `round` beendet den Thread nicht (nächster Versuch nach `fallback`).
pub(crate) fn spawn<F>(name: &str, ticker: Arc<Ticker>, fallback: Duration, mut round: F) -> Option<JoinHandle<()>>
where
    F: FnMut() -> Duration + Send + 'static,
{
    let generation = lock(&ticker.ctl).generation;
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || loop {
            let delay = catch_unwind(AssertUnwindSafe(&mut round)).unwrap_or(fallback);
            if !ticker.wait(generation, delay) {
                return;
            }
        })
        .ok()
}

/// Ergebnis von [`hidden_cmd::run`](super::hidden_cmd::run) als `Ok(stdout)` bei Exit 0,
/// sonst kurz „ssh: Exit 255 <erste Zeile von stderr>“.
pub(crate) fn stdout_or_error(name: &str, result: Result<Output, String>) -> Result<String, String> {
    let out = result?;
    if out.code == 0 {
        return Ok(out.stdout);
    }
    Err(format!("{name}: Exit {} {}", out.code, short_error(&out.stderr)).trim_end().to_string())
}

/// Merkt Fehlversuche in Folge und protokolliert nur die Wechsel.
pub struct Health {
    /// „Titan nicht erreichbar“.
    down: &'static str,
    /// „Titan wieder erreichbar“.
    up: &'static str,
    failures: u32,
}

impl Health {
    pub fn new(down: &'static str, up: &'static str) -> Health {
        Health { down, up, failures: 0 }
    }

    /// Ergebnis eines Abrufs eintragen.
    pub fn record(&mut self, log: &LogFile, result: Result<(), &str>) {
        match result {
            Ok(()) if self.failures > 0 => {
                log.note(&format!("{} nach {} Fehlversuchen", self.up, self.failures));
                self.failures = 0;
            }
            Ok(()) => {}
            Err(e) => {
                if self.failures == 0 {
                    log.note(&format!("{}: {e}", self.down));
                }
                self.failures = self.failures.saturating_add(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logfile::Rotate;
    use std::sync::mpsc;
    use std::time::Instant;

    #[test]
    fn only_the_change_between_working_and_failing_is_logged_with_the_failure_count() {
        let d = tempfile::tempdir().unwrap();
        let log = LogFile::new(d.path().join("x.log"), 100_000, Rotate::Rename);
        let mut h = Health::new("X nicht erreichbar", "X wieder erreichbar");
        h.record(&log, Ok(()));
        h.record(&log, Err("Exit 255"));
        h.record(&log, Err("Exit 255"));
        h.record(&log, Err("Zeitüberschreitung"));
        h.record(&log, Ok(()));
        h.record(&log, Ok(()));
        let text = std::fs::read_to_string(log.path()).unwrap();
        let lines: Vec<&str> = text.lines().map(|l| &l[20..]).collect();
        assert_eq!(lines, ["X nicht erreichbar: Exit 255", "X wieder erreichbar nach 3 Fehlversuchen"]);
    }

    #[test]
    fn a_failed_program_reads_as_exit_code_and_first_error_line() {
        let out = |code: i32| Ok(Output { code, stdout: "daten".into(), stderr: "
 kaputt 
mehr".into() });
        assert_eq!(stdout_or_error("ssh", out(0)), Ok("daten".into()));
        assert_eq!(stdout_or_error("ssh", out(255)), Err("ssh: Exit 255 kaputt".into()));
        assert_eq!(stdout_or_error("ssh", Err("ssh: Zeitlimit 15 s".into())), Err("ssh: Zeitlimit 15 s".into()));
    }

    #[test]
    fn a_kick_starts_the_next_round_early_and_stop_ends_the_thread() {
        let ticker = Arc::new(Ticker::default());
        let (tx, rx) = mpsc::channel();
        let handle = spawn("test-ticker", ticker.clone(), Duration::from_secs(60), move || {
            let _ = tx.send(());
            Duration::from_secs(3600)
        })
        .unwrap();
        rx.recv_timeout(Duration::from_secs(5)).expect("erste Runde sofort");
        let t0 = Instant::now();
        ticker.kick();
        rx.recv_timeout(Duration::from_secs(5)).expect("angestoßen");
        assert!(t0.elapsed() < Duration::from_secs(5));
        ticker.stop();
        handle.join().unwrap();
    }

    #[test]
    fn a_panicking_round_does_not_kill_the_thread() {
        let ticker = Arc::new(Ticker::default());
        let (tx, rx) = mpsc::channel();
        let mut n = 0;
        let handle = spawn("test-panic", ticker.clone(), Duration::from_millis(10), move || {
            n += 1;
            let _ = tx.send(n);
            if n == 1 {
                panic!("Absicht");
            }
            Duration::from_secs(3600)
        })
        .unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(1));
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(2), "nach der Panik weiter");
        ticker.stop();
        handle.join().unwrap();
    }
}
