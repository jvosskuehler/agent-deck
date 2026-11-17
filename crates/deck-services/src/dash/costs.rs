//! Datenquelle `costs`: Tokenkosten je Ticket aus den Transcripts von Claude Code.
//!
//! Ein Hintergrund-Thread liest alle 60 s die Dateien `~/.claude/projects/**/*.jsonl`
//! (Sitzungen und darunter ihre Subagenten) und verbucht jede Antwort samt den
//! Ticket-Hinweisen ihrer Datei (`deck_core::dash::costs`, `cost_ledger`). Gelesen wird
//! inkrementell: je Datei der Byte-Versatz bis zur letzten vollständigen Zeile; eine Zeile
//! ohne `\n` am Ende schreibt Claude Code gerade noch. Dateien, die seit 31 Tagen nicht
//! geändert wurden, werden nicht geöffnet. Der erste Lauf liest rund 1 GB (Protokoll
//! nennt Dauer und Menge).
//!
//! Inhalte aus den Transcripts kommen nie ins Protokoll (Patientendaten möglich), nur
//! Anzahlen, Dauern und Modellnamen.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, UNIX_EPOCH};

use deck_core::dash::cost_ledger::{CostLedger, CostView};

use crate::fsutil::now_unix;
use crate::logfile::LogFile;

/// Takt des Dienstes.
pub const SCAN_SECONDS: u64 = 60;
/// So weit zurück wird gelesen und gemerkt (die Ansicht zeigt höchstens 30 Tage und den
/// Vergleich mit dem Zeitraum davor, soweit die Transcripts reichen).
const KEEP_S: f64 = 31.0 * 86_400.0;

/// Abstand der Ortszeit zu UTC in Sekunden (MEZ: 3600) zum Zeitpunkt `now`.
pub fn local_offset_s(now: f64) -> i64 {
    use chrono::{Local, Offset, TimeZone};
    Local.timestamp_opt(now.floor() as i64, 0).single().map_or(0, |dt| i64::from(dt.offset().fix().local_minus_utc()))
}

/// Was ein Lauf getan hat.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScanStats {
    pub files_read: usize,
    pub bytes_read: u64,
    pub files_failed: usize,
}

/// Der inkrementelle Leser. Ohne Thread, damit Tests ihn direkt treiben.
pub struct CostScanner {
    root: PathBuf,
    ledger: CostLedger,
    offsets: HashMap<PathBuf, u64>,
}

impl CostScanner {
    pub fn new(root: impl Into<PathBuf>) -> CostScanner {
        CostScanner { root: root.into(), ledger: CostLedger::default(), offsets: HashMap::new() }
    }

    /// Alles vergessen, der nächste Lauf liest jede Datei von vorn.
    pub fn restart(&mut self) {
        self.ledger = CostLedger::default();
        self.offsets.clear();
    }

    pub fn ledger(&self) -> &CostLedger {
        &self.ledger
    }

    /// Ein Lauf. Fehler nur, wenn der Wurzelordner selbst nicht lesbar ist.
    pub fn scan(&mut self, now: f64, prefix: &str) -> io::Result<ScanStats> {
        let mut files = Vec::new();
        collect_jsonl(&self.root, &mut files, true)?;
        let mut stats = ScanStats::default();
        for path in files {
            match self.read_new(&path, now - KEEP_S, prefix) {
                Ok(0) => {}
                Ok(n) => {
                    stats.files_read += 1;
                    stats.bytes_read += n;
                }
                Err(_) => stats.files_failed += 1,
            }
        }
        self.ledger.prune(now - KEEP_S);
        Ok(stats)
    }

    fn read_new(&mut self, path: &Path, since: f64, prefix: &str) -> io::Result<u64> {
        let meta = fs::metadata(path)?;
        let modified = meta.modified()?.duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
        if modified < since {
            return Ok(0);
        }
        let mut offset = self.offsets.get(path).copied().unwrap_or(0);
        if meta.len() < offset {
            offset = 0; // neu geschrieben; doppelte Antworten fängt die message.id ab
        }
        if meta.len() == offset {
            return Ok(0);
        }
        let (key, parent) = file_key(path);
        let mut file = File::open(path)?;
        file.seek(SeekFrom::Start(offset))?;
        let mut reader = BufReader::with_capacity(1 << 16, file);
        let mut buf = Vec::new();
        let start = offset;
        loop {
            buf.clear();
            let n = reader.read_until(b'\n', &mut buf)?;
            if n == 0 || buf.last() != Some(&b'\n') {
                break;
            }
            offset += n as u64;
            if let Ok(line) = std::str::from_utf8(&buf) {
                self.ledger.add_line(&key, parent.as_deref(), line.trim_end(), prefix);
            }
        }
        self.offsets.insert(path.to_path_buf(), offset);
        Ok(offset - start)
    }
}

