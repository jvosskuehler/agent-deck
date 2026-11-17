//! Eintrag unter „Apps“ in den Windows-Einstellungen (Apps & Features), ohne
//! Administratorrechte: `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\AgentDeck`.
//!
//! „Deinstallieren“ dort startet `bin\uninstall.ps1` in Windows PowerShell; der stille
//! Weg (`QuietUninstallString`, für Verwaltungswerkzeuge) fragt nichts und lässt die
//! Einstellungen des Decks stehen.

use std::path::Path;

use crate::installed::Installed;
use crate::report::{Grade, Report};
use crate::user_system::{RegValue, UNINSTALL_KEY, UserSystem};

pub const DISPLAY_NAME: &str = "Agent Deck";
pub const PUBLISHER: &str = "Agent Deck";

/// Die Werte des Eintrags, in fester Reihenfolge.
pub fn values(bin: &Path, version: &str, size_kb: u32) -> Vec<(&'static str, RegValue)> {
    let script = bin.join("uninstall.ps1");
    let run = format!("powershell.exe -NoProfile -ExecutionPolicy Bypass -File \"{}\"", script.display());
    let s = |v: String| RegValue::Str(v);
    vec![
        ("DisplayName", s(DISPLAY_NAME.into())),
        ("DisplayVersion", s(version.into())),
        ("Publisher", s(PUBLISHER.into())),
        ("DisplayIcon", s(bin.join("agent-deck.exe").to_string_lossy().into_owned())),
        ("InstallLocation", s(bin.to_string_lossy().into_owned())),
        ("UninstallString", s(run.clone())),
        ("QuietUninstallString", s(format!("{run} -Quiet -KeepData"))),
        ("NoModify", RegValue::Dword(1)),
        ("NoRepair", RegValue::Dword(1)),
        ("EstimatedSize", RegValue::Dword(size_kb)),
    ]
}

/// Größe der Dateien in `bin` in KB (so will Windows `EstimatedSize`).
pub fn size_kb(bin: &Path) -> u32 {
    let bytes: u64 = std::fs::read_dir(bin).into_iter().flatten().flatten().filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum();
    u32::try_from(bytes.div_ceil(1024)).unwrap_or(u32::MAX)
}

pub fn set(bin: &Path, version: &str, sys: &dyn UserSystem, m: &mut Installed) -> Report {
    let mut r = Report::default();
    for (name, v) in values(bin, version, size_kb(bin)) {
        if let Err(e) = sys.set(UNINSTALL_KEY, name, &v) {
            r.push(Grade::Warn, format!("Eintrag unter „Apps“ nicht angelegt: {e}"));
            return r;
        }
    }
    m.apps_entry = true;
    r.changed = true;
    r.push(Grade::Info, "Eintrag unter „Apps“ angelegt");
    r
}

/// Der Schlüssel gehört dem Deck (Name), er geht immer.
pub fn remove(sys: &dyn UserSystem, m: &mut Installed) -> Report {
    let mut r = Report::default();
    m.apps_entry = false;
    match sys.delete_key(UNINSTALL_KEY) {
        Ok(()) => r.push(Grade::Info, "Eintrag unter „Apps“ entfernt"),
        Err(e) => r.push(Grade::Warn, format!("Eintrag unter „Apps“ nicht entfernt: {e}")),
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::user_system::mem::MemSystem;

    #[test]
    fn the_apps_entry_uninstalls_with_the_script_in_bin_and_has_a_quiet_way() {
        let d = tempfile::tempdir().unwrap();
        let bin = d.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("agent-deck.exe"), vec![0u8; 3000]).unwrap();
        let sys = MemSystem::default();
        let mut m = Installed::default();
        assert!(set(&bin, "0.3.0", &sys, &mut m).is_ok());
        assert!(m.apps_entry);
        let get = |n: &str| sys.get(UNINSTALL_KEY, n).unwrap();
        assert_eq!(get("DisplayName"), RegValue::Str("Agent Deck".into()));
        assert_eq!(get("Publisher"), RegValue::Str("Agent Deck".into()));
        assert_eq!(get("DisplayVersion"), RegValue::Str("0.3.0".into()));
        let RegValue::Str(un) = get("UninstallString") else { panic!() };
        assert_eq!(un, format!("powershell.exe -NoProfile -ExecutionPolicy Bypass -File \"{}\"", bin.join("uninstall.ps1").display()));
        assert_eq!(get("QuietUninstallString"), RegValue::Str(format!("{un} -Quiet -KeepData")));
        assert_eq!(get("NoModify"), RegValue::Dword(1));
        assert_eq!(get("NoRepair"), RegValue::Dword(1));
        assert_eq!(get("EstimatedSize"), RegValue::Dword(3));
        assert!(remove(&sys, &mut m).is_ok());
        assert!(!sys.has_key(UNINSTALL_KEY) && !m.apps_entry);
    }
}
