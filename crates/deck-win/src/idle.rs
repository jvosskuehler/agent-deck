//! Wie lange die letzte Eingabe (Taste, Maus) her ist, systemweit. Das Deck schickt ein
//! automatisches `/compact` nicht in ein Fenster, in dem gerade jemand tippt
//! (`deck_core::prompt_cache::user_busy`). Nur lesen, keine Wirkung.

use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

/// Sekunden seit der letzten Eingabe. Kann Windows es nicht sagen: 0, also „gerade
/// getippt“; lieber einmal zu vorsichtig als jemandem in die Eingabe zu schreiben.
pub fn input_idle_s() -> f64 {
    let mut info = LASTINPUTINFO { cbSize: size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    // SAFETY: reine Abfrage in eine lokale Struktur mit gesetzter Größe.
    if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
        return 0.0;
    }
    // Beide Zähler laufen nach 49,7 Tagen über; die Differenz stimmt trotzdem.
    // SAFETY: reine Abfrage.
    let now = unsafe { GetTickCount() };
    f64::from(now.wrapping_sub(info.dwTime)) / 1000.0
}

#[cfg(test)]
mod tests {
    #[test]
    fn idle_time_is_a_plausible_number_of_seconds() {
        let s = super::input_idle_s();
        assert!((0.0..50.0 * 86_400.0).contains(&s), "{s}");
    }
}
