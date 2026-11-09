//! Was der Windows-Terminal-Host ([`crate::wt_host::WtHost`]) von außen bekommt: die
//! Programme und Pfade zum Öffnen eines Agenten und die Wirkungen nach außen (Fenster,
//! Prozesse, Konsole, Starten von `wt.exe`), gebündelt und in Tests durch Attrappen
//! ersetzbar.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use deck_core::paths::WT_HOSTS_FILE;
use deck_core::wt;
use deck_win::effects::{ConsoleInput, Desktop, Processes};
use deck_win::which::which;

/// Startet `wt.exe`. Eigener Trait, damit Tests nie ein Terminal öffnen.
pub trait Launcher: Send + Sync {
    fn launch(&self, argv: &[String]) -> bool;
}

/// Die Wirkungen nach außen, gebündelt.
#[derive(Clone)]
pub struct WtEffects {
    pub desktop: Arc<dyn Desktop>,
    pub procs: Arc<dyn Processes>,
    pub console: Arc<dyn ConsoleInput>,
    pub launcher: Arc<dyn Launcher>,
}

/// Programme und Pfade für das Öffnen eines Agenten.
#[derive(Debug, Clone)]
pub struct WtSetup {
    pub hosts_file: PathBuf,
    pub state_dir: PathBuf,
    pub script: PathBuf,
    pub wt_exe: String,
    pub shell: String,
    /// Verstecktes Profil „Agent Deck“, falls in den WT-Einstellungen vorhanden.
    pub profile: Option<String>,
}

/// Programm auf dem PATH als Text, sonst die Vorgabe (wie `shutil.which(...) or ...`).
fn which_or(name: &str, fallback: &str) -> String {
    which(name).map_or_else(|| fallback.to_string(), |p| p.to_string_lossy().into_owned())
}

/// Die echte Einrichtung: `wt.exe`, PowerShell 7 (sonst Windows PowerShell) und das
/// Profil „Agent Deck“, wenn es in den Einstellungen von Windows Terminal steht.
pub fn real_setup(app_dir: &Path, state_dir: &Path, script: PathBuf) -> WtSetup {
    let local = deck_core::paths::local_app_data();
    let wt_settings = local.join("Packages").join("Microsoft.WindowsTerminal_8wekyb3d8bbwe").join("LocalState").join("settings.json");
    let profile = fs::read_to_string(wt_settings).ok().filter(|t| t.contains(&format!("\"{}\"", wt::PROFILE))).map(|_| wt::PROFILE.to_string());
    WtSetup {
        hosts_file: app_dir.join(WT_HOSTS_FILE),
        state_dir: state_dir.to_path_buf(),
        script,
        wt_exe: which_or("wt.exe", "wt.exe"),
        shell: which_or("pwsh.exe", "powershell.exe"),
        profile,
    }
}
