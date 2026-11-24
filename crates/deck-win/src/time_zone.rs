//! Die Zeitzone von Windows, für den Ort des Sonnenstands (`deck_core::sun_zone`).
//!
//! Nur lesen, ohne Wirkung nach außen: darum ohne Attrappe und ohne Sperre für Tests.

use windows::Win32::System::Time::{DYNAMIC_TIME_ZONE_INFORMATION, GetDynamicTimeZoneInformation, TIME_ZONE_ID_INVALID};

/// Schlüssel der Zeitzone (`W. Europe Standard Time`) und Bias in Minuten (UTC = Ortszeit
/// + Bias). `None`, wenn Windows keine Antwort gibt.
pub fn current() -> Option<(String, i32)> {
    let mut info = DYNAMIC_TIME_ZONE_INFORMATION::default();
    // SAFETY: schreibt nur in die übergebene Struktur.
    let id = unsafe { GetDynamicTimeZoneInformation(&mut info) };
    if id == TIME_ZONE_ID_INVALID {
        return None;
    }
    let raw = &info.TimeZoneKeyName;
    let len = raw.iter().position(|c| *c == 0).unwrap_or(raw.len());
    Some((String::from_utf16_lossy(&raw[..len]), info.Bias))
}

#[cfg(test)]
mod tests {
    #[test]
    fn windows_names_a_time_zone_with_a_sane_bias() {
        let (key, bias) = super::current().expect("Zeitzone");
        assert!(!key.is_empty());
        assert!(bias.abs() <= 14 * 60, "{bias}");
    }
}
