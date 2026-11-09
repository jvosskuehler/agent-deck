//! Claude-Nutzung (Session- und Wochenlimits) im Hintergrund holen (Python
//! `claude/usage.py`, `UsagePoller`).
//!
//! Ein Thread fragt im Takt den gemeinsamen Poller ([`shared`]) und hält den letzten
//! Stand als `deck_core::claude::usage::UsageSnapshot`. Der gemeinsame Poller ist nur
//! dann ein Netzabruf, wenn sein Cache fällig ist; sonst ein billiges Lesen. Darum darf
//! die Anzeige hier alle ~30 s spiegeln, ohne das Rate-Limit zu belasten.
//!
//! Zwei Regeln beim Übernehmen, beide aus schmerzhafter Erfahrung:
//! * `data_ts` ist die Zeit der DATEN, nicht des Lesens, und `error` bleibt auch neben
//!   Daten stehen (2026-08-05: stundenalte 48 % als frisch und grün gezeigt).
//! * Ein Limit hinter seinem Reset ist keine Zahl mehr (`expire_limits` in `deck-core`).

pub mod account;
pub mod fetch;
pub mod policy;
pub mod shared;
pub mod token;

use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use deck_core::claude::usage::{UsageSnapshot, apply, fail, note_account, usage_view};
use deck_core::i18n::Lang;
use deck_core::view::UsageView;

use crate::fsutil::{now_unix, random_unit};
use shared::{NamedMutex, SharedPoller, SharedView};

/// Rückruf bei jedem neuen Stand (aus dem Hintergrund-Thread).
pub type UsageCallback = Box<dyn Fn(&UsageSnapshot) + Send + Sync>;

/// Gemeinsamen Stand in den Snapshot übernehmen (Python `poll_once`, Zweig mit
/// gemeinsamem Poller). Ohne Daten entscheidet der Fehlertext, ob die Zahl verschwindet
/// (Token weg: hart) oder stehen bleibt (Rate-Limit, Netz: weich). Vorher zählt das
/// Konto: gehört der Stand einem anderen als bisher, fallen die alten Zahlen weg.
pub fn apply_view(snap: &mut UsageSnapshot, v: &SharedView, now: f64, lang: Lang) {
    note_account(snap, v.account.as_deref());
    let Some(data) = &v.data else {
        let err = v.error.clone().unwrap_or_default();
        let low = err.to_lowercase();
        if low.contains("ungueltig") {
            fail(snap, lang.l("Token ungültig, bitte 'claude auth login'", "Token invalid, run 'claude auth login'"), true);
        } else if low.contains("notokenerror") {
            // Weder CLI noch Desktop haben ein Token. Der Hinweis zeigt auf die CLI:
            // die hat jeder Deck-Nutzer, ein Login dort ist der kürzere Weg.
            fail(snap, lang.l("Nicht angemeldet, bitte 'claude auth login'", "Not signed in, run 'claude auth login'"), true);
        } else if low.contains("kontowechsel") {
            fail(snap, lang.l("Kontowechsel, lade neu…", "Account switched, reloading…"), false);
        } else if low.contains("rate") {
            fail(snap, lang.l("Rate-Limit, kurz warten", "Rate limit, wait a moment"), false);
        } else if err.is_empty() {
            fail(snap, lang.l("warte auf Daten…", "waiting for data…"), false);
        } else {
            fail(snap, &err, false);
        }
        return;
    };
    apply(snap, data, v.data_ts, v.error.clone().filter(|e| !e.is_empty()), now, lang);
}

/// Pause bis zum nächsten Blick (Python `_loop`): mit Zahl ~30 s ± 8 (gegen Gleichschritt),
/// mit Fehler höchstens 30 s (nicht alle 5 s weiterhämmern), beim frischen Start 5 s.
pub fn next_delay(snap: &UsageSnapshot, poll_seconds: f64, unit: f64) -> f64 {
    let delay = if snap.session_percent.is_some() {
        30.0 + 8.0 * unit
    } else if snap.error.is_some() {
        poll_seconds.min(30.0)
    } else {
        5.0
    };
    delay.max(3.0)
}

/// `generation` zählt jedes `stop`: ein Thread läuft nur, solange seine Generation gilt.
/// So beendet ein `stop` direkt vor einem neuen `start` den alten Thread sicher.
#[derive(Default)]
struct Control {
    generation: u64,
    force: bool,
}

struct State {
    poller: SharedPoller,
    snap: Mutex<UsageSnapshot>,
    lang: Mutex<Lang>,
    ctl: Mutex<Control>,
    wake: Condvar,
    on_update: Mutex<Option<UsageCallback>>,
    poll_seconds: f64,
}

