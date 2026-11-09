//! Prozesse genauer ansehen, auf ihr Ende warten und gezielt beenden. Gebraucht für
//! „Läuft noch ohne Fenster“ (Regeln in `deck_core::orphans`, Dienst in
//! `deck_services::orphans`).
//!
//! **PID und Startzeit gehören zusammen.** Windows vergibt PIDs neu; jede Funktion hier
//! bekommt darum beides und prüft die Startzeit am SELBEN Handle, mit dem sie danach
//! liest oder beendet. Zwischen Prüfen und Beenden kann so kein fremder Prozess unter die
//! PID rutschen, anders als bei `taskkill /PID`, das nur die Nummer kennt (und mit `/T`
//! sogar Kinder eines früheren Trägers der Nummer mitnähme).
//!
//! Die Kommandozeile kommt aus `NtQueryInformationProcess(ProcessCommandLineInformation)`
//! (seit Windows 8.1, braucht nur `PROCESS_QUERY_LIMITED_INFORMATION`): kein WMI, kein
//! Hilfsprozess, gemessen wenige Mikrosekunden je Prozess.

use windows::Wdk::System::Threading::{NtQueryInformationProcess, ProcessCommandLineInformation};
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, UNICODE_STRING, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::{
    GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_ACCESS_RIGHTS, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, QueryFullProcessImageNameW,
    TerminateProcess, WaitForMultipleObjects, WaitForSingleObject,
};
use windows::core::PWSTR;

use crate::guard;
use crate::types::{Error, Result};

/// Was über einen Prozess nur auf Nachfrage gelesen wird (kostet je Prozess einen Handle).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProcDetails {
    /// Volle Kommandozeile; `None` ohne Zugriff.
    pub cmdline: Option<String>,
    /// Voller Pfad der Exe; `None` ohne Zugriff.
    pub image: Option<String>,
    /// Windows-Sitzung (0 = Dienste); `None` unbekannt.
    pub session: Option<u32>,
}

/// Ist das „Zugriff verweigert“ (erhöhter Prozess, anderes Konto)?
pub fn is_access_denied(e: &Error) -> bool {
    matches!(e, Error::Win32 { code: E_ACCESS_DENIED, .. })
}

/// Ausgang von [`terminate_exact`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Terminated {
    /// Beendet und das Ende abgewartet.
    Killed,
    /// Lief schon nicht mehr (oder die PID gehört inzwischen einem anderen Prozess).
    AlreadyGone,
}

/// HRESULT von `ERROR_ACCESS_DENIED`.
const E_ACCESS_DENIED: u32 = 0x8007_0005;
/// `GetExitCodeProcess` meldet so einen laufenden Prozess.
const STILL_ACTIVE: u32 = 259;
/// So lange wartet [`terminate_exact`] auf das Ende nach `TerminateProcess` (ms).
const TERMINATE_WAIT_MS: u32 = 5_000;

/// Ein Prozess-Handle, das sich selbst schließt.
struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: das Handle stammt aus OpenProcess und wird genau hier geschlossen.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn open(pid: u32, access: PROCESS_ACCESS_RIGHTS) -> std::result::Result<Handle, windows::core::Error> {
    // SAFETY: reiner Aufruf, das Ergebnis wird in Handle verpackt und geschlossen.
    unsafe { OpenProcess(access, false, pid).map(Handle) }
}

