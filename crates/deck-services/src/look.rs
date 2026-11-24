//! Der „Papier“-Look ab Werk: Schrift, Farben in Claude Code und der Shader, der den
//! blassen Vorschlagstext im hellen Terminal lesbar macht. Die Teile liegen im Paket in
//! `resources\papier` (im Checkout ebenso):
//!
//! | Quelle | Ziel |
//! |---|---|
//! | `fonts\IBMPlexMono-*.ttf` | `%LOCALAPPDATA%\Microsoft\Windows\Fonts`, angemeldet unter `HKCU\…\Fonts` |
//! | `claude-theme-papier.json` | `<Claude-Ordner>\themes\papier.json` (eigenes Theme `custom:papier`) |
//! | `agent-deck-wt.settings.json` | `<Claude-Ordner>\agent-deck-wt.settings.json`; `wt_agent.ps1` gibt sie nur den Agenten des Decks mit |
//! | `wt-faint-hell.hlsl` | `%LOCALAPPDATA%\claude-agent-deck\wt-faint-hell.hlsl` (Profil „Agent Deck“) |
//!
//! Alles pro Benutzer, ohne Administratorrechte. Eine Schrift, die es unter diesem
//! Dateinamen oder Namen schon gibt (beim Benutzer oder im System), bleibt, wie sie ist.
//! Eine Datei, die schon da ist und nicht von uns stammt oder die der Mensch geändert
//! hat, wird nie überschrieben. Was angelegt wurde, steht in `installed.json`
//! ([`crate::installed`]); `remove` nimmt genau das wieder heraus.

use std::path::{Path, PathBuf};

use crate::fsutil::basename;
use crate::guard;
use crate::installed::{Font, Installed, hash};
use crate::report::{Grade, Report};
use crate::user_system::{FONTS_KEY, RUN_ONCE_KEY, RegValue, UserSystem};

/// (Datei, Name des Registrierungswerts).
pub const FONTS: [(&str, &str); 4] = [
    ("IBMPlexMono-Regular.ttf", "IBM Plex Mono (TrueType)"),
    ("IBMPlexMono-Italic.ttf", "IBM Plex Mono Italic (TrueType)"),
    ("IBMPlexMono-Bold.ttf", "IBM Plex Mono Bold (TrueType)"),
    ("IBMPlexMono-BoldItalic.ttf", "IBM Plex Mono Bold Italic (TrueType)"),
];
pub const FONT_FACE: &str = "IBM Plex Mono";
const THEME: &str = "claude-theme-papier.json";
const WT_SETTINGS: &str = "agent-deck-wt.settings.json";
pub const SHADER: &str = "wt-faint-hell.hlsl";

/// Wohin die Teile gehen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookPaths {
    /// Claudes Ordner (`CLAUDE_CONFIG_DIR`, sonst `~\.claude`).
    pub claude_dir: PathBuf,
    /// `%LOCALAPPDATA%\claude-agent-deck`.
    pub app_dir: PathBuf,
    /// `%LOCALAPPDATA%\Microsoft\Windows\Fonts`.
    pub fonts_dir: PathBuf,
}

impl LookPaths {
    pub fn real() -> LookPaths {
        LookPaths {
            claude_dir: deck_core::paths::claude_dir(),
            app_dir: deck_core::paths::app_dir(),
            fonts_dir: deck_core::paths::local_app_data().join("Microsoft").join("Windows").join("Fonts"),
        }
    }

    pub fn theme(&self) -> PathBuf {
        self.claude_dir.join("themes").join("papier.json")
    }

    pub fn wt_settings(&self) -> PathBuf {
        self.claude_dir.join(WT_SETTINGS)
    }

    pub fn shader(&self) -> PathBuf {
        self.app_dir.join(SHADER)
    }
}

/// Die echten Ziele sind in Tests gesperrt; Wegwerf-Ordner sind frei.
fn check_target(p: &LookPaths) -> std::io::Result<()> {
    if *p == LookPaths::real() {
        guard::check("Schrift und Farben einrichten")?;
    }
    Ok(())
}

