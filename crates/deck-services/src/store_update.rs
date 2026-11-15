//! Windows Terminal und PowerShell aktualisieren, wenn es keinem Agenten schadet: beim
//! Start des Decks und nur, solange kein Agent läuft.
//!
//! Anlass (15.11.2026, 04:28): Der Microsoft Store hat PowerShell im Hintergrund
//! aktualisiert und dabei jede laufende `pwsh.exe` beendet, also jedes Agentenfenster.
//! Seitdem stehen die automatischen Store-Updates per Richtlinie aus
//! (`HKLM\SOFTWARE\Policies\Microsoft\WindowsStore`, `AutoDownload` = 2), und die beiden
//! Programme, in denen die Agenten leben, holt das Deck selbst nach.
//!
//! Wann: einige Zeit nach dem Start ([`START_DELAY`]), höchstens alle
//! [`MIN_INTERVAL_S`], und nur, wenn kein Agent lebt. Jedes Paket zusätzlich nur, wenn sein
//! Programm gerade gar nicht läuft (ein offenes Terminal oder eine pwsh, die jemand von
//! Hand gestartet hat, würde das Update sonst ebenso beenden). Was übersprungen wird,
//! kommt beim nächsten Start dran.
//!
//! Aktualisiert wird über `winget` aus der Quelle `msstore`: derselbe Kanal wie der Store,
//! ohne Adminrechte (die Pakete sind je Benutzer installiert).

use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use deck_core::paths::{load_json, save_json};
use deck_win::which::which;
use serde_json::json;

use crate::guard;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Ein Store-Paket, das das Deck selbst aktualisiert.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Package {
    /// Kennung im Microsoft Store (`winget --source msstore`).
    pub id: &'static str,
    pub name: &'static str,
    /// Läuft dieses Programm, wird das Paket übersprungen.
    pub exe: &'static str,
}

pub const PACKAGES: [Package; 2] = [
    Package { id: "9MZ1SNWT0N5D", name: "PowerShell", exe: "pwsh.exe" },
    Package { id: "9N0DX20HK701", name: "Windows Terminal", exe: "WindowsTerminal.exe" },
];

/// `winget upgrade` ohne neuere Fassung (`APPINSTALLER_CLI_ERROR_UPDATE_NOT_APPLICABLE`,
/// gemessen 15.11.2026).
pub const NO_UPDATE: i32 = 0x8A15_002B_u32 as i32;
pub const START_DELAY: Duration = Duration::from_secs(60);
pub const MIN_INTERVAL_S: f64 = 12.0 * 3600.0;
pub const TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// Wann zuletzt alle Pakete geprüft wurden (App-Ordner).
pub const MEMO_FILE: &str = "store-update.json";

/// Führt `winget upgrade` für ein Paket aus und liefert den Exit-Code.
pub trait Upgrader: Send + Sync {
    fn upgrade(&self, id: &str, timeout: Duration) -> Result<i32, String>;
}

/// Was gerade läuft.
pub trait Probe {
    /// Lebende Agenten des Decks.
    fn agents_alive(&self) -> usize;
    /// Exe-Namen aller laufenden Prozesse; `None` = unbekannt.
    fn running_exes(&self) -> Option<Vec<String>>;
}

/// Die echte Fassung: `winget upgrade --id <id> --source msstore`, still und ohne Fenster.
#[derive(Debug, Default, Clone, Copy)]
pub struct Winget;

impl Upgrader for Winget {
    fn upgrade(&self, id: &str, timeout: Duration) -> Result<i32, String> {
        guard::check("winget aufrufen").map_err(|e| e.to_string())?;
        let exe = which("winget").ok_or("winget nicht gefunden")?;
        let mut child = Command::new(exe)
            .args(["upgrade", "--id", id, "--exact", "--source", "msstore", "--silent"])
            .args(["--accept-package-agreements", "--accept-source-agreements", "--disable-interactivity"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("Start fehlgeschlagen: {}", e.kind()))?;
        let deadline = Instant::now() + timeout;
        loop {
            match child.try_wait() {
                Ok(Some(s)) => return s.code().ok_or_else(|| "ohne Exit-Code beendet".into()),
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(500)),
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("nach {} s abgebrochen", timeout.as_secs()));
                }
                Err(e) => return Err(format!("Warten fehlgeschlagen: {}", e.kind())),
            }
        }
    }
}

/// Ist ein Lauf fällig? `last` = Zeitpunkt des letzten vollständigen Laufs.
pub fn due(last: Option<f64>, now: f64) -> bool {
    last.is_none_or(|t| now - t >= MIN_INTERVAL_S || now < t)
}

/// Exit-Code von winget, lesbar.
pub fn outcome_text(code: i32) -> String {
    match code {
        0 => "aktualisiert".into(),
        NO_UPDATE => "schon aktuell".into(),
        c => format!("Fehler 0x{:08X}", c as u32),
    }
}

/// Warum ein Paket jetzt nicht dran ist, oder `None`.
fn blocker(pkg: &Package, probe: &dyn Probe) -> Option<String> {
    let agents = probe.agents_alive();
    if agents > 0 {
        return Some(format!("{agents} Agenten laufen"));
    }
    match probe.running_exes() {
        None => Some("Prozessliste nicht lesbar".into()),
        Some(exes) if exes.iter().any(|e| e.eq_ignore_ascii_case(pkg.exe)) => Some(format!("{} läuft", pkg.exe)),
        Some(_) => None,
    }
}

