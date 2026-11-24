//! `wsl.exe` fragen und durch `wsl.exe` arbeiten: Distros auflisten, Home und
//! Einhängepunkt einer Distro ermitteln, nachsehen, ob `claude` dort installiert ist.
//!
//! Alles hinter [`WslCli`]: Tests geben eine Attrappe, kein Test startet `wsl.exe`.
//! Öffentlich, weil auch Erweiterungen durch `wsl.exe` arbeiten (etwa git in der Distro).

use std::io::Read;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use deck_core::wsl::{self, Wsl, decode_wsl_output, parse_distros};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Erster Aufruf startet womöglich die Distro (einige Sekunden).
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(40);

/// `wsl.exe` mit Argumenten aufrufen: (Exit-Code, stdout roh). `Err` = nicht startbar
/// oder Zeitlimit.
pub trait WslCli: Send + Sync {
    fn run(&self, args: &[String], timeout: Duration) -> Result<(i32, Vec<u8>), String>;
}

/// Die echte Fassung: versteckt, ohne Eingabe, mit Zeitlimit. `WSL_UTF8=1` lässt
/// `wsl.exe` selbst UTF-8 sprechen (ältere Fassungen ignorieren es, dann UTF-16).
#[derive(Debug, Default, Clone, Copy)]
pub struct WslExe;

impl WslCli for WslExe {
    fn run(&self, args: &[String], timeout: Duration) -> Result<(i32, Vec<u8>), String> {
        crate::guard::check("wsl.exe aufrufen").map_err(|e| e.to_string())?;
        let mut child = Command::new("wsl.exe")
            .args(args)
            .env("WSL_UTF8", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("wsl.exe nicht startbar: {}", e.kind()))?;
        let mut out = child.stdout.take();
        let reader = std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(o) = out.as_mut() {
                let _ = o.read_to_end(&mut buf);
            }
            buf
        });
        let deadline = Instant::now() + timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break Some(s),
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(30)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
            }
        };
        let bytes = reader.join().unwrap_or_default();
        status.map(|s| (s.code().unwrap_or(1), bytes)).ok_or_else(|| format!("wsl.exe: Zeitlimit {} s", timeout.as_secs()))
    }
}

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| (*s).to_string()).collect()
}

/// Die Distros zur Wahl: (Name, Standard?). Leer = WSL fehlt oder hat keine Distro.
pub fn distros(cli: &dyn WslCli) -> Vec<(String, bool)> {
    match cli.run(&args(&["-l", "-v"]), PROBE_TIMEOUT) {
        Ok((0, out)) => parse_distros(&decode_wsl_output(&out)),
        _ => Vec::new(),
    }
}

/// Home und Einhängepunkt der Distro. `None`: Distro startet nicht oder antwortet nicht.
pub fn probe(cli: &dyn WslCli, distro: &str) -> Option<Wsl> {
    let script = r#"printf '%s\n' "$HOME"; wslpath -u 'C:\' 2>/dev/null"#;
    let (code, out) = cli.run(&args(&["-d", distro, "-e", "sh", "-c", script]), PROBE_TIMEOUT).ok()?;
    if code != 0 {
        return None;
    }
    let text = decode_wsl_output(&out);
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let home = lines.next().filter(|h| h.starts_with('/'))?;
    // `wslpath -u 'C:\'` ergibt `/mnt/c/`: der Einhängepunkt ist alles davor.
    let mount = lines.next().and_then(|m| m.trim_end_matches('/').strip_suffix('c').map(String::from)).unwrap_or_else(|| wsl::DEFAULT_MOUNT_ROOT.into());
    Some(Wsl::new(distro, home, &mount))
}

/// Pfad von `claude` in einer Login-Shell der Distro (so findet ihn auch das Startskript),
/// `None` = nicht installiert.
pub fn claude_path(cli: &dyn WslCli, distro: &str) -> Option<String> {
    let (code, out) = cli.run(&args(&["-d", distro, "-e", "bash", "-lic", "command -v claude"]), PROBE_TIMEOUT).ok()?;
    let text = decode_wsl_output(&out);
    let path = text.lines().map(str::trim).rfind(|l| l.starts_with('/'))?;
    (code == 0).then(|| path.to_string())
}

#[cfg(test)]
#[path = "wsl_cli_tests.rs"]
mod tests;
