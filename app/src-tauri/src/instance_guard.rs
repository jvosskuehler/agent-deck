//! Beim Start: läuft schon ein Deck, das hängt, wird es beendet, statt ihm das Feld zu
//! überlassen (Rechnung: `deck_core::liveness::predecessor`).
//!
//! Ohne das gibt die Einzelinstanz-Sperre jeden neuen Start an die alte Instanz weiter;
//! hängt die, „passiert beim Neustart einfach nichts“ (11.11.2026). Läuft vor dem Aufbau
//! von Tauri, damit die Sperre danach frei ist.

use std::path::Path;
use std::time::{Duration, Instant};

use deck_core::liveness::{Predecessor, UI_ALIVE_FILE, UiAlive, after_wait, is_fresh, predecessor};
use deck_services::heartbeat;
use deck_win::proc_scan;
use deck_win::procs::process_start;

use crate::applog::note;
use crate::ui_watch::AFTER_VAR;

/// So lange darf eine knapp verspätete Instanz noch antworten.
const WAIT: Duration = Duration::from_secs(8);
/// So lange wartet das Kind eines Selbst-Neustarts auf das Ende der Eltern-Instanz.
const AFTER_WAIT_MS: u32 = 10_000;
/// FILETIME-Ticks je Sekunde und Abstand 1601 → 1970.
const TICKS: f64 = 10_000_000.0;
const EPOCH_DIFF_S: f64 = 11_644_473_600.0;

fn read_alive(app_dir: &Path) -> Option<UiAlive> {
    serde_json::from_value(deck_core::paths::load_json(&app_dir.join(UI_ALIVE_FILE))?).ok()
}

/// Ist die PID wirklich ein laufendes agent-deck (nicht neu vergeben)? Dann ihre Startzeit.
fn deck_process(pid: u32) -> Option<u64> {
    let start = process_start(pid)?;
    let image = proc_scan::details(pid, start)?.image?;
    let name = Path::new(&image).file_name()?.to_string_lossy().to_lowercase();
    (name == "agent-deck.exe").then_some(start)
}

/// Vor dem Aufbau der App aufrufen.
pub fn take_over_if_stuck(app_dir: &Path) {
    if let Some(pid) = std::env::var(AFTER_VAR).ok().and_then(|v| v.parse::<u32>().ok()) {
        // SAFETY: ganz am Anfang, bevor irgendein anderer Thread läuft.
        unsafe { std::env::remove_var(AFTER_VAR) };
        if let Some(start) = process_start(pid) {
            proc_scan::wait_any_exit(&[(pid, start)], AFTER_WAIT_MS);
        }
    }
    let Some(beat) = heartbeat::read(app_dir) else { return };
    let me = std::process::id();
    if beat.pid == me || beat.pid == 0 || beat.ended.is_some() {
        return;
    }
    let Some(start) = deck_process(beat.pid) else { return };
    let now = crate::now();
    let age = now - (start as f64 / TICKS - EPOCH_DIFF_S);
    let reason = match predecessor(now, beat.pid, beat.ts, age, read_alive(app_dir).as_ref()) {
        Predecessor::Healthy => return,
        Predecessor::Stuck(why) => why,
        Predecessor::Wait => {
            let t0 = Instant::now();
            loop {
                std::thread::sleep(Duration::from_millis(500));
                let now = crate::now();
                let alive = read_alive(app_dir).filter(|a| a.pid == beat.pid);
                match alive {
                    Some(a) if is_fresh(now, &a) => return,
                    _ if t0.elapsed() < WAIT => continue,
                    Some(a) => break after_wait(now, &a),
                    None => break "Lebenszeichen fehlen".into(),
                }
            }
        }
    };
    match proc_scan::terminate_exact(beat.pid, start) {
        Ok(_) => note(format!("Vorgänger-Deck (PID {}) hing: {reason}. Beendet, diese Instanz übernimmt.", beat.pid)),
        Err(e) => note(format!("Vorgänger-Deck (PID {}) hängt ({reason}), Beenden fehlgeschlagen: {e}", beat.pid)),
    }
}