/// Ein Durchgang. Jede Zeile für das Protokoll geht an `log`.
pub fn run_once(up: &dyn Upgrader, probe: &dyn Probe, app_dir: &Path, now: f64, log: &mut dyn FnMut(String)) {
    let memo = app_dir.join(MEMO_FILE);
    let last = load_json(&memo).and_then(|v| v.get("ts").and_then(|t| t.as_f64()));
    if !due(last, now) {
        return;
    }
    let agents = probe.agents_alive();
    if agents > 0 {
        log(format!("Store-Update übersprungen: {agents} Agenten laufen, nächster Versuch beim nächsten Start"));
        return;
    }
    let mut complete = true;
    for pkg in &PACKAGES {
        if let Some(why) = blocker(pkg, probe) {
            log(format!("Store-Update {}: übersprungen ({why})", pkg.name));
            complete = false;
            continue;
        }
        let t0 = Instant::now();
        match up.upgrade(pkg.id, TIMEOUT) {
            Ok(code) => {
                log(format!("Store-Update {}: {} ({} s)", pkg.name, outcome_text(code), t0.elapsed().as_secs()));
                complete &= code == 0 || code == NO_UPDATE;
            }
            Err(e) => {
                log(format!("Store-Update {}: {e}", pkg.name));
                complete = false;
            }
        }
    }
    if complete {
        let _ = save_json(&memo, &json!({ "ts": now }));
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    struct FakeUp {
        calls: Mutex<Vec<String>>,
        code: i32,
    }
    impl Upgrader for FakeUp {
        fn upgrade(&self, id: &str, _: Duration) -> Result<i32, String> {
            self.calls.lock().unwrap().push(id.into());
            Ok(self.code)
        }
    }

    struct FakeProbe {
        agents: Cell<usize>,
        exes: Option<Vec<String>>,
    }
    impl Probe for FakeProbe {
        fn agents_alive(&self) -> usize {
            self.agents.get()
        }
        fn running_exes(&self) -> Option<Vec<String>> {
            self.exes.clone()
        }
    }

    fn probe(agents: usize, exes: &[&str]) -> FakeProbe {
        FakeProbe { agents: Cell::new(agents), exes: Some(exes.iter().map(|s| s.to_string()).collect()) }
    }

    fn run(up: &FakeUp, p: &FakeProbe, dir: &Path, now: f64) -> Vec<String> {
        let mut lines = Vec::new();
        run_once(up, p, dir, now, &mut |l| lines.push(l));
        lines
    }

    #[test]
    fn nothing_is_updated_while_an_agent_lives() {
        let dir = tempfile::tempdir().unwrap();
        let up = FakeUp::default();
        let lines = run(&up, &probe(2, &[]), dir.path(), 1000.0);
        assert!(up.calls.lock().unwrap().is_empty());
        assert_eq!(lines, ["Store-Update übersprungen: 2 Agenten laufen, nächster Versuch beim nächsten Start"]);
        assert!(!dir.path().join(MEMO_FILE).exists(), "nächster Start versucht es wieder");
    }

    #[test]
    fn a_running_program_skips_only_its_own_package() {
        let dir = tempfile::tempdir().unwrap();
        let up = FakeUp { code: NO_UPDATE, ..FakeUp::default() };
        let lines = run(&up, &probe(0, &["explorer.exe", "PWSH.EXE"]), dir.path(), 1000.0);
        assert_eq!(*up.calls.lock().unwrap(), ["9N0DX20HK701"]);
        assert_eq!(lines[0], "Store-Update PowerShell: übersprungen (pwsh.exe läuft)");
        assert!(lines[1].starts_with("Store-Update Windows Terminal: schon aktuell"), "{lines:?}");
        assert!(!dir.path().join(MEMO_FILE).exists());
    }

    #[test]
    fn a_complete_run_is_remembered_for_twelve_hours() {
        let dir = tempfile::tempdir().unwrap();
        let up = FakeUp::default();
        let lines = run(&up, &probe(0, &[]), dir.path(), 1000.0);
        assert_eq!(up.calls.lock().unwrap().len(), 2);
        assert!(lines.iter().all(|l| l.contains("aktualisiert")), "{lines:?}");
        assert!(run(&up, &probe(0, &[]), dir.path(), 1000.0 + MIN_INTERVAL_S - 1.0).is_empty());
        assert_eq!(up.calls.lock().unwrap().len(), 2);
        run(&up, &probe(0, &[]), dir.path(), 1000.0 + MIN_INTERVAL_S);
        assert_eq!(up.calls.lock().unwrap().len(), 4);
    }

    #[test]
    fn an_unreadable_process_list_updates_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let up = FakeUp::default();
        let p = FakeProbe { agents: Cell::new(0), exes: None };
        let lines = run(&up, &p, dir.path(), 1000.0);
        assert!(up.calls.lock().unwrap().is_empty());
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn winget_codes_read_as_words() {
        assert_eq!(outcome_text(0), "aktualisiert");
        assert_eq!(outcome_text(NO_UPDATE), "schon aktuell");
        assert_eq!(outcome_text(0x8A15_0014_u32 as i32), "Fehler 0x8A150014");
    }

    #[test]
    fn the_real_winget_is_locked_in_tests() {
        assert!(Winget.upgrade("9MZ1SNWT0N5D", Duration::from_secs(1)).unwrap_err().contains("gesperrt"));
    }
}
