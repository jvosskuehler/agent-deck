//! Was die Einrichtung außerhalb von Dateien anfasst: die Registratur des Benutzers
//! (HKCU; HKLM nur lesend) und die Schriften der laufenden Sitzung. Alles ohne
//! Administratorrechte.
//!
//! Hinter dem Trait [`UserSystem`], damit Tests eine Attrappe ([`mem::MemSystem`])
//! bekommen. Die echte Fassung ([`RealSystem`]) verweigert jeden schreibenden Eingriff,
//! solange [`crate::guard`] sperrt; ausgenommen ist nur der Testschlüssel [`TEST_ROOT`],
//! den ein Test selbst wieder löscht.

use std::io;
use std::path::Path;

use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

use crate::guard;

/// Schriften des Benutzers (Wert = voller Pfad) bzw. des Systems (Wert = Dateiname).
pub const FONTS_KEY: &str = r"Software\Microsoft\Windows NT\CurrentVersion\Fonts";
pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub const RUN_ONCE_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\RunOnce";
/// Eintrag unter „Apps“ in den Windows-Einstellungen (pro Benutzer).
pub const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\AgentDeck";
/// Der einzige Schlüssel, den Tests echt beschreiben dürfen.
pub const TEST_ROOT: &str = r"Software\AgentDeckTest";

/// Ein Wert, wie die Einrichtung ihn schreibt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegValue {
    Str(String),
    Dword(u32),
}

pub trait UserSystem {
    /// Textwerte eines Schlüssels unter HKCU als (Name, Text); fehlt er: leer.
    fn user_values(&self, key: &str) -> Vec<(String, String)>;
    /// Dasselbe unter HKLM (nur lesen).
    fn machine_values(&self, key: &str) -> Vec<(String, String)>;
    /// Wert unter HKCU setzen, Schlüssel bei Bedarf anlegen.
    fn set(&self, key: &str, name: &str, value: &RegValue) -> io::Result<()>;
    /// Wert unter HKCU löschen; fehlt er, ist das gut.
    fn delete_value(&self, key: &str, name: &str) -> io::Result<()>;
    /// Schlüssel unter HKCU samt Inhalt löschen; fehlt er, ist das gut.
    fn delete_key(&self, key: &str) -> io::Result<()>;
    /// Schrift in der laufenden Sitzung laden bzw. entladen und es allen Fenstern sagen
    /// (`WM_FONTCHANGE`), damit Windows Terminal sie ohne Neuanmeldung kennt.
    fn font_session(&self, file: &Path, load: bool);

    /// Ein Textwert unter HKCU.
    fn user_value(&self, key: &str, name: &str) -> Option<String> {
        self.user_values(key).into_iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v)
    }
}

/// Die echte Registratur.
pub struct RealSystem;

fn allowed(key: &str, what: &str) -> io::Result<()> {
    let k = key.to_ascii_lowercase();
    if k == TEST_ROOT.to_ascii_lowercase() || k.starts_with(&format!("{}\\", TEST_ROOT.to_ascii_lowercase())) {
        return Ok(());
    }
    guard::check(what)
}

