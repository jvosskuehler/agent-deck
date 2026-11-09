//! Laufende Prozesse: auflisten, nachsehen, beenden (aus `platform/procs.py`).
//!
//! Gebraucht vom VS-Code-Neustart: ein geschlossenes Fenster beweist nichts. VS Code
//! räumt danach noch Renderer, Extension-Hosts und den pty-Host ab, und in dieser Lücke
//! würde ein neu gestartetes `Code.exe` sich beim sterbenden Prozess anmelden statt eine
//! frische Instanz zu werden. Darum wird nicht gewartet, sondern nachgesehen.
//!
//! **„Weiß nicht“ ist nie „nein“.** Jede Abfrage, die scheitern kann, liefert dann `None`
//! und nicht die leere Liste bzw. 0: sonst startet der Neustart in einen noch lebenden
//! VS Code hinein.
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

/// Nur die Exe-Namen (billiger als [`processes`], ohne Startzeiten).
pub fn exe_names() -> Option<Vec<String>> {
    snapshot().map(|l| l.into_iter().map(|p| p.exe).collect())
}

/// Wie viele Prozesse dieses Exe-Namens laufen (ohne Groß-/Kleinschreibung).
/// `None` = konnte nicht nachsehen; leerer Name = 0.
pub fn count_exe(name: &str) -> Option<usize> {
    if name.is_empty() {
        return Some(0);
    }
    let names = exe_names()?;
    Some(count_in(&names, name))
}

/// „Läuft noch ein `name`?“ Drei Antworten: ja, nein, weiß nicht (`None`).
pub fn is_running(name: &str) -> Option<bool> {
    count_exe(name).map(|n| n > 0)
}

/// Zählung ohne Groß-/Kleinschreibung, ausgelagert für Tests.
pub fn count_in(names: &[String], name: &str) -> usize {
    names
        .iter()
        .filter(|n| n.eq_ignore_ascii_case(name))
        .count()
}

/// Startzeit eines LEBENDEN Prozesses (FILETIME), sonst `None` (beendet, unbekannt,
/// kein Zugriff).
pub fn process_start(pid: u32) -> Option<u64> {
    start_time(pid, true)
}

/// FILETIME → Unix-Sekunden.
pub fn filetime_to_unix(ft: u64) -> f64 {
    const EPOCH_DIFF: u64 = 116_444_736_000_000_000; // 1601 → 1970 in 100 ns
    (ft as f64 - EPOCH_DIFF as f64) / 10_000_000.0
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

/// Alle Prozesse dieses Exe-Namens samt Kindern hart beenden. Die Notbremse des
/// Neustarts: unter VS Code hängen Renderer, Extension-Hosts, der pty-Host und die
/// Claude-Sessions (die danach per `claude --resume` zurückkommen). Ein einzeln
/// abgeschossener Hauptprozess ließe die Kinder als Waisen zurück.
///
/// Der Name muss ein schlichter Exe-Name sein ([`is_plain_exe_name`]): er geht als
/// Argument in eine Kommandozeile, und ein `/F` oder `*` darin wäre ein anderer Befehl.
pub fn kill_by_name(name: &str) -> Result<()> {
    guard::check("kill_by_name")?;
    if !is_plain_exe_name(name) {
        return Err(Error::Rejected(format!(
            "kein schlichter Exe-Name: {name:?}"
        )));
    }
    taskkill(&["/F", "/T", "/IM", name])
}

/// `^[A-Za-z0-9 ._-]+\.exe$`, ohne Groß-/Kleinschreibung der Endung.
pub fn is_plain_exe_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let Some(stem) = lower.strip_suffix(".exe") else {
        return false;
    };
    !stem.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b' ' | b'.' | b'_' | b'-'))
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
    fn only_plain_exe_names_reach_taskkill() {
        assert!(is_plain_exe_name("Code.exe"));
        assert!(is_plain_exe_name("Code - Insiders.EXE"));
        assert!(!is_plain_exe_name("*.exe"));
        assert!(!is_plain_exe_name("/F.exe"));
        assert!(!is_plain_exe_name("Code"));
        assert!(!is_plain_exe_name(".exe"));
        assert!(!is_plain_exe_name("a\"b.exe"));
    }

    #[test]
    fn missing_process_counts_as_killed() {
        assert!(taskkill_succeeded(0));
        assert!(taskkill_succeeded(128));
        assert!(!taskkill_succeeded(1));
    }

    #[test]
    fn exe_count_ignores_case_and_empty_name_is_zero() {
        let names = vec!["Code.exe".to_string(), "code.EXE".into(), "pwsh.exe".into()];
        assert_eq!(count_in(&names, "CODE.exe"), 2);
        assert_eq!(count_exe(""), Some(0));
    }

    #[test]
    fn filetime_epoch_maps_to_unix_zero() {
        assert_eq!(filetime_to_unix(116_444_736_000_000_000), 0.0);
        assert_eq!(filetime_to_unix(116_444_736_000_000_000 + 15_000_000), 1.5);
    }

    #[test]
    fn killing_is_blocked_in_tests() {
        assert!(matches!(kill_tree(12345), Err(Error::Forbidden(_))));
        assert!(matches!(kill_by_name("Code.exe"), Err(Error::Forbidden(_))));
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
        let unix = filetime_to_unix(start);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        assert!(
            unix <= now && now - unix < 24.0 * 3600.0,
            "Start liegt in den letzten 24 h"
        );
    }

    #[test]
    fn a_dead_pid_has_no_start_time() {
        assert_eq!(process_start(0), None);
    }
}