/// Der Hintergrund-Dienst.
pub struct UsageService {
    state: Arc<State>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl UsageService {
    /// Mit eigenem Poller (Tests, oder ein anderer Cache-Ordner). `poll_seconds` wie
    /// `config::USAGE_POLL_SECONDS` (mindestens 30).
    pub fn new(poller: SharedPoller, lang: Lang, poll_seconds: f64) -> UsageService {
        UsageService {
            state: Arc::new(State {
                poller,
                snap: Mutex::new(UsageSnapshot::default()),
                lang: Mutex::new(lang),
                ctl: Mutex::new(Control::default()),
                wake: Condvar::new(),
                on_update: Mutex::new(None),
                poll_seconds: poll_seconds.max(30.0),
            }),
            thread: Mutex::new(None),
        }
    }

    /// Die echte Fassung: gemeinsamer Cache unter `%LOCALAPPDATA%\claude-usage`,
    /// Named-Mutex, Tokens von CLI und Claude Desktop (Konto je Token über dasselbe
    /// HTTP), HTTPS über Schannel.
    pub fn system(lang: Lang) -> UsageService {
        let http: Arc<dyn fetch::UsageHttp> = Arc::new(fetch::UreqHttp::new());
        let poller = SharedPoller::new(
            shared::default_dir(),
            Arc::new(NamedMutex),
            Arc::new(token::DiskTokens::system(http.clone())),
            http,
            Arc::new(now_unix),
            Arc::new(random_unit),
        );
        UsageService::new(poller, lang, f64::from(deck_core::config::USAGE_POLL_SECONDS))
    }

    /// Thread starten; ein zweiter Aufruf ersetzt nur den Rückruf.
    pub fn start(&self, on_update: UsageCallback) {
        *lock(&self.state.on_update) = Some(on_update);
        let mut t = lock(&self.thread);
        if t.is_some() {
            return;
        }
        let generation = lock(&self.state.ctl).generation;
        let st = self.state.clone();
        *t = std::thread::Builder::new().name("usage-poll".into()).spawn(move || run(&st, generation)).ok();
    }

    /// Anhalten (wartet nicht auf einen laufenden Abruf, der endet nach höchstens 20 s).
    pub fn stop(&self) {
        lock(&self.state.ctl).generation += 1;
        self.state.wake.notify_all();
        lock(&self.thread).take();
    }

    /// Manueller Refresh (Python: Rechtsklick auf die Anzeige): Frist und gespeichertes
    /// Token übergehen, im Hintergrund.
    pub fn refresh(&self) {
        lock(&self.state.ctl).force = true;
        self.state.wake.notify_all();
    }

    /// Sprache der Fehlertexte (folgt `language` in Claudes settings.json).
    pub fn set_lang(&self, lang: Lang) {
        *lock(&self.state.lang) = lang;
    }

    pub fn snapshot(&self) -> UsageSnapshot {
        lock(&self.state.snap).clone()
    }

    /// Anzeigemodell für jetzt. Die Frische altert ohne neuen Abruf; wer zeichnet, fragt
    /// darum regelmäßig nach, nicht nur beim Rückruf.
    pub fn view(&self, lang: Lang) -> UsageView {
        usage_view(&self.snapshot(), now_unix(), lang)
    }

    /// Ein Blick, synchron (der Thread macht genau das im Takt).
    pub fn poll_once(&self, force: bool) -> UsageSnapshot {
        poll(&self.state, force)
    }
}

impl Drop for UsageService {
    fn drop(&mut self) {
        self.stop();
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn poll(st: &State, force: bool) -> UsageSnapshot {
    let v = st.poller.get_usage(force);
    let lang = *lock(&st.lang);
    let mut snap = lock(&st.snap);
    apply_view(&mut snap, &v, now_unix(), lang);
    snap.clone()
}

fn run(st: &State, generation: u64) {
    let mut force = false;
    loop {
        // Die Schleife darf nie sterben: ein Fehler im Abruf ist ein Fehlertext.
        let snap = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| poll(st, force)))
            .unwrap_or_else(|_| lock(&st.snap).clone());
        if let Some(cb) = lock(&st.on_update).as_ref() {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cb(&snap)));
        }
        let delay = Duration::from_secs_f64(next_delay(&snap, st.poll_seconds, random_unit()));
        let mut ctl = lock(&st.ctl);
        if ctl.generation == generation && !ctl.force {
            ctl = st.wake.wait_timeout(ctl, delay).map(|(g, _)| g).unwrap_or_else(|e| e.into_inner().0);
        }
        if ctl.generation != generation {
            return;
        }
        force = std::mem::take(&mut ctl.force);
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