fn created(h: &Handle) -> Option<u64> {
    let (mut c, mut e, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    // SAFETY: Zeiger auf lokale Variablen, gültiges Handle.
    unsafe { GetProcessTimes(h.0, &mut c, &mut e, &mut k, &mut u).ok()? };
    Some((u64::from(c.dwHighDateTime) << 32) | u64::from(c.dwLowDateTime))
}

fn running(h: &Handle) -> bool {
    let mut code = 0u32;
    // SAFETY: Zeiger auf lokale Variable, gültiges Handle.
    unsafe { GetExitCodeProcess(h.0, &mut code).is_ok() && code == STILL_ACTIVE }
}

/// Kommandozeile, Exe-Pfad und Sitzung eines LEBENDEN Prozesses, wenn er wirklich der mit
/// dieser Startzeit ist; sonst `None` (beendet, PID neu vergeben, kein Zugriff).
pub fn details(pid: u32, start: u64) -> Option<ProcDetails> {
    if pid == 0 {
        return None;
    }
    let h = open(pid, PROCESS_QUERY_LIMITED_INFORMATION).ok()?;
    if created(&h)? != start || !running(&h) {
        return None;
    }
    Some(ProcDetails { cmdline: command_line(&h), image: image_path(&h), session: session_of(pid) })
}

fn command_line(h: &Handle) -> Option<String> {
    let mut len = 0u32;
    // SAFETY: erst nach der Länge fragen (Fehler „zu klein“ ist hier gewollt), dann in
    // einen ausgerichteten Puffer dieser Länge lesen. Der Puffer beginnt mit einem
    // UNICODE_STRING, dessen Zeiger in denselben Puffer zeigt.
    unsafe {
        let _ = NtQueryInformationProcess(h.0, ProcessCommandLineInformation, std::ptr::null_mut(), 0, &mut len);
        if (len as usize) < size_of::<UNICODE_STRING>() || len > 1 << 20 {
            return None;
        }
        let mut buf = vec![0u64; (len as usize).div_ceil(8)];
        let status = NtQueryInformationProcess(h.0, ProcessCommandLineInformation, buf.as_mut_ptr().cast(), len, &mut len);
        if status.0 < 0 {
            return None;
        }
        let us = &*buf.as_ptr().cast::<UNICODE_STRING>();
        if us.Buffer.is_null() || us.Length == 0 {
            return Some(String::new());
        }
        let wide = std::slice::from_raw_parts(us.Buffer.0, usize::from(us.Length) / 2);
        Some(String::from_utf16_lossy(wide))
    }
}

fn image_path(h: &Handle) -> Option<String> {
    let mut buf = vec![0u16; 1024];
    let mut size = buf.len() as u32;
    // SAFETY: Puffer und Länge passen zusammen, gültiges Handle.
    unsafe { QueryFullProcessImageNameW(h.0, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut size).ok()? };
    Some(String::from_utf16_lossy(&buf[..size as usize]))
}

fn session_of(pid: u32) -> Option<u32> {
    let mut s = 0u32;
    // SAFETY: Zeiger auf lokale Variable.
    unsafe { ProcessIdToSessionId(pid, &mut s).ok()? };
    Some(s)
}

/// Den Prozess mit GENAU dieser PID und Startzeit beenden und auf sein Ende warten.
///
/// Prüfen und Beenden laufen über dasselbe Handle: es gibt kein Zeitfenster, in dem die
/// PID den Besitzer wechseln könnte. Beendet wird nur dieser eine Prozess, nicht sein
/// Baum; welche Kinder dazugehören, entscheidet der Aufrufer anhand von PID UND Startzeit.
/// PIDs ≤ 4 (Leerlauf, System) werden abgelehnt.
pub fn terminate_exact(pid: u32, start: u64) -> Result<Terminated> {
    guard::check("terminate_exact")?;
    if pid <= 4 {
        return Err(Error::Rejected(format!("PID {pid} ist ein Systemprozess")));
    }
    let h = match open(pid, PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE) {
        Ok(h) => h,
        Err(e) => {
            // Kein Zugriff heißt nicht „weg“: nur wenn der Prozess auch lesend nicht mehr
            // mit dieser Startzeit zu finden ist, ist das Ziel erreicht.
            let still_there = open(pid, PROCESS_QUERY_LIMITED_INFORMATION)
                .ok()
                .is_some_and(|q| created(&q) == Some(start) && running(&q));
            if !still_there {
                return Ok(Terminated::AlreadyGone);
            }
            // Kein Zugriff (erhöht oder anderes Konto) kommt als Win32-Fehler mit
            // `E_ACCESS_DENIED` zurück; der Aufrufer erkennt ihn an [`is_access_denied`].
            return Err(Error::win("OpenProcess", e));
        }
    };
    if created(&h) != Some(start) || !running(&h) {
        return Ok(Terminated::AlreadyGone);
    }
    // SAFETY: gültiges Handle mit PROCESS_TERMINATE.
    unsafe { TerminateProcess(h.0, 1) }.map_err(|e| Error::win("TerminateProcess", e))?;
    // SAFETY: gültiges Handle mit SYNCHRONIZE.
    let waited = unsafe { WaitForSingleObject(h.0, TERMINATE_WAIT_MS) };
    if waited == WAIT_OBJECT_0 {
        Ok(Terminated::Killed)
    } else {
        Err(Error::Helper(format!("PID {pid} endet nicht")))
    }
}

/// Warten, bis einer dieser Prozesse endet, höchstens `timeout_ms`. `true` = einer ist
/// zu Ende (oder lief schon nicht mehr), `false` = Zeit abgelaufen oder nichts zu warten.
///
/// So bemerkt der Dienst das Schließen eines Agentenfensters sofort und schaut nach,
/// solange die Kinder der Shell noch ihre Eltern-PID tragen.
pub fn wait_any_exit(procs: &[(u32, u64)], timeout_ms: u32) -> bool {
    let mut handles = Vec::new();
    for &(pid, start) in procs.iter().take(64) {
        match open(pid, PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION) {
            Ok(h) if created(&h) == Some(start) && running(&h) => handles.push(h),
            // Schon weg (oder PID neu vergeben): das ist ein Ende.
            Ok(_) => return true,
            Err(e) if e.code().0 as u32 == E_ACCESS_DENIED => {}
            Err(_) => return true,
        }
    }
    if handles.is_empty() {
        std::thread::sleep(std::time::Duration::from_millis(u64::from(timeout_ms)));
        return false;
    }
    let raw: Vec<HANDLE> = handles.iter().map(|h| h.0).collect();
    // SAFETY: höchstens 64 gültige Handles mit SYNCHRONIZE; sie leben bis nach dem Aufruf.
    let r = unsafe { WaitForMultipleObjects(&raw, false, timeout_ms) };
    r != WAIT_TIMEOUT && r.0 < WAIT_OBJECT_0.0 + raw.len() as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::procs::process_start;

    #[test]
    fn own_process_has_command_line_image_and_session() {
        let me = std::process::id();
        let start = process_start(me).expect("eigene Startzeit");
        let d = details(me, start).expect("eigene Details");
        let image = d.image.expect("Exe-Pfad");
        assert!(image.to_ascii_lowercase().ends_with(".exe"), "{image}");
        let cmd = d.cmdline.expect("Kommandozeile");
        let stem = std::path::Path::new(&image).file_stem().unwrap().to_string_lossy().to_string();
        assert!(cmd.contains(&stem), "{cmd} enthält {stem}");
        assert!(d.session.is_some_and(|s| s > 0), "Testprozess läuft nicht in Sitzung 0");
    }

    #[test]
    fn a_wrong_start_time_is_another_process() {
        let me = std::process::id();
        let start = process_start(me).unwrap();
        assert_eq!(details(me, start + 1), None);
        assert_eq!(details(0, start), None);
    }

    #[test]
    fn terminating_is_blocked_in_tests() {
        assert!(matches!(terminate_exact(12345, 1), Err(Error::Forbidden(_))));
    }

    #[test]
    fn waiting_on_a_process_with_wrong_start_returns_at_once() {
        let me = std::process::id();
        let start = process_start(me).unwrap();
        assert!(wait_any_exit(&[(me, start + 1)], 5_000));
        assert!(!wait_any_exit(&[(me, start)], 10));
    }
}