/// Schlüssel einer Transcript-Datei: die Sitzung (Dateiname ohne `.jsonl`); ein Subagent
/// unter `<sitzung>/subagents/agent-x.jsonl` bekommt `<sitzung>/agent-x` und als Eltern
/// die Sitzung.
pub fn file_key(path: &Path) -> (String, Option<String>) {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
    let parent_dir = path.parent();
    let is_sub = parent_dir.and_then(Path::file_name).and_then(|n| n.to_str()) == Some("subagents");
    if is_sub && let Some(session) = parent_dir.and_then(Path::parent).and_then(Path::file_name).and_then(|n| n.to_str()) {
        return (format!("{session}/{stem}"), Some(session.to_string()));
    }
    (stem, None)
}

fn collect_jsonl(dir: &Path, out: &mut Vec<PathBuf>, is_root: bool) -> io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if is_root => return Err(e),
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else { continue };
        let path = entry.path();
        if kind.is_dir() {
            collect_jsonl(&path, out, false)?;
        } else if kind.is_file() && path.extension().is_some_and(|e| e.eq_ignore_ascii_case("jsonl")) {
            out.push(path);
        }
    }
    Ok(())
}

#[derive(Default)]
struct Control {
    generation: u64,
}

#[derive(Default)]
struct Health {
    first_done: bool,
    root_error: bool,
    files_failed: usize,
    unpriced: usize,
}

struct State {
    scanner: Mutex<CostScanner>,
    view: Mutex<Option<CostView>>,
    prefix: Mutex<String>,
    /// Präfix gewechselt: der nächste Lauf liest von vorn.
    reset: Mutex<bool>,
    health: Mutex<Health>,
    ctl: Mutex<Control>,
    cv: Condvar,
    log: Option<LogFile>,
}

