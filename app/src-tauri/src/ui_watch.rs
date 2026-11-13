//! Lebt die Oberfläche? Prüft im Takt Hauptthread und die Seite des sichtbaren Fensters
//! und startet das Deck neu, wenn eins davon schweigt (Rechnung: `deck_core::liveness`).
//!
//! Ein Deck, dessen Leiste weg ist, das aber weiterläuft, ist schlimmer als ein
//! abgestürztes: es blockiert über die Einzelinstanz-Sperre jeden Neustart (11.11.2026).
//!
//! Regel aus `poll.rs` gilt auch hier: nie auf etwas warten, das der Hauptthread liefern
//! muss. Darum `try_lock` auf das Dock und `run_on_main_thread` nur als Botschaft, deren
//! Ausbleiben gemessen wird.

use std::collections::HashMap;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use deck_core::liveness::{CHECK_EVERY_S, SLEEP_GAP_S, UI_ALIVE_FILE, UiAlive, Verdict, verdict};
use deck_services::heartbeat::{EndReason, mark_ended};
use tauri::{AppHandle, Emitter, WebviewWindow};

use crate::applog::note;
use crate::state::shared;
use crate::ui_windows::{HANDLE, PANEL};

/// Umgebungsvariable für das Kind eines Selbst-Neustarts: PID der Instanz, auf deren
/// Ende es warten soll (`instance_guard`).
pub const AFTER_VAR: &str = "AGENT_DECK_AFTER";

const DETACHED_PROCESS: u32 = 0x0000_0008;
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;

/// Letzte Antwort je Fenster (Label → Unix-Sekunden).
fn pongs() -> &'static Mutex<HashMap<String, f64>> {
    static P: OnceLock<Mutex<HashMap<String, f64>>> = OnceLock::new();
    P.get_or_init(Mutex::default)
}

/// Letzter Durchlauf auf dem Hauptthread (f64-Bits).
static MAIN_SEEN: AtomicU64 = AtomicU64::new(0);

fn load(a: &AtomicU64) -> f64 {
    f64::from_bits(a.load(Ordering::SeqCst))
}

/// Die Seite eines Fensters hat auf `deck://ping` geantwortet.
#[tauri::command]
pub async fn ui_pong(window: WebviewWindow) -> Result<(), String> {
    if let Ok(mut m) = pongs().lock() {
        m.insert(window.label().to_string(), crate::now());
    }
    Ok(())
}

/// Welches Fenster gerade zu sehen sein soll. Ist das Dock gesperrt (es wartet womöglich
/// selbst auf den Hauptthread), bleibt es beim letzten Wissen.
fn expected(app: &AppHandle, last: &'static str) -> &'static str {
    let s = shared(app);
    match s.dock.try_lock() {
        Ok(ctl) => {
            if ctl.open() {
                PANEL
            } else {
                HANDLE
            }
        }
        Err(_) => last,
    }
}

pub fn start(app: &AppHandle, app_dir: PathBuf) {
    let app = app.clone();
    let started = crate::now();
    MAIN_SEEN.store(started.to_bits(), Ordering::SeqCst);
    let spawned = std::thread::Builder::new().name("ui-watch".into()).spawn(move || {
        let mut last_loop = started;
        let mut window = HANDLE;
        loop {
            std::thread::sleep(Duration::from_secs_f64(CHECK_EVERY_S));
            let now = crate::now();
            if now - last_loop > SLEEP_GAP_S {
                // Der Rechner schlief: niemand konnte antworten, alles gilt ab jetzt.
                MAIN_SEEN.store(now.to_bits(), Ordering::SeqCst);
                if let Ok(mut m) = pongs().lock() {
                    m.values_mut().for_each(|t| *t = now);
                }
            }
            last_loop = now;
            for label in [PANEL, HANDLE] {
                let _ = app.emit_to(label, "deck://ping", ());
            }
            let _ = app.run_on_main_thread(|| MAIN_SEEN.store(crate::now().to_bits(), Ordering::SeqCst));
            window = expected(&app, window);
            let ui = pongs().lock().ok().and_then(|m| m.get(window).copied()).unwrap_or(started);
            let alive = UiAlive { pid: std::process::id(), ui, main: load(&MAIN_SEEN), ts: now };
            write(&app_dir, &alive);
            let v = verdict(now, started, &alive);
            if v != Verdict::Alive {
                note(format!("--- Oberfläche hängt: {}. Neustart ---", v.describe(window)));
                restart(&app_dir);
            }
        }
    });
    if let Err(e) = spawned {
        note(format!("ui-watch nicht gestartet: {e}"));
    }
}

fn write(app_dir: &Path, alive: &UiAlive) {
    if let Ok(raw) = serde_json::to_vec(alive) {
        let _ = deck_core::paths::atomic_write(&app_dir.join(UI_ALIVE_FILE), &raw);
    }
}

/// Neu starten, ohne auf den Hauptthread zu warten: Kind starten (es wartet auf unser
/// Ende), dann den eigenen Prozess hart beenden. Ein geordnetes Ende ginge über den
/// Hauptthread, und genau der antwortet womöglich nicht.
fn restart(app_dir: &Path) -> ! {
    mark_ended(app_dir, EndReason::Restart, crate::now());
    let exe = std::env::current_exe().ok();
    let started = exe.as_ref().is_some_and(|exe| {
        let dir = exe.parent().unwrap_or(Path::new("."));
        let flags = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
        [flags | CREATE_BREAKAWAY_FROM_JOB, flags].into_iter().any(|f| {
            Command::new(exe).current_dir(dir).env(AFTER_VAR, std::process::id().to_string()).creation_flags(f).spawn().is_ok()
        })
    });
    if !started {
        note("Neustart: Kind nicht gestartet, der Wächter muss einspringen");
    }
    // SAFETY: beendet nur den eigenen Prozess.
    unsafe {
        use windows::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
        let _ = TerminateProcess(GetCurrentProcess(), 3);
    }
    std::process::exit(3)
}
