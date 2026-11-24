//! `deck-wsl <spec>`: steht im Windows-Terminal-Fenster eines WSL-Agenten an der Stelle,
//! an der bei Windows-Agenten PowerShell mit `wt_agent.ps1` steht.
//!
//! Es schreibt die Herkunftsmarke `state\<slot>.wtpid` mit der eigenen PID und Startzeit
//! (daran sieht das Deck, dass der Slot lebt), setzt die Umgebung für die Hooks samt
//! `WSLENV` und startet `wsl.exe` mit dem Startskript in der Distro. Es wartet, bis
//! `wsl.exe` endet, und endet mit dessen Exit-Code. Die Rechnung steht in
//! `deck_core::wsl_launch`; hier ist nur, was Windows dazu braucht.
//!
//! Konsolenprogramm: `wsl.exe` erbt die Konsole des Fensters, und das Deck schreibt
//! Tasten über diese Konsole (`deck-console`, `AttachConsole` auf diese PID).

use std::process::{Command, ExitCode};

use deck_core::wsl_launch::{WslSpec, plan};
use windows::Win32::System::Console::SetConsoleCtrlHandler;

/// Nach einem Fehler so lange offen bleiben, damit man die Zeile lesen kann (das Profil
/// „Agent Deck“ schließt das Fenster sonst sofort).
const SHOW_ERROR_S: u64 = 30;

fn fail(msg: &str) -> ExitCode {
    eprintln!("Agent Deck: {msg}");
    std::thread::sleep(std::time::Duration::from_secs(SHOW_ERROR_S));
    ExitCode::from(1)
}

fn main() -> ExitCode {
    // Strg+C gehört dem Agenten in WSL, nicht diesem Wartenden. `wsl.exe` liest die
    // Konsole roh; käme das Signal doch an, beendete es sonst den Starter samt Marke.
    // SAFETY: kein Handler, nur „Strg+C ignorieren“ für diesen Prozess.
    let _ = unsafe { SetConsoleCtrlHandler(None, true) };
    let Some(raw) = std::env::args().nth(1) else { return fail("Aufruf: deck-wsl <spec>") };
    let Some(spec) = deck_core::wt::decode_spec(&raw).as_ref().and_then(WslSpec::from_json) else {
        return fail("Startangaben unlesbar");
    };
    let pid = std::process::id();
    let created = deck_win::procs::process_start(pid).unwrap_or(0);
    let p = plan(&spec, pid, created, std::env::var("WSLENV").ok().as_deref());
    // Die Marke ZUERST, wie in wt_agent.ps1: an ihr erkennt das Deck, dass der Slot lebt.
    if let Err(e) = deck_core::paths::atomic_write(std::path::Path::new(&p.pidfile), p.pidfile_body.as_bytes()) {
        return fail(&format!("Zustandsdatei nicht schreibbar ({}): {e}", p.pidfile));
    }
    let mut cmd = Command::new("wsl.exe");
    cmd.args(&p.argv);
    for k in &p.remove_env {
        cmd.env_remove(k);
    }
    for (k, v) in &p.set_env {
        cmd.env(k, v);
    }
    match cmd.status() {
        Ok(s) => ExitCode::from(u8::try_from(s.code().unwrap_or(1)).unwrap_or(1)),
        Err(e) => fail(&format!("wsl.exe ließ sich nicht starten: {e}. Ist WSL installiert?")),
    }
}
