//! KI-Kurzzusammenfassung und erkannte Bezüge eines Chats, im Hintergrund (Python
//! `claude/summarize.py` `generate`/`_run_claude` und `ui/uithread.py` `_ensure_chat_info`).
//!
//! Je Anfrage ein Thread: erst Ticket und PR (billig, reine Regex, sofort gemeldet),
//! dann die Zusammenfassung (teuer: `claude -p --safe-mode --model haiku`, ~8 s Start).
//! Ob überhaupt neu gefragt wird, entscheidet der Cache in `deck-core`
//! (`summary_cache::plan_summary`); hier sind nur Prozess, Begrenzung und Rückruf.
//!
//! * Je Session läuft höchstens ein Auftrag; adressiert wird über die SESSION, nicht den
//!   Slot (bis das Ergebnis da ist, kann die Kachel längst woanders liegen).
//! * Höchstens [`MAX_CONCURRENT`] `claude`-Prozesse gleichzeitig: ein Lauf ist ein
//!   ~260-MB-Node-Bundle, und beim Vorab-Erzeugen kämen sonst alle Agenten auf einmal.
//! * Zeitlimit; danach wird der Prozess beendet und der alte Cache-Wert bleibt.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use deck_core::claude::refs::Refs;
use deck_core::claude::summary_cache::{
    SummaryGates, SummaryPlan, cached_refs, cached_summary, ensure_refs, finish_summary, plan_summary,
    projects_dir, prune, summary_dir,
};
use deck_core::i18n::Lang;
use deck_win::which::which;

use crate::fsutil::now_unix;
use crate::guard;

/// Höchstens so viele `claude`-Läufe gleichzeitig (der Rest wartet im Thread).
pub const MAX_CONCURRENT: usize = 2;
/// Kein aufblitzendes Konsolenfenster für `claude`.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Ruft `claude` und liefert die reine Textantwort; `None` bei jedem Fehler.
pub trait ClaudeRunner: Send + Sync {
    fn run(&self, prompt: &str, model: &str, timeout: Duration, cwd: &Path) -> Option<String>;
}

/// Die echte Fassung: `claude -p --safe-mode --no-session-persistence --model <m>`.
/// safe-mode schaltet CLAUDE.md, Skills, Hooks, MCP und Plugins ab (schnell, kein
/// agentisches Tool-Loopen); OAuth-Anmeldung und Modellwahl bleiben.
#[derive(Debug, Default, Clone, Copy)]
pub struct ClaudeCli;

impl ClaudeRunner for ClaudeCli {
    fn run(&self, prompt: &str, model: &str, timeout: Duration, cwd: &Path) -> Option<String> {
        guard::check("claude aufrufen").ok()?;
        let exe = which("claude")?;
        let _ = std::fs::create_dir_all(cwd);
        let mut child = Command::new(exe)
            .args(["-p", "--safe-mode", "--no-session-persistence", "--model", model])
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .ok()?;
        // Schreiben und Lesen in eigenen Threads: ein voller Puffer auf einer Seite darf
        // den Prozess nicht festhalten, während hier auf ihn gewartet wird.
        let mut stdin = child.stdin.take()?;
        let input = prompt.as_bytes().to_vec();
        std::thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
        let mut stdout = child.stdout.take()?;
        let reader = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = stdout.read_to_end(&mut buf);
            buf
        });
        let deadline = Instant::now() + timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break Some(s),
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
            }
        };
        let out = reader.join().ok()?;
        status.filter(|s| s.success()).map(|_| String::from_utf8_lossy(&out).into_owned())
    }
}

/// Stellschrauben (Python-Vorgaben: Haiku, 60 s, Gates aus `deck-core`).
#[derive(Debug, Clone)]
pub struct SummaryConfig {
    /// `~/.claude/projects`: die Transcripts.
    pub projects: PathBuf,
    /// `state\summaries`: der Cache, zugleich das Arbeitsverzeichnis von `claude`.
    pub cache_dir: PathBuf,
    pub model: String,
    pub timeout: Duration,
    pub gates: SummaryGates,
    /// Jira-Projekt für die Ticket-Erkennung zum Start (`config::JIRA_PROJECT_KEY`); die
    /// App setzt danach das aus den Einstellungen (`SummaryService::set_project_key`).
    pub project_key: Option<String>,
    /// Ticket und PR erkennen (`config::TICKET_AUTODETECT`).
    pub detect_refs: bool,
}

