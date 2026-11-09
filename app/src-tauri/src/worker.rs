//! Ein Arbeiter-Thread für Aufträge mit Wirkung nach außen (Fenster, Prozesse, Tasten).
//!
//! Hintereinander statt parallel: zwei gleichzeitige Fokuswechsel oder Tastenfolgen
//! verheddern sich. Und nicht im Takt selbst: ein Klick soll den 400-ms-Poll nicht
//! anhalten. In Tests läuft der Auftrag sofort im aufrufenden Thread (`inline`).

use std::sync::Mutex;
use std::sync::mpsc::{Sender, channel};

use crate::applog::note;

type Job = Box<dyn FnOnce() + Send + 'static>;

pub struct Worker {
    tx: Option<Mutex<Sender<Job>>>,
}

impl Worker {
    /// Eigener Thread mit Namen.
    pub fn spawn(name: &str) -> Worker {
        let (tx, rx) = channel::<Job>();
        let name = name.to_string();
        let label = name.clone();
        let started = std::thread::Builder::new().name(name).spawn(move || {
            for job in rx {
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)).is_err() {
                    note(format!("Arbeiter {label}: Auftrag abgestürzt"));
                }
            }
        });
        match started {
            Ok(_) => Worker { tx: Some(Mutex::new(tx)) },
            Err(e) => {
                note(format!("Arbeiter nicht startbar ({e}), Aufträge laufen direkt"));
                Worker::inline()
            }
        }
    }

    /// Aufträge laufen sofort (Tests).
    pub fn inline() -> Worker {
        Worker { tx: None }
    }

    pub fn run(&self, job: impl FnOnce() + Send + 'static) {
        match &self.tx {
            Some(tx) => {
                let sent = tx.lock().map(|t| t.send(Box::new(job)).is_ok()).unwrap_or(false);
                if !sent {
                    note("Arbeiter weg, Auftrag verworfen");
                }
            }
            None => job(),
        }
    }
}
