//! Die Win32-Seite der SpeechMike-Sprechtaste (Fachliches in `deck_core::speechmike`):
//! die Tasten-Schnittstelle des Mikros finden und lesen, prüfen, ob Windows Terminal vorn
//! ist, und Alt+K schicken.
//!
//! Gelesen wird mit geteiltem Zugriff und blockierend in einem eigenen Thread. ORPHEUS
//! liest dieselbe Schnittstelle gleichzeitig (WebHID), unter Windows bekommen beide jeden
//! Bericht. Zieht man das Mikro ab, endet `read` mit einem Fehler; der Aufrufer sucht
//! dann neu.

use windows::Win32::Devices::DeviceAndDriverInstallation::{
    DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W,
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW,
    SetupDiGetDeviceInterfaceDetailW,
};
use windows::Win32::Devices::HumanInterfaceDevice::{
    HIDP_CAPS, HidD_FreePreparsedData, HidD_GetHidGuid, HidD_GetPreparsedData, HidP_GetCaps,
    PHIDP_PREPARSED_DATA,
};
use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, ReadFile,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY,
    VK_K, VK_MENU,
};
use windows::core::PCWSTR;

use crate::guard;
use crate::types::{Error, Result};
use crate::window::{WT_WINDOW_CLASS, class_of};

/// Eine geöffnete HID-Schnittstelle, nur zum Lesen.
pub struct HidReader {
    handle: HANDLE,
}

// SAFETY: das Handle gehört allein diesem Wert; ReadFile darf jeder Thread aufrufen.
unsafe impl Send for HidReader {}

impl Drop for HidReader {
    fn drop(&mut self) {
        // SAFETY: eigenes Handle, genau einmal geschlossen.
        let _ = unsafe { CloseHandle(self.handle) };
    }
}

impl HidReader {
    /// Die erste angeschlossene Schnittstelle des Herstellers `vendor_id` mit dieser Usage
    /// Page, sonst `None`.
    pub fn open(vendor_id: u16, usage_page: u16) -> Option<HidReader> {
        let needle = format!("vid_{vendor_id:04x}");
        hid_paths()
            .into_iter()
            .filter(|p| p.to_lowercase().contains(&needle))
            .find_map(|p| {
                let reader = open_path(&p)?;
                (reader.usage_page() == Some(usage_page)).then_some(reader)
            })
    }

    /// Blockiert bis zum nächsten Bericht; Report-ID steht vorn. Fehler = Gerät weg.
    pub fn read(&self, buf: &mut [u8]) -> Result<usize> {
        let mut n = 0u32;
        // SAFETY: Puffer und Zähler leben über den synchronen Aufruf hinaus.
        unsafe { ReadFile(self.handle, Some(buf), Some(&mut n), None) }
            .map_err(|e| Error::win("ReadFile", e))?;
        Ok(n as usize)
    }

    fn usage_page(&self) -> Option<u16> {
        let mut pre = PHIDP_PREPARSED_DATA::default();
        // SAFETY: Ausgabezeiger auf lokale Variable; freigegeben unten.
        if !unsafe { HidD_GetPreparsedData(self.handle, &mut pre) } {
            return None;
        }
        let mut caps = HIDP_CAPS::default();
        // SAFETY: `pre` ist gültig bis zur Freigabe direkt danach.
        let status = unsafe { HidP_GetCaps(pre, &mut caps) };
        // SAFETY: von HidD_GetPreparsedData geliefert, genau einmal freigegeben.
        unsafe { HidD_FreePreparsedData(pre) };
        status.is_ok().then_some(caps.UsagePage)
    }
}

fn open_path(path: &str) -> Option<HidReader> {
    let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    // SAFETY: nullterminierter Pfad; das Handle übernimmt HidReader.
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            GENERIC_READ.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    }
    .ok()?;
    Some(HidReader { handle })
}

/// Gerätepfade aller angeschlossenen HID-Schnittstellen.
fn hid_paths() -> Vec<String> {
    // SAFETY: reine Abfragen; die Liste wird am Ende freigegeben, der Detailpuffer ist
    // groß genug (vorher erfragt) und für die Struktur ausgerichtet.
    unsafe {
        let guid = HidD_GetHidGuid();
        let Ok(set) = SetupDiGetClassDevsW(Some(&guid), PCWSTR::null(), None, DIGCF_PRESENT | DIGCF_DEVICEINTERFACE)
        else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for index in 0.. {
            let mut iface = SP_DEVICE_INTERFACE_DATA { cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32, ..Default::default() };
            if SetupDiEnumDeviceInterfaces(set, None, &guid, index, &mut iface).is_err() {
                break;
            }
            let mut need = 0u32;
            let _ = SetupDiGetDeviceInterfaceDetailW(set, &iface, None, 0, Some(&mut need), None);
            if need == 0 {
                continue;
            }
            let mut buf = vec![0u64; (need as usize).div_ceil(8)];
            let detail = buf.as_mut_ptr().cast::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>();
            (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
            if SetupDiGetDeviceInterfaceDetailW(set, &iface, Some(detail), need, None, None).is_ok() {
                out.push(PCWSTR((*detail).DevicePath.as_ptr()).to_string().unwrap_or_default());
            }
        }
        let _ = SetupDiDestroyDeviceInfoList(set);
        out
    }
}

/// Ist gerade ein Windows-Terminal-Fenster aktiv?
pub fn terminal_in_front() -> bool {
    crate::focus::foreground_hwnd().is_some_and(|h| class_of(h) == WT_WINDOW_CLASS)
}

/// Alt+K an das aktive Fenster, so als hätte man es getippt.
pub fn send_alt_k() -> Result<()> {
    guard::check("send_alt_k")?;
    let key = |vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        // Die Marke lässt die eigene Taste durch die ORPHEUS-Sperre (`paste_guard`).
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: vk, dwFlags: flags, dwExtraInfo: crate::paste_guard::DECK_INPUT_TAG, ..Default::default() },
        },
    };
    let none = KEYBD_EVENT_FLAGS(0);
    let inputs = [key(VK_MENU, none), key(VK_K, none), key(VK_K, KEYEVENTF_KEYUP), key(VK_MENU, KEYEVENTF_KEYUP)];
    // SAFETY: Feld lebt über den Aufruf; Größe der Struktur wird mitgegeben.
    let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err(Error::win("SendInput", windows::core::Error::from_thread()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sending_keys_is_blocked_in_tests() {
        assert_eq!(send_alt_k(), Err(Error::Forbidden("send_alt_k")));
    }
}
