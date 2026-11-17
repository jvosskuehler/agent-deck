//! Datenquelle `titan`: alle 60 s (mit laufenden `titan run`-Läufen alle 20 s) ein
//! ssh-Aufruf mit dem Status-Skript aus `deck_core::dash::titan`. Texte baut erst
//! deck-core.
//!
//! ssh wie `Repos/titan/titan.py`: das Windows-OpenSSH (nutzt den Windows-SSH-Agent mit
//! dem Titan-Schlüssel; das ssh aus Git Bash hätte einen eigenen Agenten ohne
//! Schlüssel), Host `gpu` aus `~/.ssh/config`, beides überschreibbar per `TITAN_SSH` /
//! `TITAN_HOST`. Das Skript geht als ein Argument an ssh (Rust quotet es für die
//! Windows-Befehlszeile, geprüft mit dem echten Probelauf unten).

use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use deck_core::dash::titan::{merge, next_delay_s, parse, TitanSnapshot, STATUS_SH};

use super::hidden_cmd;
use super::ticker::{lock, spawn, stdout_or_error, Health, Ticker, Wake};
use crate::logfile::LogFile;

/// Zeitlimit eines Abrufs (Verbindungsaufbau selbst höchstens 5 s).
const SSH_TIMEOUT: Duration = Duration::from_secs(15);

/// Holt die rohe Ausgabe des Status-Skripts.
pub trait TitanProbe: Send + Sync {
    fn status(&self, script: &str) -> Result<String, String>;
}

/// Die echte Fassung: Windows-OpenSSH, nur lesend.
#[derive(Debug, Clone)]
pub struct SshProbe {
    pub ssh: PathBuf,
    pub host: String,
}

impl SshProbe {
    /// Programm und Host wie `titan.py`.
    pub fn system() -> SshProbe {
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        SshProbe {
            ssh: env("TITAN_SSH").map_or_else(|| PathBuf::from(r"C:\Windows\System32\OpenSSH\ssh.exe"), PathBuf::from),
            host: env("TITAN_HOST").unwrap_or_else(|| "gpu".into()),
        }
    }
}

impl TitanProbe for SshProbe {
    fn status(&self, script: &str) -> Result<String, String> {
        let mut cmd = Command::new(&self.ssh);
        cmd.args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", self.host.as_str(), script]);
        stdout_or_error("ssh", hidden_cmd::run(cmd, SSH_TIMEOUT))
    }
}

struct Inner {
    probe: Arc<dyn TitanProbe>,
    snap: Mutex<TitanSnapshot>,
    ticker: Arc<Ticker>,
    health: Mutex<Health>,
    log: LogFile,
}

/// Der Hintergrund-Dienst.
pub struct TitanService {
    inner: Arc<Inner>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

fn now_unix() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

impl TitanService {
    /// Mit eigener Quelle (Tests: Attrappe). `log` bekommt nur die Wechsel zwischen
    /// erreichbar und nicht erreichbar.
    pub fn new(probe: Arc<dyn TitanProbe>, log: LogFile) -> TitanService {
        TitanService {
            inner: Arc::new(Inner {
                probe,
                snap: Mutex::new(TitanSnapshot::default()),
                ticker: Arc::new(Ticker::default()),
                health: Mutex::new(Health::new("Titan nicht erreichbar", "Titan wieder erreichbar")),
                log,
            }),
            thread: Mutex::new(None),
        }
    }

    /// Die echte Fassung über ssh.
    pub fn system(log: LogFile) -> TitanService {
        TitanService::new(Arc::new(SshProbe::system()), log)
    }

    /// Thread starten; `wake` nach jedem Abruf. Ein zweiter Aufruf tut nichts.
    pub fn start(&self, wake: Wake) {
        let mut t = lock(&self.thread);
        if t.is_some() {
            return;
        }
        let inner = self.inner.clone();
        *t = spawn("dash-titan", self.inner.ticker.clone(), Duration::from_secs(60), move || {
            let snap = poll(&inner);
            wake();
            Duration::from_secs(next_delay_s(&snap))
        });
    }

    /// Anhalten (ein laufender Abruf endet nach höchstens 15 s von selbst).
    pub fn stop(&self) {
        self.inner.ticker.stop();
        lock(&self.thread).take();
    }

    /// Sofort neu abrufen (im Hintergrund).
    pub fn refresh(&self) {
        self.inner.ticker.kick();
    }

    /// Lauf `name` beenden, aber nur, wenn der letzte Stand ihn als verwaist kennt; das
    /// Skript prüft das auf dem Titan noch einmal. Danach sofort neu nachsehen.
    pub fn kill_orphan(&self, name: &str) -> Result<u32, String> {
        let known = lock(&self.inner.snap).runs.iter().any(|r| r.name == name && r.orphan);
        if !known {
            return Err("kein verwaister Lauf mit diesem Namen".into());
        }
        let script = deck_core::dash::titan::kill_script(name).ok_or("ungültiger Name")?;
        let res = self.inner.probe.status(&script).and_then(|out| deck_core::dash::titan::parse_kill(&out));
        match &res {
            Ok(n) => self.inner.log.note(&format!("Lauf {name} beendet, {n} Prozesse")),
            Err(e) => self.inner.log.note(&format!("Lauf {name} nicht beendet: {e}")),
        }
        self.refresh();
        res
    }