/// Eine Datei hinlegen, ohne Fremdes oder Geändertes zu überschreiben.
fn place(want: &[u8], dst: &Path, label: &str, m: &mut Installed, r: &mut Report) {
    let ours = m.file(dst).map(|f| f.hash.clone());
    match std::fs::read(dst) {
        Ok(have) if have == want => {
            r.push(Grade::Ok, format!("{label} ist auf Stand"));
            return;
        }
        Ok(have) if ours.as_deref() != Some(hash(&have).as_str()) => {
            r.push(Grade::Info, format!("{label}: eigene Fassung in {} bleibt", dst.display()));
            return;
        }
        _ => {}
    }
    match deck_core::paths::atomic_write(dst, want) {
        Ok(()) => {
            m.record_file(dst, hash(want));
            r.changed = true;
            r.push(Grade::Info, format!("{label}: {}", dst.display()));
        }
        Err(e) => r.push(Grade::Warn, format!("{label} nicht angelegt ({}): {e}", dst.display())),
    }
}

/// Gibt es diese Schrift schon (nicht von uns)? Gleicher Dateiname oder gleicher Name,
/// beim Benutzer oder im System.
fn font_elsewhere(file: &str, name: &str, own: Option<&Font>, sys: &dyn UserSystem) -> bool {
    let hit = |(n, d): &(String, String)| basename(d).eq_ignore_ascii_case(file) || n.eq_ignore_ascii_case(name);
    let mine = |(n, d): &(String, String)| own.is_some_and(|f| f.name.eq_ignore_ascii_case(n) && f.path.eq_ignore_ascii_case(d));
    sys.user_values(FONTS_KEY).iter().any(|v| hit(v) && !mine(v)) || sys.machine_values(FONTS_KEY).iter().any(hit)
}

fn install_font(src: &Path, file: &str, name: &str, p: &LookPaths, sys: &dyn UserSystem, m: &mut Installed, r: &mut Report) {
    let own = m.font(file).cloned();
    if font_elsewhere(file, name, own.as_ref(), sys) {
        r.push(Grade::Ok, format!("Schrift {file} ist schon da"));
        return;
    }
    let dst = p.fonts_dir.join(file);
    let Ok(want) = std::fs::read(src.join("fonts").join(file)) else {
        r.push(Grade::Warn, format!("Schrift {file} fehlt im Paket"));
        return;
    };
    let path = dst.to_string_lossy().into_owned();
    let registered = sys.user_value(FONTS_KEY, name).is_some_and(|d| d.eq_ignore_ascii_case(&path));
    if registered && std::fs::read(&dst).is_ok_and(|have| have == want) {
        r.push(Grade::Ok, format!("Schrift {file} ist auf Stand"));
        return;
    }
    // Eine geladene Schrift ist gesperrt; gleicher Inhalt muss nicht neu geschrieben werden.
    if !std::fs::read(&dst).is_ok_and(|have| have == want)
        && let Err(e) = std::fs::create_dir_all(&p.fonts_dir).and_then(|()| std::fs::write(&dst, &want))
    {
        r.push(Grade::Warn, format!("Schrift {file} nicht kopiert: {e}"));
        return;
    }
    if let Err(e) = sys.set(FONTS_KEY, name, &RegValue::Str(path.clone())) {
        r.push(Grade::Warn, format!("Schrift {file} nicht angemeldet: {e}"));
        return;
    }
    sys.font_session(&dst, true);
    m.record_font(Font { file: file.into(), name: name.into(), path });
    r.changed = true;
    r.push(Grade::Info, format!("Schrift {file} installiert"));
}

