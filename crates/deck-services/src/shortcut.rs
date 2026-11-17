//! Startmenü-Verknüpfung „Agent Deck“, damit die Windows-Suche das Deck findet.
//!
//! Liegt in `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Agent Deck.lnk`, zeigt auf
//! `bin\agent-deck.exe` (Arbeitsordner `bin`, Symbol aus der Exe) und trägt die
//! App-Kennung [`APP_ID`] (`System.AppUserModel.ID`). Dieselbe Kennung muss die laufende
//! App für sich setzen (`SetCurrentProcessExplicitAppUserModelID`), sonst ordnet die
//! Taskleiste das Fenster nicht dieser Verknüpfung zu (Anheften, Symbol).
//!
//! Gebaut mit `IShellLinkW` + `IPropertyStore` + `IPersistFile`; ein zweiter Lauf
//! überschreibt die Datei mit demselben Inhalt.

use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{E_OUTOFMEMORY, PROPERTYKEY, RPC_E_CHANGED_MODE};
use windows::Win32::System::Com::StructuredStorage::{PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0, PropVariantClear};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemAlloc, CoUninitialize, IPersistFile, STGM_READ,
};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
use windows::core::{GUID, HSTRING, Interface, PWSTR};

use crate::fsutil::normcase;
use crate::guard;
use crate::report::{Grade, Report};

/// App-Kennung des Decks: der `identifier` aus `app/src-tauri/tauri.conf.json`.
pub const APP_ID: &str = "de.idm.agentdeck";
pub const LINK_NAME: &str = "Agent Deck.lnk";
const DESCRIPTION: &str = "Dashboard für parallel laufende Claude-Code-Agenten";

/// `PKEY_AppUserModel_ID` (propkey.h).
const KEY_APP_ID: PROPERTYKEY = PROPERTYKEY { fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3), pid: 5 };

/// `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Agent Deck.lnk`.
pub fn link_path() -> PathBuf {
    let roaming = std::env::var_os("APPDATA")
        .filter(|v| !v.is_empty())
        .map_or_else(|| deck_core::paths::home_dir().join("AppData").join("Roaming"), PathBuf::from);
    roaming.join("Microsoft").join("Windows").join("Start Menu").join("Programs").join(LINK_NAME)
}

/// Die echte Verknüpfung ist in Tests gesperrt; Wegwerf-Ordner sind frei.
fn check_target(link: &Path) -> std::io::Result<()> {
    if normcase(&link.to_string_lossy()) == normcase(&link_path().to_string_lossy()) {
        guard::check("Startmenü-Verknüpfung ändern")?;
    }
    Ok(())
}

/// COM für die Dauer von `f`. Wer COM schon anders eingerichtet hat, bekommt kein
/// `CoUninitialize` von uns.
fn with_com<T>(f: impl FnOnce() -> windows::core::Result<T>) -> windows::core::Result<T> {
    // SAFETY: Gegenstück unten nur nach eigenem Erfolg.
    let init = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let ours = init.is_ok();
    if !ours && init != RPC_E_CHANGED_MODE {
        return Err(init.into());
    }
    let out = f();
    if ours {
        // SAFETY: Gegenstück zum erfolgreichen CoInitializeEx oben.
        unsafe { CoUninitialize() };
    }
    out
}

/// Text als `VT_LPWSTR` im COM-Speicher. Bewusst nie freigegeben: welcher Property-Store
/// den Text übernimmt und selbst freigibt, ist nicht verlässlich (`deck_win::taskbar`
/// hat daran einen Heap-Fehler erlebt). deck-setup endet gleich danach.
fn string_value(s: &str) -> Option<PROPVARIANT> {
    let wide: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: frischer Block passender Größe, danach genau `wide.len()` u16 hineinkopiert.
    let ptr = unsafe { CoTaskMemAlloc(wide.len() * size_of::<u16>()) }.cast::<u16>();
    if ptr.is_null() {
        return None;
    }
    unsafe { std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len()) };
    Some(PROPVARIANT {
        Anonymous: PROPVARIANT_0 {
            Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                vt: VT_LPWSTR,
                Anonymous: PROPVARIANT_0_0_0 { pwszVal: PWSTR(ptr) },
                ..Default::default()
            }),
        },
    })
}