    pub fn snapshot(&self) -> TitanSnapshot {
        lock(&self.inner.snap).clone()
    }

    /// Ein Abruf, synchron (der Thread macht genau das im Takt).
    pub fn poll_once(&self) -> TitanSnapshot {
        poll(&self.inner)
    }
}

impl Drop for TitanService {
    fn drop(&mut self) {
        self.stop();
    }
}

fn poll(inner: &Inner) -> TitanSnapshot {
    let now = now_unix();
    let result = inner.probe.status(STATUS_SH).and_then(|text| parse(&text, now));
    lock(&inner.health).record(&inner.log, result.as_ref().map(|_| ()).map_err(String::as_str));
    let mut snap = lock(&inner.snap);
    *snap = merge(&snap, result, now);
    snap.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logfile::Rotate;
    use std::collections::VecDeque;
    use std::sync::mpsc;

    /// Attrappe: liefert der Reihe nach die vorgegebenen Antworten, danach Fehler.
    struct FakeProbe(Mutex<VecDeque<Result<String, String>>>);

    impl TitanProbe for FakeProbe {
        fn status(&self, script: &str) -> Result<String, String> {
            assert!(script.ends_with("echo end=1"), "das Status-Skript wird geschickt");
            lock(&self.0).pop_front().unwrap_or_else(|| Err("leer".into()))
        }
    }

    const OK: &str = "cores=32\nload1=3.5\nmem_total_kb=4194304\nmem_avail_kb=2097152\ncontainers=2\nrun=agent-deck-320ef9\nend=1\n";

    fn service(answers: Vec<Result<String, String>>) -> (TitanService, tempfile::TempDir) {
        let d = tempfile::tempdir().unwrap();
        let log = LogFile::new(d.path().join("dash.log"), 100_000, Rotate::Rename);
        (TitanService::new(Arc::new(FakeProbe(Mutex::new(answers.into()))), log), d)
    }

    fn log_lines(d: &tempfile::TempDir) -> Vec<String> {
        std::fs::read_to_string(d.path().join("dash.log")).unwrap_or_default().lines().map(|l| l[20..].to_string()).collect()
    }

    #[test]
    fn a_successful_poll_fills_the_snapshot() {
        let (s, _d) = service(vec![Ok(OK.into())]);
        let snap = s.poll_once();
        assert!(snap.reachable && snap.last_ok.is_some());
        assert_eq!((snap.cores, snap.containers, snap.runs.len()), (32, 2, 1));
        assert_eq!(s.snapshot(), snap);
        assert_eq!(next_delay_s(&snap), 20, "mit Lauf schneller");
    }

    #[test]
    fn an_outage_keeps_the_last_values_and_is_logged_once_per_change() {
        let (s, d) = service(vec![Ok(OK.into()), Err("ssh: Exit 255 timeout".into()), Err("ssh: Exit 255".into()), Ok(OK.into())]);
        s.poll_once();
        let down = s.poll_once();
        assert!(!down.reachable);
        assert_eq!(down.cores, 32);
        assert_eq!(down.error.as_deref(), Some("ssh: Exit 255 timeout"));
        s.poll_once();
        assert!(s.poll_once().reachable);
        assert_eq!(log_lines(&d), ["Titan nicht erreichbar: ssh: Exit 255 timeout", "Titan wieder erreichbar nach 2 Fehlversuchen"]);
    }

    #[test]
    fn unreadable_output_counts_as_unreachable() {
        let (s, _d) = service(vec![Ok("cores=32\n".into())]);
        let snap = s.poll_once();
        assert!(!snap.reachable && snap.last_ok.is_none());
    }

    #[test]
    fn the_thread_polls_wakes_the_app_and_stops() {
        let (s, _d) = service(vec![Ok(OK.into())]);
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        s.start(Arc::new(move || {
            let _ = lock(&tx).send(());
        }));
        rx.recv_timeout(Duration::from_secs(5)).expect("wake nach dem ersten Abruf");
        assert!(s.snapshot().reachable);
        s.refresh();
        rx.recv_timeout(Duration::from_secs(5)).expect("refresh ruft sofort ab");
        assert!(!s.snapshot().reachable, "zweite Antwort der Attrappe ist ein Fehler");
        s.stop();
    }

    /// Echter Probelauf, nur lesend (ein ssh-Aufruf):
    /// `cargo test -p deck-services dash::titan::tests::real -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_titan_answers_the_status_script() {
        let probe = SshProbe::system();
        let t0 = std::time::Instant::now();
        let raw = probe.status(STATUS_SH);
        let snap = raw.and_then(|t| parse(&t, now_unix())).expect("Titan antwortet");
        println!(
            "{:.1} s: {} Kerne, Last {:.2}, RAM {:.0}/{:.0} GB, {} Container, VMs {:?}, Läufe {:?}",
            t0.elapsed().as_secs_f64(),
            snap.cores,
            snap.load1,
            snap.mem_avail_gb,
            snap.mem_total_gb,
            snap.containers,
            snap.vms,
            snap.runs
        );
        assert!(snap.cores > 0);
    }
}
