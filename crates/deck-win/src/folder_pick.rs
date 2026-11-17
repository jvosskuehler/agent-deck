//! Der Ordner-Dialog von Windows („Ordner auswählen“), für den Repos-Ordner in den
//! Einstellungen. Bewusst kein Tauri-Plugin dafür: der Dialog ist ein COM-Aufruf, und
//! eine weitere Abhängigkeit samt Rechten in der Oberfläche lohnt sich für einen Knopf
//! nicht.
//!
//! Der Dialog ist modal zu `owner` und hat seine eigene Nachrichtenschleife. Er läuft
//! also auf einem Thread, der warten darf, nie auf dem Hauptthread der App.

use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{ERROR_CANCELLED, HWND, RPC_E_CHANGED_MODE};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize,
};
use windows::Win32::UI::Shell::{
    FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, IShellItem,
    SHCreateItemFromParsingName, SIGDN_FILESYSPATH,
};
use windows::core::{HRESULT, HSTRING};

use crate::guard;
use crate::types::{Error, Result};

/// Ordner wählen lassen. `start`: hier geht der Dialog auf, wenn es den Ordner gibt.
/// `Ok(None)` = abgebrochen.
pub fn pick_folder(owner: isize, title: &str, start: Option<&Path>) -> Result<Option<PathBuf>> {
    guard::check("folder_pick")?;
    // SAFETY: COM nur für diesen Aufruf; wer COM schon anders eingerichtet hat
    // (RPC_E_CHANGED_MODE), bekommt kein CoUninitialize von uns.
    let init = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
    let ours = init.is_ok();
    if !ours && init != RPC_E_CHANGED_MODE {
        return Err(Error::win("CoInitializeEx", init.into()));
    }
    let result = show(owner, title, start);
    if ours {
        // SAFETY: Gegenstück zum erfolgreichen CoInitializeEx oben.
        unsafe { CoUninitialize() };
    }
    result
}

fn show(owner: isize, title: &str, start: Option<&Path>) -> Result<Option<PathBuf>> {
    let cancelled = HRESULT::from_win32(ERROR_CANCELLED.0);
    // SAFETY: übliche Folge für IFileOpenDialog; jede Schnittstelle gibt ihr Drop frei,
    // den Text aus GetDisplayName geben wir mit CoTaskMemFree zurück.
    unsafe {
        let dlg: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).map_err(|e| Error::win("CoCreateInstance", e))?;
        let opts = dlg.GetOptions().map_err(|e| Error::win("GetOptions", e))?;
        dlg.SetOptions(opts | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST).map_err(|e| Error::win("SetOptions", e))?;
        let _ = dlg.SetTitle(&HSTRING::from(title));
        if let Some(dir) = start.filter(|p| p.is_dir())
            && let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(dir.as_os_str()), None)
        {
            let _ = dlg.SetFolder(&item);
        }
        let parent = (owner != 0).then_some(HWND(owner as *mut core::ffi::c_void));
        match dlg.Show(parent) {
            Ok(()) => {}
            Err(e) if e.code() == cancelled => return Ok(None),
            Err(e) => return Err(Error::win("IFileDialog::Show", e)),
        }
        let item = dlg.GetResult().map_err(|e| Error::win("GetResult", e))?;
        let raw = item.GetDisplayName(SIGDN_FILESYSPATH).map_err(|e| Error::win("GetDisplayName", e))?;
        let path = raw.to_string();
        CoTaskMemFree(Some(raw.0 as *const core::ffi::c_void));
        Ok(path.ok().map(PathBuf::from))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dialog_is_an_effect_and_forbidden_in_tests() {
        assert!(matches!(pick_folder(0, "x", None), Err(Error::Forbidden(_))));
    }
}
