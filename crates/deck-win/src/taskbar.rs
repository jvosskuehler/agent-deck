//! Eigener Taskleisten-Knopf für fremde Fenster (die Windows-Terminal-Agenten).
//!
//! Alle Agentenfenster gehören zu EINEM WindowsTerminal-Prozess und landen darum im
//! Terminal-Knopf, mit dem Terminal-Symbol. Das WT-Profil-Symbol ändert nur den Tab.
//! Windows gruppiert die Taskleiste aber nach der App-Kennung (AppUserModelID), und die
//! lässt sich je Fenster setzen, auch von außen (gemessen 2026-11-09, WT 1.24): mit
//! einer gemeinsamen Kennung bilden die Agenten eine eigene Gruppe, das Symbol kommt aus
//! `RelaunchIconResource`.
//!
//! Windows liest das Symbol nur, wenn die Kennung gesetzt wird. Ein späterer Tausch des
//! Symbols bei gleicher Kennung bleibt unsichtbar; das Symbol ist also fest.
//!
//! Falle (gemessen 2026-11-09, Heap-Fehler 0xC0000374 zwei Sekunden nach dem Start):
//! dieser Property-Store gibt die übergebenen Texte selbst frei, anders als die Doku zu
//! `SetValue` sagt. Ein Rust-Puffer wurde so zweimal freigegeben. Die Texte kommen darum
//! aus `CoTaskMemAlloc` und gehen an Windows über; wir geben sie nie selbst frei.

use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{E_OUTOFMEMORY, PROPERTYKEY, RPC_E_CHANGED_MODE, VARIANT_TRUE};
use windows::Win32::System::Com::StructuredStorage::{PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemAlloc, CoUninitialize};
use windows::Win32::System::Variant::{VT_BOOL, VT_LPWSTR};
use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
use windows::core::{GUID, PWSTR};

use crate::guard;
use crate::types::{Error, Hwnd, Result};

/// Wohin ein Fenster in der Taskleiste gehört.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskbarGroup {
    /// App-Kennung; alle Fenster mit derselben teilen sich einen Knopf.
    pub app_id: String,
    /// `.ico`-Datei für den Knopf.
    pub icon: PathBuf,
    /// Name der Gruppe (Sprungliste, Einstellungen).
    pub name: String,
}

const FMTID_APP_USER_MODEL: GUID = GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3);
const fn key(pid: u32) -> PROPERTYKEY {
    PROPERTYKEY { fmtid: FMTID_APP_USER_MODEL, pid }
}
const KEY_ID: PROPERTYKEY = key(5);
const KEY_ICON: PROPERTYKEY = key(3);
const KEY_NAME: PROPERTYKEY = key(4);
const KEY_PREVENT_PINNING: PROPERTYKEY = key(9);

/// Text als Wert, im COM-Speicher. Der Store übernimmt ihn (siehe oben), darum hier
/// kein Freigeben. `None`, wenn kein Speicher mehr da ist.
fn string_value(s: &str) -> Option<PROPVARIANT> {
    let wide: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = wide.len() * size_of::<u16>();
    // SAFETY: frischer Block passender Größe, danach genau `wide.len()` u16 hineinkopiert.
    let ptr = unsafe { CoTaskMemAlloc(bytes) }.cast::<u16>();
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

fn bool_value(v: bool) -> PROPVARIANT {
    let b = if v { VARIANT_TRUE } else { Default::default() };
    PROPVARIANT {
        Anonymous: PROPVARIANT_0 {
            Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                vt: VT_BOOL,
                Anonymous: PROPVARIANT_0_0_0 { boolVal: b },
                ..Default::default()
            }),
        },
    }
}

/// `"<pfad>,0"`: erstes Symbol der Datei, wie Windows es als Ressourcenangabe erwartet.
pub fn icon_resource(icon: &Path) -> String {
    format!("{},0", icon.display())
}

/// Das Fenster in die Gruppe stellen. Der Name kommt vor der Kennung, weil die
/// Taskleiste beim Setzen der Kennung die übrigen Angaben liest. Anheften ist aus: ein
/// angehefteter Knopf könnte einen Agenten nicht neu starten.
pub fn join_group(hwnd: Hwnd, group: &TaskbarGroup) -> Result<()> {
    guard::check("taskbar_group")?;
    // SAFETY: COM nur für diesen Aufruf; wer COM schon anders eingerichtet hat
    // (RPC_E_CHANGED_MODE), bekommt kein CoUninitialize von uns.
    let init = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let ours = init.is_ok();
    if !ours && init != RPC_E_CHANGED_MODE {
        return Err(Error::win("CoInitializeEx", init.into()));
    }
    let result = (|| -> windows::core::Result<()> {
        let text = |s: &str| string_value(s).ok_or_else(|| windows::core::Error::from(E_OUTOFMEMORY));
        // SAFETY: gültiges Fensterhandle oder Fehler; die Texte gehören danach dem Store.
        unsafe {
            let store: IPropertyStore = SHGetPropertyStoreForWindow(hwnd.h())?;
            store.SetValue(&KEY_ICON, &text(&icon_resource(&group.icon))?)?;
            store.SetValue(&KEY_NAME, &text(&group.name)?)?;
            store.SetValue(&KEY_PREVENT_PINNING, &bool_value(true))?;
            store.SetValue(&KEY_ID, &text(&group.app_id)?)?;
            store.Commit()
        }
    })();
    if ours {
        // SAFETY: Gegenstück zum erfolgreichen CoInitializeEx oben.
        unsafe { CoUninitialize() };
    }
    result.map_err(|e| Error::win("SHGetPropertyStoreForWindow", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_is_named_as_the_first_resource_of_the_file() {
        assert_eq!(icon_resource(Path::new(r"C:\a b\agent.ico")), r"C:\a b\agent.ico,0");
    }

    #[test]
    fn joining_a_group_is_an_effect_and_forbidden_in_tests() {
        let g = TaskbarGroup { app_id: "X".into(), icon: PathBuf::from("x.ico"), name: "X".into() };
        assert!(matches!(join_group(Hwnd(1), &g), Err(Error::Forbidden(_))));
    }
}
