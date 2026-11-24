//! Hooks und Infozeile in Claudes Einstellungen IN DER WSL-DISTRO (`deck-setup install
//! --wsl <Distro>`), prüfen und wieder herausnehmen.
//!
//! Dieselben Regeln wie unter Windows (`crate::install`, `deck_core::claude::hook_setup`),
//! nur an anderer Stelle:
//!
//! * Die Datei ist `~/.claude/settings.json` der Distro, gelesen und geschrieben über
//!   `\\wsl.localhost\<Distro>\…`.
//! * Die Programme sind dieselben Windows-Programme in `bin`, aufgerufen über WSL-Interop
//!   mit ihrem Linux-Pfad (`"/mnt/c/Users/…/bin/deck-report.exe" thinking || exit 0`).
//!   Claude Code führt Hooks in Linux mit `sh`/`bash` aus: `|| exit 0` greift dort wie in
//!   Git Bash.
//! * Eine fremde Infozeile kommt nach `statusline.before.wsl.json`; `deck-statusline`
//!   ruft sie in der Distro weiter auf (`deck_hooks::chain`, `wsl.exe -e sh -c`).
//!
//! **Linux-Fassungen der Hooks.** Ein Windows-Programm über Interop kostet je Aufruf rund
//! 40 ms mehr als ein Linux-Programm, und je Werkzeugaufruf laufen zwei Hooks. Darum liegen
//! `deck-report` und `deck-statusline` auch als statische Linux-Programme im Paket
//! (`bin\linux`); `install --wsl` kopiert sie nach `~/.local/share/agent-deck/bin` (aus
//! `/mnt/c` gestartet wären sie nicht schneller, drvfs liest das Programm jedes Mal neu).
//! Das Kommando ruft sie zuerst auf und nur, wenn sie fehlen, die `.exe`.
//!
//! Was eingetragen wurde, steht in `installed.json` unter `wsl`
//! ([`crate::installed::WslInstalled`]); `uninstall` nimmt genau das wieder heraus.

use std::path::{Path, PathBuf};

use deck_core::claude::hook_setup;
use deck_core::wsl::Wsl;

use crate::install::{self, StatusLine};
use crate::wsl_cli::{PROBE_TIMEOUT, WslCli};
use crate::installed::{Installed, WslInstalled};
use crate::report::{Grade, Report};

/// Wohin `install --wsl` schreibt.
#[derive(Debug, Clone)]
pub struct WslTarget {
    pub wsl: Wsl,
    /// `settings.json` in der Distro, als Windows-Pfad.
    pub settings: PathBuf,
    /// `bin` des Decks als Linux-Pfad (`/mnt/c/…/claude-agent-deck/bin`).
    pub bin_linux: String,
    /// Sicherung einer fremden Infozeile (`%LOCALAPPDATA%\claude-agent-deck\statusline.before.wsl.json`).
    pub saved: PathBuf,
    /// Ordner der Linux-Fassungen der Hooks in der Distro, wenn sie dort liegen.
    pub linux_bin: Option<String>,
}

/// Wohin die Linux-Fassungen in der Distro kommen (unter `$HOME`).
pub const LINUX_BIN_DIR: &str = ".local/share/agent-deck/bin";
/// Die Linux-Fassungen (in `bin\linux` des Decks).
pub const LINUX_HOOKS: [&str; 2] = ["deck-report", "deck-statusline"];

impl WslTarget {
    /// Aus der ermittelten Distro (`wsl_cli::probe`) und dem Windows-Ordner `bin`.
    /// `None`: ohne Home kein Claude-Ordner, oder `bin` liegt nicht auf einem Laufwerk.
    pub fn new(wsl: Wsl, bin: &Path, app_dir: &Path) -> Option<WslTarget> {
        let settings = PathBuf::from(wsl.claude_dir()?).join("settings.json");
        let bin_linux = wsl.to_linux(&bin.to_string_lossy())?;
        Some(WslTarget { settings, bin_linux, saved: app_dir.join(deck_core::paths::STATUSLINE_BACKUP_WSL_FILE), wsl, linux_bin: None })
    }