fn write_link(link: &Path, exe: &Path, app_id: &str) -> windows::core::Result<()> {
    let workdir = exe.parent().unwrap_or(Path::new(""));
    with_com(|| unsafe {
        // SAFETY: COM ist eingerichtet; alle Texte leben bis zum Ende des Aufrufs.
        let sl: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        sl.SetPath(&HSTRING::from(exe.as_os_str()))?;
        sl.SetWorkingDirectory(&HSTRING::from(workdir.as_os_str()))?;
        sl.SetIconLocation(&HSTRING::from(exe.as_os_str()), 0)?;
        sl.SetDescription(&HSTRING::from(DESCRIPTION))?;
        let store: IPropertyStore = sl.cast()?;
        let id = string_value(app_id).ok_or_else(|| windows::core::Error::from(E_OUTOFMEMORY))?;
        store.SetValue(&KEY_APP_ID, &id)?;
        store.Commit()?;
        let file: IPersistFile = sl.cast()?;
        file.Save(&HSTRING::from(link.as_os_str()), true)
    })
}

/// App-Kennung einer vorhandenen Verknüpfung (Prüfen, Tests); `None`, wenn keine da ist.
pub fn read_app_id(link: &Path) -> Option<String> {
    with_com(|| unsafe {
        // SAFETY: COM ist eingerichtet; der gelesene Wert ist unsere Kopie und wird geleert.
        let sl: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        let file: IPersistFile = sl.cast()?;
        file.Load(&HSTRING::from(link.as_os_str()), STGM_READ)?;
        let store: IPropertyStore = sl.cast()?;
        let mut v = store.GetValue(&KEY_APP_ID)?;
        let inner = &v.Anonymous.Anonymous;
        let text = if inner.vt == VT_LPWSTR { inner.Anonymous.pwszVal.to_string().ok() } else { None };
        let _ = PropVariantClear(&mut v);
        Ok(text)
    })
    .ok()
    .flatten()
}

/// Verknüpfung `link` auf `exe` anlegen oder erneuern.
pub fn create(link: &Path, exe: &Path, app_id: &str) -> Report {
    let mut r = Report::default();
    if let Err(e) = check_target(link) {
        r.push(Grade::Fail, e.to_string());
        return r;
    }
    if !exe.is_file() {
        r.push(Grade::Fail, format!("{} fehlt, keine Verknüpfung angelegt", exe.display()));
        return r;
    }
    if let Some(dir) = link.parent()
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        r.push(Grade::Fail, format!("{} nicht angelegt: {e}", dir.display()));
        return r;
    }
    match write_link(link, exe, app_id) {
        Ok(()) => {
            r.changed = true;
            r.push(Grade::Ok, format!("Startmenü: {} → {}", link.display(), exe.display()));
        }
        Err(e) => r.push(Grade::Fail, format!("Verknüpfung {} nicht geschrieben: {e}", link.display())),
    }
    r
}

/// Verknüpfung entfernen; fehlt sie, ist das gut.
pub fn remove(link: &Path) -> Report {
    let mut r = Report::default();
    if let Err(e) = check_target(link) {
        r.push(Grade::Fail, e.to_string());
        return r;
    }
    if !link.exists() {
        r.push(Grade::Ok, "Keine Startmenü-Verknüpfung: nichts zu entfernen");
        return r;
    }
    match std::fs::remove_file(link) {
        Ok(()) => {
            r.changed = true;
            r.push(Grade::Ok, format!("Startmenü-Verknüpfung entfernt: {}", link.display()));
        }
        Err(e) => r.push(Grade::Fail, format!("{} nicht entfernt: {e}", link.display())),
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shortcut_points_at_the_exe_and_carries_the_app_id() {
        let d = tempfile::tempdir().unwrap();
        let exe = d.path().join("bin").join("agent-deck.exe");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        // Irgendeine echte Exe als Inhalt, damit die Shell ein Symbol findet.
        std::fs::copy(std::env::current_exe().unwrap(), &exe).unwrap();
        let link = d.path().join("Programs").join(LINK_NAME);
        let r = create(&link, &exe, APP_ID);
        assert!(r.is_ok() && r.changed, "{r:?}");
        assert_eq!(read_app_id(&link).as_deref(), Some(APP_ID));
        let again = create(&link, &exe, APP_ID);
        assert!(again.is_ok(), "zweiter Lauf erneuert nur");
        assert!(remove(&link).changed);
        assert!(!link.exists());
        assert!(remove(&link).is_ok() && !remove(&link).changed, "fehlt sie, ist das gut");
    }

    #[test]
    fn without_the_exe_there_is_no_shortcut() {
        let d = tempfile::tempdir().unwrap();
        let link = d.path().join(LINK_NAME);
        assert_eq!(create(&link, &d.path().join("fehlt.exe"), APP_ID).fails(), 1);
        assert!(!link.exists());
    }

    #[test]
    fn the_real_start_menu_is_locked_in_tests() {
        assert!(create(&link_path(), Path::new(r"C:\x\agent-deck.exe"), APP_ID).fails() == 1);
        assert!(remove(&link_path()).fails() == 1);
    }
}
