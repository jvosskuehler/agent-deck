//! `deck-watchdog`: startet das Agent Deck neu, wenn es verschwunden ist (Python
//! `watchdog.py`). Ohne Konsolenfenster (die Aufgabenplanung ruft es im Takt auf).
//!
//! ```text
//! deck-watchdog                  ein Durchgang (für die Aufgabenplanung)
//! deck-watchdog --status         nur berichten, nichts starten (Ausgabe in die Konsole des Aufrufers)
//! deck-watchdog --loop [sek]     Dauerwächter im eigenen Prozess (mindestens 15 s, Vorgabe 60)
//! deck-watchdog --app <exe>      diese App starten statt der aus dem Herzschlag
//! ```

#![windows_subsystem = "windows"]

use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use deck_services::watchdog::{LOOP_EVERY, Watchdog};

/// In die Konsole des Aufrufers schreiben. Ein Programm ohne eigene Konsole hängt sich
/// dafür an die des Elternprozesses; `CONOUT$` ist danach die Ausgabe.
fn say(text: &str) {
    // SAFETY: reiner Aufruf ohne Zeiger; scheitert er, gibt es eben keine Ausgabe.
    unsafe {
        let _ = windows::Win32::System::Console::AttachConsole(windows::Win32::System::Console::ATTACH_PARENT_PROCESS);
    }
    match std::fs::OpenOptions::new().write(true).open("CONOUT$") {
        Ok(mut f) => {
            let _ = writeln!(f, "{text}");
        }
        Err(_) => println!("{text}"),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value_after = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).filter(|v| !v.starts_with("--"));
    let app = value_after("--app").map(PathBuf::from);
    let wd = Watchdog::system(deck_core::paths::app_dir(), app);
    let code = if args.iter().any(|a| a == "--status") {
        let (code, text) = wd.status();
        say(&text);
        code
    } else if args.iter().any(|a| a == "--loop") {
        let every = value_after("--loop").and_then(|v| v.parse::<f64>().ok()).map_or(LOOP_EVERY, |s| Duration::from_secs_f64(s.max(15.0)));
        wd.run_loop(every)
    } else {
        wd.once()
    };
    std::process::exit(code);
}
