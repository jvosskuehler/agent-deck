//! Der gemeinsame EIN-Poller (Python `claude-usage-shared/usage_poller.py`, `get_usage`).
//!
//! Tray-Monitor, Python-Deck und diese Fassung fragen denselben, eng limitierten
//! Endpoint. Pollte jedes für sich, wäre das Budget schnell weg (429, eingefrorene
//! Anzeigen). Darum teilen sich alle Prozesse einen Cache:
//!
//! * `%LOCALAPPDATA%\claude-usage\usage.json`: letztes Ergebnis, Datenzeit, Fehler,
//!   nächster erlaubter Abruf (`next_ts`), 429-Zähler, gelernter Takt.
//! * Named-Mutex `claude_usage_poller_v1`: höchstens EIN Abruf je Zeitfenster über alle
//!   Prozesse. Windows räumt ihn beim Prozessende selbst auf.
//!
//! Lebt der Cache (`now < next_ts`), wird **nicht** abgerufen, nur gelesen; dasselbe,
//! wenn ein anderer Prozess gerade den Mutex hält. Erst ein fälliger Cache führt zu
//! genau einem Abruf, mit gelerntem Takt, Backoff und `Retry-After` (`policy.rs`).
//! Diese Fassung ist damit ein gleichberechtigter Teilnehmer des gemeinsamen Pollers;
//! sie schreibt dasselbe Format und dasselbe Protokoll (`poller.log`, von `doctor.py`
//! nach Stunde ausgewertet).
//!
//! **Der Cache trägt sein Konto** (`account`, E-Mail klein, seit 2026-11-08). Ein Cache,
//! dessen Konto nicht das aktuelle ist, gilt als nicht vorhanden: seine Zahl gehört
//! jemand anderem. Darum schreibt JEDER Schreibweg `account` mit; fehlte es, verwürfe
//! der Tray (Python) den Cache bei jedem Blick als fremd und fragte selbst nach, und
//! das Budget des Endpoints wäre schnell weg.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};

use super::fetch::{FetchError, UsageHttp, fetch_usage};
use super::policy::{INTERVAL_START, SOFT_RETRY, backoff_after_429, jit, next_interval, parse_retry_after};
use super::token::TokenSource;
use crate::fsutil::atomic_write_with;
use crate::logfile::{LogFile, Rotate};

pub const MUTEX_NAME: &str = "claude_usage_poller_v1";
pub const CACHE_FILE: &str = "usage.json";
pub const LOG_FILE: &str = "poller.log";
/// Wartezeit auf den Mutex beim manuellen Refresh (ms): der Klick soll nicht ins Leere
/// laufen, nur weil gerade ein anderer Prozess pollt.
const FORCE_WAIT_MS: u32 = 2000;

/// `%LOCALAPPDATA%\claude-usage`.
pub fn default_dir() -> PathBuf {
    deck_core::paths::local_app_data().join("claude-usage")
}

/// Was die Apps aus dem Cache brauchen.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SharedView {
    /// Rohe API-Antwort; `None` = noch nie eine erhalten.
    pub data: Option<Value>,
    /// Fehler des letzten Abrufs, auch NEBEN Daten (dann sind sie alt).
    pub error: Option<String>,
    /// Zeit der Daten, nicht des Lesens.
    pub data_ts: Option<f64>,
    /// Konto, zu dem `data` gehört (E-Mail, klein); `None` = unbekannt.
    pub account: Option<String>,
}

/// Prozessübergreifende Ein-Schreiber-Sperre.
pub trait PollLock: Send + Sync {
    /// Sperre nehmen, höchstens `timeout_ms` warten. `None` = jemand anders hält sie.
    /// Die Sperre gilt, bis der Rückgabewert fällt (im selben Thread).
    fn try_lock(&self, timeout_ms: u32) -> Option<Box<dyn std::any::Any>>;
}

/// Die echte Sperre: der Named-Mutex, den auch Tray und Python-Deck nehmen.
#[derive(Debug, Default, Clone, Copy)]
pub struct NamedMutex;

