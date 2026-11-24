//! „Hell/Dunkel: Sonnenstand“: das Deck schaltet Windows bei Sonnenaufgang hell und bei
//! Sonnenuntergang dunkel, für den Ort aus der Zeitzone (oder den einer Erweiterung).
//! Ersetzt eine frühere geplante Aufgabe des Autors.
//!
//! Ein eigener Thread wartet bis kurz nach dem nächsten Wechsel, höchstens aber
//! [`SAFETY_TICK`] (falls die Uhr springt). Nachgesehen wird außerdem beim Start, nach dem
//! Aufwachen und wenn sich die Einstellung ändert.
//!
//! **Eine Hand am Schalter bleibt bis zum nächsten Wechsel.** Beim Start, nach dem
//! Aufwachen und nach einer Änderung der Einstellung setzt das Deck den Stand der Sonne
//! durch. Der Takt dazwischen schaltet nur, wenn seit dem letzten Mal die Sonne auf- oder
//! untergegangen ist: wer mittags von Hand auf dunkel stellt, behält dunkel bis zum
//! Sonnenuntergang (der nichts ändert) und wird beim nächsten Sonnenaufgang wieder hell.
//!
//! Geloggt wird nur ein Umschalten (Ziel und nächster Wechsel) und ein Fehler beim
//! ersten Auftreten und bei der Erholung, nie ein Takt.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use deck_core::isotime::utc_stamp;
use deck_core::sun::{self, Place};

use crate::sun_theme::{WindowsTheme, apply};

/// Spätestens so oft wird nachgesehen, auch ohne fälligen Wechsel.
pub const SAFETY_TICK: Duration = Duration::from_secs(600);
/// So lange nach dem berechneten Wechsel wird geschaltet: klar auf der richtigen Seite.
const AFTER_CHANGE_S: f64 = 30.0;

pub type Log = Arc<dyn Fn(String) + Send + Sync>;

/// Wie lange bis zum nächsten Blick: bis kurz nach dem nächsten Wechsel, höchstens
/// [`SAFETY_TICK`], mindestens eine Sekunde.
pub fn wait_for(now: f64, place: &Place) -> Duration {
    let tick = SAFETY_TICK.as_secs_f64();
    let secs = sun::next_change(now, place).map_or(tick, |t| (t - now + AFTER_CHANGE_S).min(tick));
    Duration::from_secs_f64(secs.max(1.0))
}

fn word(light: bool) -> &'static str {
    if light { "hell" } else { "dunkel" }
}

/// Der Stand zwischen zwei Blicken.
#[derive(Debug, Default)]
pub struct Checker {
    /// Ziel beim letzten Blick: ändert es sich, ist die Sonne auf- oder untergegangen.
    last_target: Option<bool>,
    /// Fehlschläge in Folge (geloggt wird der erste und die Erholung).
    failures: u32,
}

impl Checker {
    /// Einmal nachsehen. `enforce`: auch schalten, wenn seit dem letzten Blick kein
    /// Wechsel war (Start, Aufwachen, neue Einstellung). Liefert das neue Ziel, wenn
    /// umgeschaltet wurde.
    pub fn check(&mut self, now: f64, place: &Place, theme: &dyn WindowsTheme, enforce: bool, log: &dyn Fn(String)) -> Option<bool> {
        let target = sun::is_light(now, place);
        if !enforce && self.last_target == Some(target) {
            return None;
        }
        match apply(theme, target) {
            Ok(switched) => {
                self.last_target = Some(target);
                if self.failures > 0 {
                    log(format!("Hell/Dunkel nach Sonnenstand: geht wieder (nach {} Fehlversuchen)", self.failures));
                    self.failures = 0;
                }
                if switched {
                    let next = sun::next_change(now, place).map_or_else(|| "keiner in Sicht".into(), |t| utc_stamp(t as i64));
                    log(format!("Hell/Dunkel nach Sonnenstand ({}): Windows auf {}, nächster Wechsel {next}", place.de, word(target)));
                }
                switched.then_some(target)
            }
            Err(e) => {
                if self.failures == 0 {
                    log(format!("Hell/Dunkel nach Sonnenstand: Windows nicht auf {} gestellt: {e}", word(target)));
                }
                self.failures += 1;
                None
            }
        }
    }
}

enum Msg {
    /// Ort setzen (`None` = Sonnenstand aus) und sofort nachsehen.
    Place(Option<Place>),
    /// Sofort nachsehen und durchsetzen (nach dem Aufwachen).
    Check,
}

/// Griff auf den Thread.
pub struct SunSwitch {
    tx: Mutex<Sender<Msg>>,
}

impl SunSwitch {
    /// Thread starten; er tut nichts, bis [`SunSwitch::set`] einen Ort nennt.
    pub fn start(theme: Arc<dyn WindowsTheme>, clock: fn() -> f64, log: Log) -> SunSwitch {
        let (tx, rx) = mpsc::channel();
        let _ = std::thread::Builder::new().name("sun-switch".into()).spawn(move || run(&rx, theme.as_ref(), clock, &*log));
        SunSwitch { tx: Mutex::new(tx) }
    }

    /// Sonnenstand an (mit Ort) oder aus (`None`). Wirkt sofort.
    pub fn set(&self, place: Option<Place>) {
        self.send(Msg::Place(place));
    }

    /// Nach dem Aufwachen: sofort nachsehen.
    pub fn check_now(&self) {
        self.send(Msg::Check);
    }

    fn send(&self, m: Msg) {
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(m);
        }
    }
}

fn run(rx: &Receiver<Msg>, theme: &dyn WindowsTheme, clock: fn() -> f64, log: &dyn Fn(String)) {
    let mut place: Option<Place> = None;
    let mut checker = Checker::default();
    loop {
        let msg = match place {
            Some(p) => rx.recv_timeout(wait_for(clock(), &p)),
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        let enforce = match msg {
            Ok(Msg::Place(p)) => {
                place = p;
                true
            }
            Ok(Msg::Check) => true,
            Err(RecvTimeoutError::Timeout) => false,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        if let Some(p) = place {
            checker.check(clock(), &p, theme, enforce, log);
        }
    }
}

#[cfg(test)]
#[path = "sun_switch_tests.rs"]
mod tests;