fn missing_ok(r: io::Result<()>) -> io::Result<()> {
    match r {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

fn text_values(root: RegKey, key: &str) -> Vec<(String, String)> {
    let Ok(k) = root.open_subkey(key) else { return Vec::new() };
    k.enum_values()
        .flatten()
        .filter_map(|(name, v)| match v.vtype {
            winreg::enums::RegType::REG_SZ | winreg::enums::RegType::REG_EXPAND_SZ => {
                <String as winreg::types::FromRegValue>::from_reg_value(&v).ok().map(|t| (name, t))
            }
            _ => None,
        })
        .collect()
}

impl UserSystem for RealSystem {
    fn user_values(&self, key: &str) -> Vec<(String, String)> {
        text_values(RegKey::predef(HKEY_CURRENT_USER), key)
    }

    fn machine_values(&self, key: &str) -> Vec<(String, String)> {
        text_values(RegKey::predef(HKEY_LOCAL_MACHINE), key)
    }

    fn set(&self, key: &str, name: &str, value: &RegValue) -> io::Result<()> {
        allowed(key, "Registratur schreiben")?;
        let (k, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(key)?;
        match value {
            RegValue::Str(s) => k.set_value(name, s),
            RegValue::Dword(d) => k.set_value(name, d),
        }
    }

    fn delete_value(&self, key: &str, name: &str) -> io::Result<()> {
        allowed(key, "Registratur ändern")?;
        let k = match RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(key, winreg::enums::KEY_SET_VALUE) {
            Ok(k) => k,
            Err(e) => return missing_ok(Err(e)),
        };
        missing_ok(k.delete_value(name))
    }

    fn delete_key(&self, key: &str) -> io::Result<()> {
        allowed(key, "Registratur ändern")?;
        missing_ok(RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(key))
    }

    fn font_session(&self, file: &Path, load: bool) {
        if guard::check("Schrift laden").is_err() {
            return;
        }
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::Graphics::Gdi::{AddFontResourceW, RemoveFontResourceW};
        use windows::Win32::UI::WindowsAndMessaging::{HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_FONTCHANGE};
        let path = windows::core::HSTRING::from(file.as_os_str());
        // SAFETY: gültiger, nullterminierter Pfad; die Rundsendung wartet höchstens 1 s je Fenster.
        unsafe {
            if load {
                AddFontResourceW(&path);
            } else {
                let _ = RemoveFontResourceW(&path);
            }
            SendMessageTimeoutW(HWND_BROADCAST, WM_FONTCHANGE, WPARAM(0), LPARAM(0), SMTO_ABORTIFHUNG, 1000, None);
        }
    }
}

/// Attrappe für Tests: eine Registratur im Speicher.
#[cfg(test)]
pub mod mem {
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::*;

    #[derive(Default)]
    pub struct MemSystem {
        pub user: RefCell<BTreeMap<String, BTreeMap<String, RegValue>>>,
        pub machine: BTreeMap<String, BTreeMap<String, RegValue>>,
        /// (Datei, geladen?) in der Reihenfolge der Aufrufe.
        pub session: RefCell<Vec<(PathBuf, bool)>>,
    }

    fn texts(m: Option<&BTreeMap<String, RegValue>>) -> Vec<(String, String)> {
        m.into_iter()
            .flatten()
            .filter_map(|(n, v)| match v {
                RegValue::Str(s) => Some((n.clone(), s.clone())),
                RegValue::Dword(_) => None,
            })
            .collect()
    }

    impl MemSystem {
        pub fn get(&self, key: &str, name: &str) -> Option<RegValue> {
            self.user.borrow().get(&key.to_lowercase()).and_then(|k| k.get(name)).cloned()
        }

        pub fn has_key(&self, key: &str) -> bool {
            self.user.borrow().contains_key(&key.to_lowercase())
        }
    }

    impl UserSystem for MemSystem {
        fn user_values(&self, key: &str) -> Vec<(String, String)> {
            texts(self.user.borrow().get(&key.to_lowercase()))
        }

        fn machine_values(&self, key: &str) -> Vec<(String, String)> {
            texts(self.machine.get(&key.to_lowercase()))
        }

        fn set(&self, key: &str, name: &str, value: &RegValue) -> io::Result<()> {
            self.user.borrow_mut().entry(key.to_lowercase()).or_default().insert(name.into(), value.clone());
            Ok(())
        }

        fn delete_value(&self, key: &str, name: &str) -> io::Result<()> {
            if let Some(k) = self.user.borrow_mut().get_mut(&key.to_lowercase()) {
                k.remove(name);
            }
            Ok(())
        }

        fn delete_key(&self, key: &str) -> io::Result<()> {
            self.user.borrow_mut().remove(&key.to_lowercase());
            Ok(())
        }

        fn font_session(&self, file: &Path, load: bool) {
            self.session.borrow_mut().push((file.to_path_buf(), load));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_real_registry_writes_only_below_the_test_key_in_tests() {
        let s = RealSystem;
        let key = format!(r"{TEST_ROOT}\{}", crate::fsutil::random_u64());
        s.set(&key, "Text", &RegValue::Str("Papier".into())).unwrap();
        s.set(&key, "Zahl", &RegValue::Dword(7)).unwrap();
        assert_eq!(s.user_value(&key, "text").as_deref(), Some("Papier"));
        assert_eq!(s.user_values(&key).len(), 1, "nur Textwerte");
        s.delete_value(&key, "Text").unwrap();
        s.delete_value(&key, "Text").unwrap();
        assert_eq!(s.user_value(&key, "Text"), None);
        s.delete_key(TEST_ROOT).unwrap();
        s.delete_key(TEST_ROOT).unwrap();
        assert!(s.user_values(&key).is_empty());
        // Alles andere ist gesperrt.
        assert!(s.set(RUN_KEY, "AgentDeck", &RegValue::Str("x".into())).is_err());
        assert!(s.delete_value(FONTS_KEY, "x").is_err());
        assert!(s.delete_key(UNINSTALL_KEY).is_err());
        assert!(s.delete_key(&format!("{TEST_ROOT}x")).is_err(), "nur der Testschlüssel selbst, kein Namensvetter");
    }

    #[test]
    fn system_fonts_are_readable_without_rights() {
        assert!(!RealSystem.machine_values(FONTS_KEY).is_empty());
    }
}
