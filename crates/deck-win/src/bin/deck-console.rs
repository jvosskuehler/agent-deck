//! Wegwerfprozess: schreibt Tasten in die Konsole eines fremden Prozesses.
//!
//! Aufruf: `deck-console <pid>`, die Schritte kommen über stdin (Format in
//! `deck_win::console_input::encode_steps`). Antwort auf stdout, genau eine Zeile:
//! `ok <ereignisse>` oder `fail <grund>`; Exitcode 0 bzw. 1.
//!
//! Warum ein eigener Prozess: siehe Kopf von `console_input.rs`. Kurz: wer an einer
//! fremden Konsole hängt, stirbt mit ihr. Darum auch das Windows-Subsystem: der Prozess
//! bekommt gar keine eigene Konsole, es blitzt nichts auf, und es gibt nichts, was
//! vor dem Anhängen erst freigegeben werden müsste.

#![windows_subsystem = "windows"]

use std::io::Read;
use std::process::ExitCode;

use deck_win::console_input::{decode_steps, inject_into_attached_console};

fn run() -> Result<usize, String> {
    let pid: u32 = std::env::args()
        .nth(1)
        .ok_or("PID fehlt")?
        .parse()
        .map_err(|e| format!("PID unlesbar: {e}"))?;
    let mut wire = String::new();
    std::io::stdin()
        .read_to_string(&mut wire)
        .map_err(|e| format!("stdin: {e}"))?;
    let steps = decode_steps(&wire)?;
    inject_into_attached_console(pid, &steps).map_err(|e| e.to_string())
}

fn main() -> ExitCode {
    // Ausgabe statt Panik-Text: der Aufrufer liest nur die eine Zeile.
    match run() {
        Ok(n) => {
            println!("ok {n}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            println!("fail {}", e.replace('\n', " "));
            ExitCode::from(1)
        }
    }
}
