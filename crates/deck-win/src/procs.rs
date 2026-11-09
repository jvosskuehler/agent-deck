//! Laufende Prozesse: auflisten, nachsehen, beenden (aus `platform/procs.py`).
//!
//! Gebraucht für die Windows-Terminal-Agenten: welche Shell lebt noch (PID samt
//! Startzeit), und beim Schließen eines Agenten sein ganzer Prozessbaum.
//!
//! **„Weiß nicht“ ist nie „nein“.** Jede Abfrage, die scheitern kann, liefert dann `None`
//! und nicht die leere Liste: sonst sähe ein lebender Agent tot aus.
//!
//! Nachgesehen wird per Toolhelp-Snapshot (kein Subprozess je Blick, keine lokalisierte
//! Ausgabe). Beendet wird dagegen absichtlich mit `taskkill /F /T`: unter einem Prozess
//! hängt ein ganzer Baum, und den kennt Windows besser als eine selbstgebaute Suche.

use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, GetExitCodeProcess, GetProcessTimes, OpenProcess,
    PROCESS_QUERY_LIMITED_INFORMATION,
};

use crate::guard;
use crate::types::{Error, Result, from_wide};

/// Ein Eintrag der Prozessliste.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub parent_pid: u32,
    /// Exe-Name ohne Pfad, wie Windows ihn führt (`Code.exe`).
    pub exe: String,
    /// Startzeit als FILETIME (100-ns-Schritte seit 1601); `None` ohne Zugriffsrecht.
    pub start: Option<u64>,
}

/// Alle laufenden Prozesse samt Startzeit. `None` = konnte nicht nachsehen.
///
/// Die Startzeit macht aus einer PID einen Beweis: Windows vergibt PIDs neu, und eine
/// alte `.wtpid`-Datei zeigte sonst irgendwann auf einen fremden Prozess.
pub fn processes() -> Option<Vec<ProcessInfo>> {
    let mut list = snapshot()?;
    for p in &mut list {
        p.start = start_time(p.pid, false);
    }
    Some(list)
}

/// Startzeit eines LEBENDEN Prozesses (FILETIME), sonst `None` (beendet, unbekannt,
/// kein Zugriff).
pub fn process_start(pid: u32) -> Option<u64> {
    start_time(pid, true)
}

fn snapshot() -> Option<Vec<ProcessInfo>> {
    // SAFETY: Snapshot-Handle wird in jedem Fall geschlossen; dwSize ist gesetzt.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut e = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut out = Vec::new();
        let mut ok = Process32FirstW(snap, &mut e).is_ok();
        let readable = ok; // nicht einmal der Leerlaufprozess: Liste unlesbar
        while ok {
            out.push(ProcessInfo {
                pid: e.th32ProcessID,
                parent_pid: e.th32ParentProcessID,
                exe: from_wide(&e.szExeFile),
                start: None,
            });
            ok = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
        readable.then_some(out)
    }
}

fn start_time(pid: u32, require_alive: bool) -> Option<u64> {
    if pid == 0 {
        return None;
    }
    // SAFETY: Handle wird in jedem Fall geschlossen; Zeiger auf lokale Variablen.
    unsafe {
        let h: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let result = (|| {
            if require_alive {
                let mut code = 0u32;
                GetExitCodeProcess(h, &mut code).ok()?;
                if code != 259 {
                    return None; // 259 = STILL_ACTIVE
                }
            }
            let (mut c, mut e, mut k, mut u) = (
                FILETIME::default(),
                FILETIME::default(),
                FILETIME::default(),
                FILETIME::default(),
            );
            GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u).ok()?;
            Some((u64::from(c.dwHighDateTime) << 32) | u64::from(c.dwLowDateTime))
        })();
        let _ = CloseHandle(h);
        result
    }
}

/// Einen Prozess SAMT KINDERN beenden (`taskkill /F /T /PID`). Gedacht für die Shell
/// eines Agenten-Panes: darunter hängt `claude` und was es gerade laufen lässt.
/// PIDs ≤ 4 (Leerlauf, System) werden abgelehnt.
pub fn kill_tree(pid: u32) -> Result<()> {
    guard::check("kill_tree")?;
    if pid <= 4 {
        return Err(Error::Rejected(format!("PID {pid} ist ein Systemprozess")));
    }
    taskkill(&["/F", "/T", "/PID", &pid.to_string()])
}

/// taskkill-Exitcodes: 0 = beendet, 128 = kein solcher Prozess (aus Sicht des
/// Aufrufers dasselbe Ziel erreicht).
pub fn taskkill_succeeded(code: i32) -> bool {
    code == 0 || code == 128
}

const TASKKILL_TIMEOUT: Duration = Duration::from_secs(30);

fn taskkill(args: &[&str]) -> Result<()> {
    // Voller Pfad statt PATH-Suche: ein `taskkill.exe` im Arbeitsordner wäre sonst
    // ein fremdes Programm mit unseren Rechten.
    let exe = std::env::var_os("SystemRoot")
        .map(|r| {
            std::path::PathBuf::from(r)
                .join("System32")
                .join("taskkill.exe")
        })
        .unwrap_or_else(|| "taskkill.exe".into());
    let mut child = Command::new(exe)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW.0)
        .spawn()
        .map_err(|e| Error::Helper(format!("taskkill startet nicht: {e}")))?;
    let deadline = Instant::now() + TASKKILL_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let code = status.code().unwrap_or(-1);
                return if taskkill_succeeded(code) {
                    Ok(())
                } else {
                    Err(Error::Helper(format!(
                        "taskkill {args:?} endete mit {code}"
                    )))
                };
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                return Err(Error::Helper(format!(
                    "taskkill {args:?}: Zeitüberschreitung"
                )));
            }
            Err(e) => return Err(Error::Helper(format!("taskkill {args:?}: {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_process_counts_as_killed() {
        assert!(taskkill_succeeded(0));
        assert!(taskkill_succeeded(128));
        assert!(!taskkill_succeeded(1));
    }

    #[test]
    fn killing_is_blocked_in_tests() {
        assert!(matches!(kill_tree(12345), Err(Error::Forbidden(_))));
    }

    #[test]
    fn own_process_is_in_the_list_with_parent_and_start_time() {
        // Lesend gegen das echte System.
        let me = std::process::id();
        let list = processes().expect("Prozessliste lesbar");
        let entry = list
            .iter()
            .find(|p| p.pid == me)
            .expect("eigene PID gefunden");
        assert!(entry.exe.to_ascii_lowercase().ends_with(".exe"));
        assert_ne!(entry.parent_pid, 0);
        let start = process_start(me).expect("eigene Startzeit");
        assert_eq!(entry.start, Some(start));
    }

    #[test]
    fn a_dead_pid_has_no_start_time() {
        assert_eq!(process_start(0), None);
    }
}
