//! Die echte Fassung von [`Launcher`]: `wt.exe` ohne eigenes Konsolenfenster starten,
//! ohne auf das Ende zu warten (wie `subprocess.Popen` in `ops/wt_host.py`).

use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

use crate::applog::note;
use crate::wt_setup::Launcher;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub struct ProcessLauncher;

impl Launcher for ProcessLauncher {
    fn launch(&self, argv: &[String]) -> bool {
        if deck_win::effects_forbidden() {
            note("launch gesperrt (Tests)");
            return false;
        }
        let Some((exe, args)) = argv.split_first() else { return false };
        let started = Command::new(exe)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            // Die Variablen einer umgebenden Claude-Session nicht weiterreichen: das
            // Startskript räumt sie zwar auch ab, aber wt.exe startet womöglich erst
            // jetzt und vererbt sie dann jeder Shell, die es je öffnet.
            .env_remove("CLAUDECODE")
            .env_remove("CLAUDE_CODE_ENTRYPOINT")
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
        match started {
            Ok(_) => true,
            Err(e) => {
                note(format!("wt_host.launch {exe}: {e}"));
                false
            }
        }
    }
}
