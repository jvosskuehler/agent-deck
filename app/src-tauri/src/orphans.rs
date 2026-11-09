//! „Läuft noch ohne Fenster“ in der App: der Scan-Thread und die Befehle `kill_orphan`
//! und `kill_all_orphans` (docs/IPC.md).
//!
//! Der Scan läuft nie im 400-ms-Takt der Oberfläche: ein eigener Thread schaut alle 2 s
//! hin und SOFORT, wenn eine Agenten-Shell endet (er wartet auf ihre Prozess-Handles).
//! Dann tragen die Kinder noch die Eltern-PID der eben gestorbenen Shell, und Kinder,
//! die kurz vor dem Schließen gestartet sind, werden noch zugeordnet.

use std::sync::Arc;
use std::time::Duration;

use deck_core::i18n::Lang;
use deck_services::orphans::OrphanWatch;
use tauri::AppHandle;

use crate::applog::note;
use crate::poll;
use crate::services::Wake;
use crate::state::{lock, shared};

/// So oft (s) schaut der Scan-Thread ohne besonderen Anlass hin.
const SCAN_EVERY: Duration = Duration::from_secs(2);

/// Den Scan-Thread starten; `wake` stößt den Takt an, wenn sich etwas geändert hat.
pub fn start(watch: Arc<OrphanWatch>, wake: Wake) {
    let spawned = std::thread::Builder::new().name("orphans".into()).spawn(move || {
        loop {
            if watch.scan(crate::now()) {
                wake();
            }
            watch.wait(SCAN_EVERY);
        }
    });
    if let Err(e) = spawned {
        note(format!("orphans: Scan-Thread startet nicht: {e}"));
    }
}

fn watch_and_lang(app: &AppHandle) -> Option<(Arc<OrphanWatch>, Lang)> {
    let s = shared(app);
    let d = lock(&s.deck);
    Some((d.services.orphan_watch()?, d.lang))
}

/// Eine Zeile beenden. Kehrt sofort zurück; Ausgang und Fehler stehen danach in der Zeile.
#[tauri::command]
pub async fn kill_orphan(app: AppHandle, id: String) -> Result<(), String> {
    let Some((watch, lang)) = watch_and_lang(&app) else { return Ok(()) };
    watch.mark_busy(&id);
    poll::nudge(&app);
    std::thread::spawn(move || {
        if let Err(e) = watch.kill(&id, crate::now(), lang) {
            note(format!("orphans: {id} nicht beendet: {e}"));
        }
        poll::wake();
    });
    Ok(())
}

/// Alle Zeilen beenden („Alle beenden“).
#[tauri::command]
pub async fn kill_all_orphans(app: AppHandle) -> Result<(), String> {
    let Some((watch, lang)) = watch_and_lang(&app) else { return Ok(()) };
    for r in watch.rows(crate::now(), lang) {
        watch.mark_busy(&r.id);
    }
    poll::nudge(&app);
    std::thread::spawn(move || {
        if let Err(e) = watch.kill_all(crate::now(), lang) {
            note(format!("orphans: Alle beenden: {e}"));
        }
        poll::wake();
    });
    Ok(())
}
