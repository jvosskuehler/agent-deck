//! Hell oder dunkel für das Deck selbst, und Bescheid nach dem Aufwachen.
//!
//! **Festes Thema.** Steht die Einstellung „Hell/Dunkel“ auf hell oder dunkel, gilt das
//! nur für das Deck: Windows bleibt, wie es ist. Was in Rust nach dem Thema fragt
//! (Fensterrand der Agenten, Gruppenschatten, Glow), fragt [`crate::effects::Desktop::light_theme`];
//! die echte Fassung nimmt das hier gesetzte Thema vor dem Wert aus der Registratur. Ein
//! Wert für den ganzen Prozess, weil der Rahmen-Thread seine eigene `Desktop`-Kopie hält.
//! Tests benutzen Attrappen und sehen ihn nicht.
//!
//! **Aufwachen.** Nach Standby oder Ruhezustand soll der Sonnenstand sofort nachgesehen
//! werden, nicht erst beim nächsten Takt: [`on_resume`] meldet sich dafür bei Windows an
//! (`PowerRegisterSuspendResumeNotification`, ohne Fenster).

use std::ffi::c_void;
use std::sync::atomic::{AtomicU8, Ordering};

use windows::Win32::Foundation::{ERROR_SUCCESS, HANDLE};
use windows::Win32::System::Power::{DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS, PowerRegisterSuspendResumeNotification};
use windows::Win32::UI::WindowsAndMessaging::{DEVICE_NOTIFY_CALLBACK, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND};

use crate::types::{Error, Result};

const FOLLOW: u8 = 0;
const LIGHT: u8 = 1;
const DARK: u8 = 2;

static FORCED: AtomicU8 = AtomicU8::new(FOLLOW);

/// Festes Thema setzen: `Some(true)` hell, `Some(false)` dunkel, `None` = wie Windows.
pub fn force(light: Option<bool>) {
    let v = match light {
        None => FOLLOW,
        Some(true) => LIGHT,
        Some(false) => DARK,
    };
    FORCED.store(v, Ordering::SeqCst);
}

/// Das gesetzte feste Thema, sonst `None`.
pub fn forced() -> Option<bool> {
    match FORCED.load(Ordering::SeqCst) {
        LIGHT => Some(true),
        DARK => Some(false),
        _ => None,
    }
}

type Callback = Box<dyn Fn() + Send + Sync>;

unsafe extern "system" fn resumed(context: *const c_void, kind: u32, _setting: *const c_void) -> u32 {
    if (kind == PBT_APMRESUMEAUTOMATIC || kind == PBT_APMRESUMESUSPEND) && !context.is_null() {
        // SAFETY: `context` ist der in `on_resume` absichtlich nie freigegebene Callback.
        let cb = unsafe { &*context.cast::<Callback>() };
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(cb));
    }
    0
}

/// `cb` nach jedem Aufwachen aus Standby oder Ruhezustand rufen (auf einem Thread von
/// Windows; `cb` soll nur kurz anstoßen). Gilt bis zum Ende des Prozesses.
pub fn on_resume(cb: impl Fn() + Send + Sync + 'static) -> Result<()> {
    crate::guard::check("on_resume")?;
    // Callback und Parameter leben bis zum Prozessende: Windows hält den Zeiger.
    let cb: &'static Callback = Box::leak(Box::new(Box::new(cb) as Callback));
    let params: &'static mut DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS = Box::leak(Box::new(DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
        Callback: Some(resumed),
        Context: std::ptr::from_ref(cb).cast_mut().cast(),
    }));
    let mut handle: *mut c_void = std::ptr::null_mut();
    // SAFETY: `params` lebt für immer, `handle` ist eine lokale Ausgabevariable.
    let err = unsafe {
        PowerRegisterSuspendResumeNotification(
            DEVICE_NOTIFY_CALLBACK,
            HANDLE(std::ptr::from_mut(params).cast()),
            &mut handle,
        )
    };
    if err == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(Error::Win32 { call: "PowerRegisterSuspendResumeNotification", code: err.0, message: "Anmeldung abgelehnt".into() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_forced_theme_reads_back_and_none_follows_windows_again() {
        force(Some(false));
        assert_eq!(forced(), Some(false));
        force(Some(true));
        assert_eq!(forced(), Some(true));
        force(None);
        assert_eq!(forced(), None);
    }
}
