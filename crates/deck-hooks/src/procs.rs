//! Die eigene Prozesskette (ich → Eltern → …) über einen Toolhelp32-Schnappschuss.
//!
//! Klein und privat hier, weil die Hooks schnell starten müssen und `deck-win` die
//! ganze Fensterwelt mitbrächte. Später mit der Prozessliste in `deck-win`
//! zusammenführen (eine Quelle für „wer ist mein Vorfahr“).

/// PID-Kette ab dem eigenen Prozess nach oben. Bei Problemen: leer.
#[cfg(windows)]
pub fn ancestor_pids() -> Vec<u32> {
    use std::collections::{HashMap, HashSet};

    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
    };

    let mut parent: HashMap<u32, u32> = HashMap::new();
    // SAFETY: Schnappschuss und Eintrag leben nur in diesem Block; dwSize ist gesetzt,
    // wie die API es verlangt, und der Handle wird genau einmal geschlossen.
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return Vec::new();
        };
        let mut e = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap, &mut e).is_ok();
        while ok {
            parent.insert(e.th32ProcessID, e.th32ParentProcessID);
            ok = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    let mut chain = Vec::new();
    let mut seen = HashSet::new();
    let mut pid = std::process::id();
    while pid != 0 && seen.insert(pid) {
        chain.push(pid);
        pid = parent.get(&pid).copied().unwrap_or(0);
    }
    chain
}

/// Außerhalb von Windows gibt es keine pidmap der Extension.
#[cfg(not(windows))]
pub fn ancestor_pids() -> Vec<u32> {
    Vec::new()
}