    /// Die Linux-Fassungen aus `src` (Windows-Ordner `bin\linux`) in die Distro kopieren,
    /// ausführbar (`install -m 755`, über die Freigabe geschrieben wären sie es nicht).
    /// Danach zeigen die Hooks auf sie. `Err` mit Grund: dann bleibt es bei den `.exe`.
    pub fn place_linux_hooks(&mut self, cli: &dyn WslCli, src: &Path) -> Result<(), String> {
        if !LINUX_HOOKS.iter().all(|f| src.join(f).is_file()) {
            return Err(format!("Linux-Fassungen fehlen in {}", src.display()));
        }
        let from = self.wsl.to_linux(&src.to_string_lossy()).ok_or("Programmordner von WSL aus nicht erreichbar")?;
        let to = format!("{}/{LINUX_BIN_DIR}", self.wsl.home);
        let script = r#"mkdir -p "$1" && install -m 755 "$2/deck-report" "$2/deck-statusline" "$1/""#;
        let argv: Vec<String> = ["-d", &self.wsl.distro, "-e", "sh", "-c", script, "sh", &to, &from].iter().map(|s| s.to_string()).collect();
        match cli.run(&argv, PROBE_TIMEOUT) {
            Ok((0, _)) => {
                self.linux_bin = Some(to);
                Ok(())
            }
            Ok((code, _)) => Err(format!("Kopieren in die Distro scheiterte (Exit {code})")),
            Err(e) => Err(e),
        }
    }
}

/// Ein `Installed` nur mit dem, was in der Distro steht: so laufen dieselben
/// `install`/`uninstall` wie unter Windows, ohne die Windows-Einträge anzufassen.
fn view_of(w: &WslInstalled) -> Installed {
    Installed { claude_defaults: w.claude_defaults.clone(), statusline_backup: w.statusline_backup.clone(), ..Default::default() }
}

/// Hooks und Infozeile eintragen (Merker wie `install::install`).
pub fn install(t: &WslTarget, mode: StatusLine, m: &mut Installed) -> Report {
    let prev = m.wsl.clone().filter(|w| w.distro == t.wsl.distro).unwrap_or_default();
    let mut sub = view_of(&prev);
    let bin = PathBuf::from(&t.bin_linux);
    let r = install::install_with(&t.settings, &bin, mode, &t.saved, t.linux_bin.as_deref(), &mut sub);
    if r.fails() == 0 {
        m.wsl = Some(WslInstalled {
            distro: t.wsl.distro.clone(),
            settings: t.settings.to_string_lossy().into_owned(),
            claude_defaults: sub.claude_defaults,
            statusline_backup: sub.statusline_backup,
            linux_bin: t.linux_bin.clone(),
        });
    }
    r
}

/// Prüfen wie `install::check`; ob ein Programm da ist, beantwortet sein Windows-Pfad.
pub fn check(t: &WslTarget) -> Report {
    let mut r = Report::default();
    let Some(serde_json::Value::Object(s)) = deck_core::paths::load_json(&t.settings) else {
        r.push(Grade::Fail, format!("Die Claude-Einstellungen in WSL ({}) fehlen oder sind beschädigt.", t.settings.display()));
        return r;
    };
    let exists = |p: &str| Path::new(&t.wsl.to_windows(p)).exists();
    let same = |p: &str| p.to_string();
    r.extend_notes(&hook_setup::audit(&s, &t.bin_linux, &exists, &same));
    r
}

/// Die Einträge in der Distro herausnehmen, die gesicherte Infozeile zurück.
/// `saved` = Sicherung der Infozeile (`statusline.before.wsl.json` im App-Ordner); `cli`
/// nimmt die Linux-Fassungen aus der Distro.
pub fn uninstall(saved: &Path, cli: &dyn WslCli, m: &mut Installed) -> Report {
    let Some(w) = m.wsl.clone() else { return Report::default() };
    let mut sub = view_of(&w);
    let mut r = install::uninstall(Path::new(&w.settings), saved, &mut sub);
    if let Some(dir) = &w.linux_bin {
        let script = r#"rm -f "$1/deck-report" "$1/deck-statusline"; rmdir "$1" 2>/dev/null; exit 0"#;
        let argv: Vec<String> = ["-d", &w.distro, "-e", "sh", "-c", script, "sh", dir].iter().map(|s| s.to_string()).collect();
        if !matches!(cli.run(&argv, PROBE_TIMEOUT), Ok((0, _))) {
            r.push(Grade::Warn, format!("Die Hook-Programme in WSL ({dir}) ließen sich nicht entfernen."));
        }
    }
    if r.fails() == 0 {
        m.wsl = None;
    } else {
        r.push(Grade::Warn, format!("Die Claude-Einstellungen in WSL ({}) sind nicht erreichbar; die Einträge bleiben.", w.distro));
    }
    r
}

#[cfg(test)]
#[path = "install_wsl_tests.rs"]
mod tests;
