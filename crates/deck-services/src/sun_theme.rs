//! Windows hell oder dunkel schalten, wie es die Einstellungen von Windows tun: die
//! beiden Werte unter `HKCU\…\Themes\Personalize` setzen und allen Fenstern Bescheid
//! geben (`WM_SETTINGCHANGE` mit „ImmersiveColorSet“). Gebraucht von „Hell/Dunkel:
//! Sonnenstand“ ([`crate::sun_switch`]); vorher tat das Jorrits geplante Aufgabe
//! „DarkModeZeitplan“ mit genau diesen Schritten.
//!
//! Hinter dem Trait [`WindowsTheme`], Tests bekommen eine Attrappe. Die echte Fassung
//! schreibt nur, solange [`crate::guard`] nicht sperrt; ausgenommen ist ein Schlüssel unter
//! [`TEST_ROOT`], dann aber ohne Rundruf an die Fenster.

use std::io;

use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE};
use windows::core::w;
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};

use crate::guard;
use crate::user_system::TEST_ROOT;

/// Wo Windows sich hell und dunkel merkt.
pub const PERSONALIZE_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
pub const APPS_VALUE: &str = "AppsUseLightTheme";
pub const SYSTEM_VALUE: &str = "SystemUsesLightTheme";

/// Hell und dunkel von Windows lesen und setzen.
pub trait WindowsTheme: Send + Sync {
    /// (Apps hell?, System hell?); `None`, wo der Wert fehlt.
    fn read(&self) -> (Option<bool>, Option<bool>);
    /// Apps und System auf hell bzw. dunkel setzen und es allen Fenstern sagen.
    fn write(&self, light: bool) -> io::Result<()>;
}

/// Beide Werte auf `light` bringen, aber nur, wenn einer abweicht. `Ok(true)` = umgeschaltet.
pub fn apply(theme: &dyn WindowsTheme, light: bool) -> io::Result<bool> {
    if theme.read() == (Some(light), Some(light)) {
        return Ok(false);
    }
    theme.write(light)?;
    Ok(true)
}

/// Die echte Registratur.
pub struct RegistryTheme {
    key: String,
    broadcast: bool,
}

impl RegistryTheme {
    /// Der Schlüssel von Windows, mit Rundruf an alle Fenster.
    pub fn real() -> RegistryTheme {
        RegistryTheme { key: PERSONALIZE_KEY.into(), broadcast: true }
    }

    /// Ein Schlüssel unter [`TEST_ROOT`], ohne Rundruf (Tests).
    pub fn under_test_root(sub: &str) -> RegistryTheme {
        RegistryTheme { key: format!(r"{TEST_ROOT}\{sub}"), broadcast: false }
    }

    fn is_test_key(&self) -> bool {
        self.key.to_ascii_lowercase().starts_with(&format!(r"{}\", TEST_ROOT.to_ascii_lowercase()))
    }
}

fn read_flag(key: &RegKey, name: &str) -> Option<bool> {
    key.get_value::<u32, _>(name).ok().map(|v| v != 0)
}

/// `WM_SETTINGCHANGE` „ImmersiveColorSet“ an alle Fenster, wie die Einstellungen von
/// Windows es schicken; hängende Fenster werden übersprungen, keins wartet länger als 5 s.
fn broadcast() {
    let mut result = 0usize;
    // SAFETY: Konstante Zeichenkette, die Rückgabe landet in einer lokalen Variablen.
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(w!("ImmersiveColorSet").as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            5000,
            Some(&raw mut result),
        );
    }
}

impl WindowsTheme for RegistryTheme {
    fn read(&self) -> (Option<bool>, Option<bool>) {
        match RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(&self.key, KEY_READ) {
            Ok(k) => (read_flag(&k, APPS_VALUE), read_flag(&k, SYSTEM_VALUE)),
            Err(_) => (None, None),
        }
    }

    fn write(&self, light: bool) -> io::Result<()> {
        if !self.is_test_key() {
            guard::check("Hell/Dunkel von Windows umschalten")?;
        }
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey_with_flags(&self.key, KEY_SET_VALUE)?;
        let v = u32::from(light);
        key.set_value(APPS_VALUE, &v)?;
        key.set_value(SYSTEM_VALUE, &v)?;
        if self.broadcast && !self.is_test_key() {
            broadcast();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// Windows als Attrappe: zwei Werte, jedes Schreiben wird mitgezählt.
    #[derive(Default)]
    struct FakeTheme {
        values: Mutex<(Option<bool>, Option<bool>)>,
        writes: Mutex<Vec<bool>>,
    }

    impl WindowsTheme for FakeTheme {
        fn read(&self) -> (Option<bool>, Option<bool>) {
            *self.values.lock().unwrap()
        }
        fn write(&self, light: bool) -> io::Result<()> {
            *self.values.lock().unwrap() = (Some(light), Some(light));
            self.writes.lock().unwrap().push(light);
            Ok(())
        }
    }

    #[test]
    fn windows_is_only_written_when_a_value_differs() {
        let t = FakeTheme::default();
        assert!(apply(&t, false).unwrap(), "fehlende Werte zählen als abweichend");
        assert!(!apply(&t, false).unwrap());
        *t.values.lock().unwrap() = (Some(false), Some(true));
        assert!(apply(&t, false).unwrap(), "System hell, Apps dunkel: beide angleichen");
        assert_eq!(*t.writes.lock().unwrap(), [false, false]);
    }

    #[test]
    fn the_real_registry_writes_both_values_under_the_test_root_only() {
        let t = RegistryTheme::under_test_root("sun_theme_roundtrip");
        t.write(false).unwrap();
        assert_eq!(t.read(), (Some(false), Some(false)));
        assert!(apply(&t, true).unwrap());
        assert_eq!(t.read(), (Some(true), Some(true)));
        let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(r"Software\AgentDeckTest\sun_theme_roundtrip");
        assert!(
            RegistryTheme::real().write(true).is_err(),
            "der echte Schlüssel ist in Tests gesperrt"
        );
    }
}