impl SummaryConfig {
    pub fn standard(state_dir: &Path) -> SummaryConfig {
        SummaryConfig {
            projects: projects_dir(),
            cache_dir: summary_dir(state_dir),
            model: deck_core::config::HOVER_SUMMARY_MODEL.into(),
            timeout: Duration::from_secs(60),
            gates: SummaryGates::default(),
            project_key: Some(deck_core::config::JIRA_PROJECT_KEY.into()),
            detect_refs: deck_core::config::TICKET_AUTODETECT,
        }
    }
}

/// Was ein Auftrag meldet.
#[derive(Debug, Clone, PartialEq)]
pub enum ChatInfo {
    /// Ticket und PR (kommt zuerst, ohne auf `claude` zu warten).
    Refs(Refs),
    /// Die Zusammenfassung; `None` = keine (Fehlschlag oder nichts zu lesen). Kommt in
    /// jedem Fall als letzte Meldung, danach ist der Auftrag beendet.
    Summary(Option<String>),
}

/// Rückruf: (Session-ID, Meldung), aus dem Hintergrund-Thread.
pub type InfoCallback = Arc<dyn Fn(&str, ChatInfo) + Send + Sync>;

/// Zählende Sperre: höchstens n gleichzeitig.
struct Gate {
    free: Mutex<usize>,
    cv: Condvar,
}

impl Gate {
    fn run<T>(&self, f: impl FnOnce() -> T) -> T {
        let mut n = self.free.lock().unwrap_or_else(|e| e.into_inner());
        while *n == 0 {
            n = self.cv.wait(n).unwrap_or_else(|e| e.into_inner());
        }
        *n -= 1;
        drop(n);
        let out = f();
        *self.free.lock().unwrap_or_else(|e| e.into_inner()) += 1;
        self.cv.notify_one();
        out
    }
}

/// Der Dienst.
pub struct SummaryService {
    cfg: SummaryConfig,
    runner: Arc<dyn ClaudeRunner>,
    gate: Arc<Gate>,
    jobs: Arc<Mutex<HashSet<String>>>,
    /// Das wirksame Jira-Projekt. Bis Nov. 2026 galt hier fest `PROJ`, auch wenn in den
    /// Einstellungen `ORPH` stand: echte Keys bekamen keinen Vorzug, und aus
    /// „Ticket 4116“ wurde `PROJ-4116`.
    project_key: Mutex<Option<String>>,
}

impl SummaryService {
    pub fn new(cfg: SummaryConfig, runner: Arc<dyn ClaudeRunner>) -> SummaryService {
        let project_key = Mutex::new(cfg.project_key.clone());
        SummaryService {
            cfg,
            runner,
            gate: Arc::new(Gate { free: Mutex::new(MAX_CONCURRENT), cv: Condvar::new() }),
            jobs: Arc::new(Mutex::new(HashSet::new())),
            project_key,
        }
    }

    /// Jira-Projekt aus den Einstellungen (`jira_prefix`); gilt ab dem nächsten Auftrag.
    pub fn set_project_key(&self, key: Option<&str>) {
        let key = key.map(str::trim).filter(|k| !k.is_empty()).map(String::from);
        *self.project_key.lock().unwrap_or_else(|e| e.into_inner()) = key;
    }