/// Schrift, Theme, Einstellungen für die Deck-Agenten und Shader einrichten. `src` ist
/// der Ordner `papier` aus dem Paket.
pub fn install(src: &Path, p: &LookPaths, sys: &dyn UserSystem, m: &mut Installed) -> Report {
    let mut r = Report::default();
    if let Err(e) = check_target(p) {
        r.push(Grade::Fail, e.to_string());
        return r;
    }
    if !src.is_dir() {
        r.push(Grade::Warn, format!("{} fehlt: Schrift und Farben bleiben, wie sie sind", src.display()));
        return r;
    }
    for (file, name) in FONTS {
        install_font(src, file, name, p, sys, m, &mut r);
    }
    for (from, dst, label) in [(THEME, p.theme(), "Farben für Claude"), (WT_SETTINGS, p.wt_settings(), "Einstellungen der Deck-Agenten"), (SHADER, p.shader(), "Shader")] {
        match std::fs::read(src.join(from)) {
            Ok(want) => place(&want, &dst, label, m, &mut r),
            Err(_) => r.push(Grade::Warn, format!("{from} fehlt im Paket")),
        }
    }
    r
}

/// Was `install` angelegt hat, wieder herausnehmen, soweit es unverändert ist.
pub fn remove(p: &LookPaths, sys: &dyn UserSystem, m: &mut Installed) -> Report {
    let mut r = Report::default();
    if let Err(e) = check_target(p) {
        r.push(Grade::Fail, e.to_string());
        return r;
    }
    for f in std::mem::take(&mut m.fonts) {
        remove_font(&f, sys, &mut r);
    }
    let mut themes_dirs = Vec::new();
    for f in std::mem::take(&mut m.files) {
        let path = PathBuf::from(&f.path);
        if let Some(dir) = path.parent().filter(|d| d.file_name().is_some_and(|n| n == "themes")) {
            themes_dirs.push(dir.to_path_buf());
        }
        match std::fs::read(&path) {
            Err(_) => {}
            Ok(have) if hash(&have) != f.hash => r.push(Grade::Info, format!("{} wurde geändert und bleibt", path.display())),
            Ok(_) => match std::fs::remove_file(&path) {
                Ok(()) => {
                    r.changed = true;
                    r.push(Grade::Info, format!("entfernt: {}", path.display()));
                }
                Err(e) => r.push(Grade::Warn, format!("{} nicht entfernt: {e}", path.display())),
            },
        }
    }
    // Der Themes-Ordner geht mit, wenn er dadurch leer ist (auch der in einer WSL-Distro,
    // `deck-setup look --wsl`: dessen Pfad steht nur in den entfernten Dateien).
    let _ = std::fs::remove_dir(p.theme().parent().unwrap_or(Path::new("")));
    for d in &themes_dirs {
        let _ = std::fs::remove_dir(d);
    }
    r
}

fn remove_font(f: &Font, sys: &dyn UserSystem, r: &mut Report) {
    let path = PathBuf::from(&f.path);
    // Zeigt die Anmeldung inzwischen woandershin, hat jemand anders sie übernommen.
    if !sys.user_value(FONTS_KEY, &f.name).is_none_or(|d| d.eq_ignore_ascii_case(&f.path)) {
        r.push(Grade::Info, format!("Schrift {} gehört nicht mehr dem Deck und bleibt", f.file));
        return;
    }
    if let Err(e) = sys.delete_value(FONTS_KEY, &f.name) {
        r.push(Grade::Warn, format!("Schrift {} nicht abgemeldet: {e}", f.file));
        return;
    }
    sys.font_session(&path, false);
    r.changed = true;
    match std::fs::remove_file(&path) {
        Ok(()) => r.push(Grade::Info, format!("Schrift {} entfernt", f.file)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => {
            // Noch geladen (ein offenes Terminal): beim nächsten Anmelden löschen. Dann ist
            // sie nicht mehr angemeldet und damit frei.
            let cmd = format!("cmd.exe /d /c del /f /q \"{}\"", path.display());
            match sys.set(RUN_ONCE_KEY, &format!("AgentDeck-{}", f.file), &RegValue::Str(cmd)) {
                Ok(()) => r.push(Grade::Warn, format!("Die Schrift {} ist noch in Benutzung und wird beim nächsten Anmelden gelöscht.", f.file)),
                Err(e) => r.push(Grade::Warn, format!("Schrift {} nicht gelöscht: {e}", path.display())),
            }
        }
    }
}

#[cfg(test)]
#[path = "look_tests.rs"]
mod tests;
