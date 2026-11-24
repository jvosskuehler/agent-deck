//! Threads, die Fenster legen und den Fokus-Rahmen zeichnen, vor die Hintergrundarbeit des
//! Rechners stellen.
//!
//! Unter Last (Build, Tests, alle Kerne voll) wartete das Deck mit normaler Priorität so
//! lange wie jeder andere Prozess: der Vorhang beim neuen Agenten kam 850 statt 300 ms
//! nach dem Klick, Ringe und Schatten zogen eine halbe Sekunde später nach (gemessen
//! 24.11.2026, `tools/probe`, Abläufe mit `-Load 6`). Erhöht wird nur der Thread, der
//! gerade läuft, eine Stufe über normal: das Deck arbeitet in Schüben von Millisekunden,
//! es kann dem Rechner damit nichts wegnehmen, was man merkt.

use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_ABOVE_NORMAL};

use crate::guard;

/// Den aufrufenden Thread eine Stufe über normal stellen. In Tests: nichts.
pub fn raise_current() {
    if guard::check("thread_priority").is_err() {
        return;
    }
    // SAFETY: Pseudo-Handle des eigenen Threads, gültig für die Dauer des Aufrufs.
    let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL) };
}