    fn project_key(&self) -> Option<String> {
        self.project_key.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Die echte Fassung mit dem `claude` vom PATH.
    pub fn system(state_dir: &Path) -> SummaryService {
        SummaryService::new(SummaryConfig::standard(state_dir), Arc::new(ClaudeCli))
    }

    /// Nur der Cache, ohne Transcript und ohne Prozess (für den Hover sofort).
    pub fn cached_summary(&self, session_id: &str) -> Option<String> {
        cached_summary(&self.cfg.cache_dir, session_id)
    }

    /// Nur die schon erkannten Bezüge.
    pub fn cached_refs(&self, session_id: &str) -> Refs {
        cached_refs(&self.cfg.cache_dir, session_id, self.project_key().as_deref())
    }

    /// Alte Cache-Dateien längst geschlossener Sessions entfernen (einmal beim Start).
    pub fn prune(&self, max_age_days: f64) {
        prune(&self.cfg.cache_dir, max_age_days, std::time::SystemTime::now());
    }

    /// Bezüge und (mit `summarize`) Zusammenfassung im Hintergrund sicherstellen. `false`
    /// = für diese Session läuft schon ein Auftrag (dessen Meldungen kommen ja).
    pub fn request(&self, session_id: &str, cwd: Option<&str>, lang: Lang, summarize: bool, on_info: InfoCallback) -> bool {
        if session_id.is_empty() {
            return false;
        }
        if !self.jobs.lock().unwrap_or_else(|e| e.into_inner()).insert(session_id.to_string()) {
            return false;
        }
        let job = Job {
            cfg: SummaryConfig { project_key: self.project_key(), ..self.cfg.clone() },
            runner: self.runner.clone(),
            gate: self.gate.clone(),
            jobs: self.jobs.clone(),
            sid: session_id.to_string(),
            cwd: cwd.map(String::from),
        };
        let spawned = std::thread::Builder::new().name("chat-info".into()).spawn(move || job.run(lang, summarize, &on_info));
        if spawned.is_err() {
            self.jobs.lock().unwrap_or_else(|e| e.into_inner()).remove(session_id);
            return false;
        }
        true
    }

    /// Zusammenfassung sicherstellen, BLOCKIEREND (nie im UI-Thread aufrufen).
    pub fn generate(&self, session_id: &str, cwd: Option<&str>, lang: Lang) -> Option<String> {
        generate(&self.cfg, self.runner.as_ref(), &self.gate, session_id, cwd, lang)
    }
}

struct Job {
    cfg: SummaryConfig,
    runner: Arc<dyn ClaudeRunner>,
    gate: Arc<Gate>,
    jobs: Arc<Mutex<HashSet<String>>>,
    sid: String,
    cwd: Option<String>,
}

impl Job {
    fn run(self, lang: Lang, summarize: bool, on_info: &InfoCallback) {
        let sid = self.sid.clone();
        // Was immer hier schiefgeht: der Auftrag muss aus `jobs` heraus, sonst kommt
        // für diese Session nie wieder eine Zusammenfassung.
        let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if self.cfg.detect_refs {
                let refs = ensure_refs(&self.cfg.projects, &self.cfg.cache_dir, &sid, self.cwd.as_deref(), self.cfg.project_key.as_deref());
                on_info(&sid, ChatInfo::Refs(refs));
            }
            if summarize { generate(&self.cfg, self.runner.as_ref(), &self.gate, &sid, self.cwd.as_deref(), lang) } else { None }
        }));
        self.jobs.lock().unwrap_or_else(|e| e.into_inner()).remove(&sid);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| on_info(&sid, ChatInfo::Summary(out.unwrap_or(None)))));
    }
}

fn generate(cfg: &SummaryConfig, runner: &dyn ClaudeRunner, gate: &Gate, sid: &str, cwd: Option<&str>, lang: Lang) -> Option<String> {
    let lang = lang.setting_value();
    match plan_summary(&cfg.projects, &cfg.cache_dir, sid, cwd, lang, now_unix(), cfg.gates) {
        SummaryPlan::Cached(text) => text,
        SummaryPlan::Generate { prompt, size, cached } => {
            let out = gate.run(|| runner.run(&prompt, &cfg.model, cfg.timeout, &cfg.cache_dir));
            finish_summary(&cfg.cache_dir, sid, out.as_deref(), size, lang, now_unix(), cached)
        }
    }
}

#[cfg(test)]
#[path = "summary_tests.rs"]
mod tests;
