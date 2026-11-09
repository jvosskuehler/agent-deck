//! Die Umgebung eines fremden Prozesses lesen, nur lesend. Gebraucht für die
//! Herkunftsmarke von „Läuft noch ohne Fenster“ (`deck_core::orphans::marker`).
//!
//! Windows hält die Umgebung im Speicher des Prozesses: PEB → `ProcessParameters` →
//! `Environment` (Block aus `NAME=wert\0…\0\0`, UTF-16) samt `EnvironmentSize`. Gelesen wird
//! über `NtQueryInformationProcess(ProcessBasicInformation)` und `ReadProcessMemory`; das
//! braucht `PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ`, also nur Prozesse desselben
//! Kontos ohne Erhöhung. Die Startzeit wird am selben Handle geprüft (PID neu vergeben).
//!
//! Die Versätze gelten für 64-Bit-Windows (das Deck läuft nur dort) und seit Windows 8;
//! auch ein 32-Bit-Prozess hat diesen 64-Bit-PEB.

use windows::Wdk::System::Threading::{NtQueryInformationProcess, ProcessBasicInformation};
use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows::Win32::System::Threading::{PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ};

use crate::proc_scan::{Handle, created, open, running};

/// `PEB.ProcessParameters`.
const PEB_PARAMS: usize = 0x20;
/// `RTL_USER_PROCESS_PARAMETERS.Environment`.
const PARAMS_ENV: usize = 0x80;
/// `RTL_USER_PROCESS_PARAMETERS.EnvironmentSize` (Bytes).
const PARAMS_ENV_SIZE: usize = 0x3F0;
/// Größer ist eine Umgebung nie (32 767 Zeichen je Variable, der Block in der Praxis
/// wenige KB); mehr wird nicht gelesen.
const MAX_ENV_BYTES: usize = 512 * 1024;

/// `PROCESS_BASIC_INFORMATION` (x64), selbst beschrieben, damit kein weiteres Feature des
/// windows-Crates nötig ist.
#[repr(C)]
#[derive(Default)]
struct BasicInfo {
    exit_status: i32,
    peb: usize,
    affinity: usize,
    base_priority: i32,
    pid: usize,
    parent: usize,
}

/// Einen Wert aus dem Speicher des Prozesses lesen.
fn read<T: Copy + Default>(h: &Handle, addr: usize) -> Option<T> {
    let mut v = T::default();
    let mut n = 0usize;
    // SAFETY: Ziel ist eine lokale Variable der gelesenen Größe; das Handle hat VM_READ.
    unsafe { ReadProcessMemory(h.0, addr as *const _, std::ptr::from_mut(&mut v).cast(), size_of::<T>(), Some(&mut n)) }.ok()?;
    (n == size_of::<T>()).then_some(v)
}

/// Den Umgebungsblock des LEBENDEN Prozesses mit dieser Startzeit, Einträge durch `\0`
/// getrennt; `None` ohne Zugriff, bei falscher Startzeit oder wenn etwas nicht passt.
pub fn environment(pid: u32, start: u64) -> Option<String> {
    if pid <= 4 {
        return None;
    }
    let h = open(pid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ).ok()?;
    if created(&h)? != start || !running(&h) {
        return None;
    }
    let mut info = BasicInfo::default();
    let mut len = 0u32;
    // SAFETY: Ziel ist die lokale Struktur mit ihrer Größe; gültiges Handle.
    let status = unsafe {
        NtQueryInformationProcess(h.0, ProcessBasicInformation, std::ptr::from_mut(&mut info).cast(), size_of::<BasicInfo>() as u32, &mut len)
    };
    if status.0 < 0 || info.peb == 0 {
        return None;
    }
    let params: usize = read(&h, info.peb + PEB_PARAMS)?;
    let env: usize = read(&h, params + PARAMS_ENV)?;
    let size: usize = read(&h, params + PARAMS_ENV_SIZE)?;
    if env == 0 || size < 4 {
        return None;
    }
    let mut buf = vec![0u16; size.min(MAX_ENV_BYTES) / 2];
    let mut n = 0usize;
    // SAFETY: Puffer und Länge passen zusammen; das Handle hat VM_READ.
    unsafe { ReadProcessMemory(h.0, env as *const _, buf.as_mut_ptr().cast(), buf.len() * 2, Some(&mut n)) }.ok()?;
    buf.truncate(n / 2);
    // Der Block endet mit einem leeren Eintrag (`\0\0`); danach kommt nichts mehr.
    let end = buf.windows(2).position(|w| w == [0, 0]).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..end]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::procs::process_start;

    #[test]
    fn the_own_environment_reads_like_the_standard_library_sees_it() {
        let me = std::process::id();
        let block = environment(me, process_start(me).unwrap()).expect("eigene Umgebung");
        let (name, value) = std::env::vars().find(|(k, _)| k.eq_ignore_ascii_case("PATH")).expect("PATH");
        assert!(block.split('\0').any(|e| e == format!("{name}={value}")), "PATH steht im Block");
    }

    #[test]
    fn a_wrong_start_time_or_a_system_pid_reads_nothing() {
        let me = std::process::id();
        assert_eq!(environment(me, process_start(me).unwrap() + 1), None);
        assert_eq!(environment(4, 0), None);
    }
}