/// Der Hintergrund-Dienst.
pub struct CostService {
    state: Arc<State>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl CostService {
    /// `root` = `~/.claude/projects` (Tests: ein tempdir).
    pub fn new(root: impl Into<PathBuf>, log: Option<LogFile>) -> CostService {
        CostService {
            state: Arc::new(State {
                scanner: Mutex::new(CostScanner::new(root)),
                view: Mutex::new(None),
                prefix: Mutex::new(String::new()),
                reset: Mutex::new(false),
                health: Mutex::new(Health::default()),
                ctl: Mutex::new(Control::default()),
                cv: Condvar::new(),
                log,
            }),
            thread: Mutex::new(None),
        }
    }

    /// Thread starten; `wake`, wenn sich die Ansicht geändert hat. Zweiter Aufruf: nichts.
    pub fn start(&self, wake: impl Fn() + Send + Sync + 'static) {
        let mut t = lock(&self.thread);
        if t.is_some() {
            return;
        }
        let generation = lock(&self.state.ctl).generation;
        let st = self.state.clone();
        *t = std::thread::Builder::new().name("dash-costs".into()).spawn(move || run(&st, generation, &wake)).ok();
    }

    pub fn stop(&self) {
        lock(&self.state.ctl).generation += 1;
        self.state.cv.notify_all();
        lock(&self.thread).take();
    }

    /// Jira-Präfix für Worktree-Ordner mit Nummer (`4258-api` → `ORPH-4258`; leer: `4258`).
    /// Vor `start` setzen, sonst liest der erste Lauf mit dem falschen. Ein Wechsel liest
    /// alles neu: gelesene Zeilen tragen ihr Ticket schon.
    pub fn set_prefix(&self, prefix: &str) {
        let mut p = lock(&self.state.prefix);
        if *p != prefix {
            *p = prefix.to_string();
            // Nicht hier neu anfangen: ein laufender Lauf hält den Leser Sekunden lang,
            // und das hier ruft der Takt des Decks.
            *lock(&self.state.reset) = true;
        }
    }

    /// Letzte Ansicht; `None` vor dem ersten Lauf.
    pub fn view(&self) -> Option<CostView> {
        lock(&self.state.view).clone()
    }

    /// Ein Lauf, synchron; `true` = Ansicht geändert.
    pub fn scan_now(&self, now: f64) -> bool {
        scan(&self.state, now)
    }
}

impl Drop for CostService {
    fn drop(&mut self) {
        self.stop();
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn note(st: &State, msg: &str) {
    if let Some(log) = &st.log {
        log.note(msg);
    }
}

fn scan(st: &State, now: f64) -> bool {
    let began = Instant::now();
    let prefix = lock(&st.prefix).clone();
    let mut scanner = lock(&st.scanner);
    if std::mem::take(&mut *lock(&st.reset)) {
        scanner.restart();
    }
    let result = scanner.scan(now, &prefix);
    let view = scanner.ledger().view(now, local_offset_s(now));
    let replies = scanner.ledger().len();
    let unpriced: Vec<String> = scanner.ledger().unpriced.iter().map(|(m, n)| format!("{m} ({n})")).collect();
    drop(scanner);
    let mut h = lock(&st.health);
    match result {
        Err(e) => {
            if !h.root_error {
                note(st, &format!("Kosten: Transcript-Ordner nicht lesbar ({:?})", e.kind()));
                h.root_error = true;
            }
        }
        Ok(stats) => {
            if h.root_error {
                note(st, "Kosten: Transcript-Ordner wieder lesbar");
                h.root_error = false;
            }
            if !h.first_done {
                h.first_done = true;
                note(st, &format!(
                    "Kosten: erster Lauf {} Dateien, {} MB, {} Antworten, {} ms",
                    stats.files_read,
                    stats.bytes_read / 1_048_576,
                    replies,
                    began.elapsed().as_millis()
                ));
            }
            if (stats.files_failed > 0) != (h.files_failed > 0) {
                note(st, &format!("Kosten: {} Dateien nicht lesbar", stats.files_failed));
            }
            h.files_failed = stats.files_failed;
            if unpriced.len() != h.unpriced {
                // Ein neues Modell ohne Preis: zählt nicht mit, bis PRICES es kennt.
                note(st, &format!("Kosten: Antworten ohne Preis, Modelle {}", unpriced.join(", ")));
                h.unpriced = unpriced.len();
            }
        }
    }
    let mut cur = lock(&st.view);
    let changed = cur.as_ref() != Some(&view);
    *cur = Some(view);
    changed
}

fn run(st: &State, generation: u64, wake: &dyn Fn()) {
    loop {
        let changed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scan(st, now_unix()))).unwrap_or(false);
        if changed {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(wake));
        }
        let ctl = lock(&st.ctl);
        let ctl = if ctl.generation == generation {
            st.cv.wait_timeout(ctl, Duration::from_secs(SCAN_SECONDS)).map(|(g, _)| g).unwrap_or_else(|e| e.into_inner().0)
        } else {
            ctl
        };
        if ctl.generation != generation {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subagent_files_point_to_their_session() {
        let (k, p) = file_key(Path::new(r"C:\x\proj\abc-123\subagents\agent-9.jsonl"));
        assert_eq!((k.as_str(), p.as_deref()), ("abc-123/agent-9", Some("abc-123")));
        let (k, p) = file_key(Path::new(r"C:\x\proj\abc-123.jsonl"));
        assert_eq!((k.as_str(), p), ("abc-123", None));
    }

    #[test]
    fn a_half_written_last_line_waits_for_the_next_scan() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("s1.jsonl");
        let line = |id: &str| {
            format!(
                r#"{{"type":"assistant","timestamp":"2026-11-16T09:00:00Z","cwd":"C:\\R\\wt\\4258-api","message":{{"id":"{id}","model":"claude-opus-5-5","usage":{{"input_tokens":1,"output_tokens":1}}}}}}"#
            )
        };
        fs::write(&f, format!("{}\n{}", line("a"), line("b"))).unwrap();
        let mut s = CostScanner::new(d.path());
        let now = 1_794_826_800.0;
        s.scan(now, "ORPH").unwrap();
        assert_eq!(s.ledger().len(), 1);
        fs::write(&f, format!("{}\n{}\n", line("a"), line("b"))).unwrap();
        s.scan(now, "ORPH").unwrap();
        assert_eq!(s.ledger().len(), 2);
    }
}