struct MutexGuard(windows::Win32::Foundation::HANDLE);

impl Drop for MutexGuard {
    fn drop(&mut self) {
        // SAFETY: das Handle gehört uns und wird genau hier freigegeben.
        unsafe {
            let _ = windows::Win32::System::Threading::ReleaseMutex(self.0);
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

impl PollLock for NamedMutex {
    fn try_lock(&self, timeout_ms: u32) -> Option<Box<dyn std::any::Any>> {
        use windows::Win32::Foundation::{CloseHandle, WAIT_ABANDONED, WAIT_OBJECT_0};
        use windows::Win32::System::Threading::{CreateMutexW, WaitForSingleObject};
        let name: Vec<u16> = MUTEX_NAME.encode_utf16().chain(Some(0)).collect();
        // SAFETY: Name ist nullterminiert und lebt während des Aufrufs; das Handle wird
        // entweder im Guard oder sofort wieder geschlossen. WAIT_ABANDONED heißt: der
        // Vorbesitzer ist gestorben, Windows hat die Sperre an uns übergeben.
        unsafe {
            let h = CreateMutexW(None, false, windows::core::PCWSTR(name.as_ptr())).ok()?;
            let r = WaitForSingleObject(h, timeout_ms);
            if r == WAIT_OBJECT_0 || r == WAIT_ABANDONED {
                return Some(Box::new(MutexGuard(h)));
            }
            let _ = CloseHandle(h);
            None
        }
    }
}

type Clock = Arc<dyn Fn() -> f64 + Send + Sync>;

/// Ein Teilnehmer des gemeinsamen Pollers.
pub struct SharedPoller {
    dir: PathBuf,
    lock: Arc<dyn PollLock>,
    tokens: Arc<dyn TokenSource>,
    http: Arc<dyn UsageHttp>,
    clock: Clock,
    /// Zufall in [-1, 1] für die Streuung.
    unit_rand: Clock,
    log: LogFile,
}

/// Die Quellen-Namen zu den Token-Indizes (gleiche Reihenfolge wie `DiskTokens`), nur
/// fürs Protokoll: ohne den Namen sieht man einem 429 nicht an, WESSEN Budget leer ist.
fn src_name(i: usize) -> String {
    ["CLI", "Desktop"].get(i).map_or_else(|| format!("#{i}"), |s| s.to_string())
}

fn f(c: &Value, key: &str) -> Option<f64> {
    c.get(key).and_then(Value::as_f64)
}

fn empty() -> Value {
    json!({"data": null, "data_ts": null, "error": null, "next_ts": 0, "n429": 0, "interval": INTERVAL_START})
}

fn view(c: &Value) -> SharedView {
    SharedView {
        data: c.get("data").filter(|v| !v.is_null()).cloned(),
        error: c.get("error").and_then(Value::as_str).map(String::from),
        data_ts: f(c, "data_ts"),
        account: account_of(c).map(String::from),
    }
}

/// Konto eines Cache-Stands; fehlt oder `null` = unbekannt (Pythons `c.get("account")`).
fn account_of(c: &Value) -> Option<&str> {
    c.get("account").and_then(Value::as_str)
}

/// Text, wenn nach einem Kontowechsel noch keine eigene Zahl da ist. Wörtlich wie im
/// gemeinsamen Poller; `apply_view` übersetzt ihn für die Oberfläche.
pub const SWITCH_ERROR: &str = "Kontowechsel – lade neu…";

impl SharedPoller {
    pub fn new(
        dir: PathBuf,
        lock: Arc<dyn PollLock>,
        tokens: Arc<dyn TokenSource>,
        http: Arc<dyn UsageHttp>,
        clock: Clock,
        unit_rand: Clock,
    ) -> SharedPoller {
        let log = LogFile::new(dir.join(LOG_FILE), 131_072, Rotate::KeepTail(300))
            .with_prefix(format!("  pid={}  ", std::process::id()));
        SharedPoller { dir, lock, tokens, http, clock, unit_rand, log }
    }

    pub fn cache_path(&self) -> PathBuf {
        self.dir.join(CACHE_FILE)
    }

    /// Cache lesen; fehlt oder kaputt → `None`.
    pub fn read_cache(&self) -> Option<Value> {
        read_cache(&self.cache_path())
    }

    fn write_cache(&self, c: &Value) {
        let suffix = format!(".{}.tmp", std::process::id());
        let _ = atomic_write_with(&self.cache_path(), deck_core::pyjson::dumps(c).as_bytes(), &suffix);
    }

    /// Die aktuelle Nutzung. Lebt der Cache, ohne Abruf; sonst prozessübergreifend
    /// genau EIN Abruf. `force` ist der manuelle Refresh: Cache-Frist UND gespeichertes
    /// Token werden übergangen, und auf den Mutex wird kurz gewartet.
    pub fn get_usage(&self, force: bool) -> SharedView {
        let now = (self.clock)();
        let acc = self.tokens.current_account(); // Kontowechsel? (im Normalfall ein stat)
        let c = self.read_cache();
        let same = c.as_ref().filter(|c| account_of(c) == acc.as_deref());
        if !force && let Some(c) = same
            && now < f(c, "next_ts").unwrap_or(0.0)
        {
            return view(c);
        }
        let Some(_guard) = self.lock.try_lock(if force { FORCE_WAIT_MS } else { 0 }) else {
            // Ein anderer Prozess pollt gerade: letzten Wert liefern, er aktualisiert
            // gleich. Aber nur, wenn er zum selben Konto gehört; sonst lieber keine Zahl
            // als eine fremde.
            return match same {
                Some(c) => view(c),
                None => SharedView { error: Some(SWITCH_ERROR.into()), account: acc, ..Default::default() },
            };
        };
        let mut c = self.read_cache().unwrap_or_else(empty);
        let now = (self.clock)();
        if account_of(&c) != acc.as_deref() {
            // Zahl und Sperrzähler gehörten zum alten Konto bzw. dessen Token. Nur der
            // gelernte Takt bleibt: er beschreibt den Endpoint, kein Konto.
            if !c.get("data").is_none_or(Value::is_null) {
                let name = |a: Option<&str>| a.unwrap_or("unbekannt").to_string();
                self.log.note(&format!("Kontowechsel {} -> {}: Cache verworfen", name(account_of(&c)), name(acc.as_deref())));
            }
            let interval = c.get("interval").filter(|v| deck_core::pystr::truthy(v)).cloned();
            let mut fresh = empty();
            if let Some(iv) = interval {
                fresh["interval"] = iv;
            }
            fresh["account"] = acc.clone().map_or(Value::Null, Value::from);
            c = fresh;
        } else if !force && now < f(&c, "next_ts").unwrap_or(0.0) {
            return view(&c); // unter der Sperre schon aktualisiert
        }
        let mut next = self.poll(&c, now, force);
        // Das Konto der Tokens, mit denen eben abgefragt wurde, nicht `acc`: bei einem
        // 401 liest `fetch_raw` die Tokens neu, das Konto kann sich dabei noch einmal
        // geändert haben. Steht in JEDEM Schreibweg (Erfolg, 401, 429, Netzfehler).
        next["account"] = self.tokens.account().map_or(Value::Null, Value::from);
        self.write_cache(&next);
        view(&next)
    }

    /// Ein Abruf und der neue Cache-Stand.
    fn poll(&self, c: &Value, now: f64, force: bool) -> Value {
        let jit = |s: f64| jit(s, (self.unit_rand)());
        let interval = f(c, "interval").filter(|i| *i > 0.0).unwrap_or(INTERVAL_START);
        let n429 = c.get("n429").and_then(Value::as_u64).unwrap_or(0) as u32;
        let keep = |error: String, next_ts: f64, n429: u32, interval: f64| {
            json!({"data": c.get("data").cloned().unwrap_or(Value::Null),
                   "data_ts": c.get("data_ts").cloned().unwrap_or(Value::Null),
                   "error": error, "next_ts": next_ts, "n429": n429, "interval": interval})
        };
        match self.fetch_raw(force) {
            Ok((data, i)) => {
                let iv = next_interval(Some(interval), true);
                self.log.note(&format!("OK (Abruf, {}-Token) -> Cache aktualisiert, Takt {}s", src_name(i), iv as i64));
                json!({"data": data, "data_ts": now, "error": null, "next_ts": now + jit(iv), "n429": 0, "interval": iv})
            }
            Err(FetchError::Http { code: code @ (401 | 403), .. }) => {
                self.log.note(&format!("HTTP {code} (Token ungueltig)"));
                json!({"data": null, "data_ts": null, "error": "Token ungueltig – 'claude auth login'",
                       "next_ts": now + jit(interval), "n429": 0, "interval": interval})
            }
            Err(FetchError::Http { code: 429, retry_after }) => {
                // Hier sind ALLE Tokens abgewiesen: jetzt ist der Backoff berechtigt.
                // Zwei Größen mit Absicht: `back` ist die akute Sperre, `iv` der gelernte
                // Takt für danach. Von beiden gewinnt die vorsichtigere.
                let n = n429 + 1;
                let ra = parse_retry_after(retry_after.as_deref(), now);
                let back = backoff_after_429(n, ra);
                let iv = next_interval(Some(interval), false);
                let wait = back.max(iv);
                let src = ra.map_or_else(|| "geraten".to_string(), |r| format!("Retry-After {}s", r as i64));
                self.log.note(&format!(
                    "HTTP 429 (alle Tokens) -> warte {}s (n={n}, {src}), Takt jetzt {}s",
                    wait as i64, iv as i64
                ));
                keep("Rate-Limit".into(), now + jit(wait), n, iv)
            }
            Err(FetchError::Http { code, .. }) => {
                self.log.note(&format!("HTTP {code} (letzter Wert bleibt)"));
                keep(format!("HTTP {code}"), now + jit(SOFT_RETRY), n429, interval)
            }
            Err(FetchError::Other(name)) => {
                // n429 BLEIBT: ein Netzfehler ist keine Entwarnung. Wer den Zähler hier
                // nullt, rennt nach 25 s direkt in die nächste Sperre (2026-08-18: 22
                // `getaddrinfo failed`, jeder hat das Limit weiter angeheizt).
                let wait = if n429 == 0 { SOFT_RETRY } else { backoff_after_429(n429, None) };
                self.log.note(&format!(
                    "FEHLER {name} (letzter Wert bleibt, warte {}s, n429={n429})",
                    wait as i64
                ));
                keep(name, now + jit(wait), n429, interval)
            }
        }
    }

    /// Ein Abruf; bei 401/403 einmal die Tokens frisch lesen. `force` liest schon beim
    /// ersten Versuch neu: nach einem Kontowechsel ist das gespeicherte Token zwar noch
    /// gültig, gehört aber zum falschen Konto und liefert brav die falschen Zahlen.
    fn fetch_raw(&self, force: bool) -> Result<(Value, usize), FetchError> {
        let mut note = |i: usize, code: u16| {
            self.log.note(&format!("  {}-Token -> HTTP {code}, naechste Quelle", src_name(i)));
        };
        let tokens = self.tokens.tokens(force).map_err(|_| FetchError::Other("NoTokenError".into()))?;
        match fetch_usage(self.http.as_ref(), &tokens, &mut note) {
            Err(FetchError::Http { code: 401 | 403, .. }) => {
                let fresh = self.tokens.tokens(true).map_err(|_| FetchError::Other("NoTokenError".into()))?;
                fetch_usage(self.http.as_ref(), &fresh, &mut note)
            }
            other => other,
        }
    }
}

/// Cache-Datei lesen; fehlt oder kaputt → `None`.
pub fn read_cache(path: &Path) -> Option<Value> {
    deck_core::paths::load_json(path).filter(Value::is_object)
}

#[cfg(test)]
#[path = "shared_tests.rs"]
pub(crate) mod tests;
