//! Das heutige Datum in Ortszeit. Der Fokus-Rahmen wechselt damit zum ersten Advent in
//! sein Winterkleid (`deck_core::season`). Nur lesen, keine Wirkung.

use windows::Win32::System::SystemInformation::GetLocalTime;

/// `(Jahr, Monat, Tag)` nach der Uhr und Zeitzone dieses Rechners.
pub fn local_date() -> (i32, u32, u32) {
    // SAFETY: reine Abfrage, liefert die Struktur als Wert.
    let now = unsafe { GetLocalTime() };
    (i32::from(now.wYear), u32::from(now.wMonth), u32::from(now.wDay))
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_local_date_is_a_real_calendar_day() {
        let (y, m, d) = super::local_date();
        assert!(y >= 2026 && (1..=12).contains(&m) && (1..=31).contains(&d), "{y}-{m}-{d}");
    }
}
