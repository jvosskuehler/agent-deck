//! `installed.json`: was die Einrichtung auf diesem Rechner hinzugefügt hat, damit die
//! Deinstallation genau das wieder herausnimmt und nichts anderes.
//!
//! `%LOCALAPPDATA%\claude-agent-deck\installed.json`. Jeder Lauf von `deck-setup`
//! liest die Datei, ergänzt, was er neu angelegt hat, und schreibt sie zurück; ein Update
//! behält also, was die erste Installation eingetragen hat. Was schon vor dem Deck da
//! war, steht nie darin.
//!
//! **Weg kommt nur, was seitdem niemand geändert hat:** eine Claude-Vorgabe nur, solange
//! sie noch den geschriebenen Wert hat; eine Datei nur, solange ihr Inhalt noch zum
//! gespeicherten Fingerabdruck passt. Was der Mensch angepasst hat, gehört ihm.
//!
//! Lesen ist tolerant: fehlt die Datei oder ist sie kaputt, gilt sie als leer (dann
//! räumt die Deinstallation nur, was am Namen eindeutig dem Deck gehört).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::fsutil::normcase;
use crate::guard;

pub const FILE: &str = "installed.json";

/// `%LOCALAPPDATA%\claude-agent-deck\installed.json`.
pub fn path() -> PathBuf {
    deck_core::paths::app_dir().join(FILE)
}

/// Eine Vorgabe in Claudes Einstellungen, die vorher fehlte. `key` mit Punkt für
/// verschachtelte Schlüssel (`permissions.defaultMode`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClaudeDefault {
    pub key: String,
    pub value: Value,
}

/// Eine Schrift, die das Deck für den Benutzer installiert hat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Font {
    pub file: String,
    /// Name des Werts unter `HKCU\…\Fonts`.
    pub name: String,
    /// Ziel der Kopie (`%LOCALAPPDATA%\Microsoft\Windows\Fonts\<file>`).
    pub path: String,
}

/// Eine Datei, die das Deck angelegt hat, mit Fingerabdruck ihres Inhalts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnFile {
    pub path: String,
    pub hash: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Installed {
    pub claude_defaults: Vec<ClaudeDefault>,
    /// Die frühere Infozeile wurde gesichert (Pfad der Sicherung).
    pub statusline_backup: Option<String>,
    pub fonts: Vec<Font>,
    pub files: Vec<OwnFile>,
    pub shortcut: Option<String>,
    pub wt_fragment: Option<String>,
    /// Wert unter `HKCU\…\Run\AgentDeck`.
    pub autostart: Option<String>,
    pub apps_entry: bool,
    /// `"theme": "system"` in die settings.json von Windows Terminal eingetragen.
    pub wt_theme_added: bool,
    /// Hooks und Infozeile in den Claude-Einstellungen einer WSL-Distro
    /// (`deck-setup install --wsl`, [`crate::install_wsl`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wsl: Option<WslInstalled>,
}

/// Was `install --wsl` in der Distro eingetragen hat.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WslInstalled {
    pub distro: String,
    /// Claudes `settings.json` in der Distro, als Windows-Pfad (`\\wsl.localhost\…`).
    pub settings: String,
    pub claude_defaults: Vec<ClaudeDefault>,
    pub statusline_backup: Option<String>,
    /// Ordner der Linux-Fassungen von deck-report und deck-statusline in der Distro.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linux_bin: Option<String>,
}

/// Fingerabdruck eines Inhalts (FNV-1a, 64 Bit). Kein Schutz gegen Absicht, nur die Frage
/// „hat jemand die Datei angefasst?“; dafür stabil über alle Rust-Fassungen.
pub fn hash(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("fnv1a64:{h:016x}")
}

fn same(a: &str, b: &str) -> bool {
    normcase(a) == normcase(b)
}

impl Installed {
    /// Lesen; fehlt oder kaputt: leer.
    pub fn load(file: &Path) -> Installed {
        std::fs::read(file)
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
            .and_then(|t| serde_json::from_str(deck_core::paths::strip_bom(&t)).ok())
            .unwrap_or_default()
    }

    /// Atomar schreiben; ist nichts mehr verzeichnet, verschwindet die Datei.
    pub fn save(&self, file: &Path) -> std::io::Result<()> {
        if same(&file.to_string_lossy(), &path().to_string_lossy()) {
            guard::check("installed.json schreiben")?;
        }
        if self.is_empty() {
            return match std::fs::remove_file(file) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            };
        }
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        deck_core::paths::atomic_write(file, &json)
    }

    pub fn is_empty(&self) -> bool {
        self == &Installed::default()
    }

    pub fn record_default(&mut self, key: &str, value: Value) {
        self.claude_defaults.retain(|d| d.key != key);
        self.claude_defaults.push(ClaudeDefault { key: key.into(), value });
    }

    pub fn file(&self, p: &Path) -> Option<&OwnFile> {
        self.files.iter().find(|f| same(&f.path, &p.to_string_lossy()))
    }

    pub fn record_file(&mut self, p: &Path, hash: String) {
        self.forget_file(p);
        self.files.push(OwnFile { path: p.to_string_lossy().into_owned(), hash });
    }

    pub fn forget_file(&mut self, p: &Path) {
        self.files.retain(|f| !same(&f.path, &p.to_string_lossy()));
    }

    pub fn font(&self, file: &str) -> Option<&Font> {
        self.fonts.iter().find(|f| f.file.eq_ignore_ascii_case(file))
    }

    pub fn record_font(&mut self, font: Font) {
        self.fonts.retain(|f| !f.file.eq_ignore_ascii_case(&font.file));
        self.fonts.push(font);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn what_was_recorded_reads_back_the_same() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("deck").join(FILE);
        let mut m = Installed::default();
        m.record_default("effortLevel", json!("high"));
        m.record_default("permissions.defaultMode", json!("bypassPermissions"));
        m.record_default("effortLevel", json!("high"));
        m.record_file(Path::new(r"C:\x\themes\papier.json"), hash(b"a"));
        m.record_file(Path::new(r"c:\X\Themes\papier.json"), hash(b"b"));
        m.record_font(Font { file: "IBMPlexMono-Regular.ttf".into(), name: "IBM Plex Mono (TrueType)".into(), path: r"C:\f\x.ttf".into() });
        m.statusline_backup = Some(r"C:\x\statusline.before.json".into());
        m.shortcut = Some(r"C:\x\Agent Deck.lnk".into());
        m.wt_fragment = Some(r"C:\x\agent-deck.json".into());
        m.autostart = Some(r#""C:\x\agent-deck.exe""#.into());
        m.apps_entry = true;
        assert_eq!(m.claude_defaults.len(), 2, "ein Schlüssel steht einmal da");
        assert_eq!(m.files.len(), 1, "Pfade gleich ohne Rücksicht auf Groß/klein");
        m.save(&file).unwrap();
        assert_eq!(Installed::load(&file), m);
    }

    #[test]
    fn a_missing_or_broken_file_counts_as_empty_and_an_empty_record_leaves_no_file() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join(FILE);
        assert!(Installed::load(&file).is_empty());
        std::fs::write(&file, "{ kaputt").unwrap();
        assert!(Installed::load(&file).is_empty());
        std::fs::write(&file, "\u{FEFF}{\"apps_entry\": true, \"neu\": 1}").unwrap();
        assert!(Installed::load(&file).apps_entry, "BOM und unbekannte Felder schaden nicht");
        Installed::default().save(&file).unwrap();
        assert!(!file.exists());
        Installed::default().save(&file).unwrap();
    }

    #[test]
    fn the_fingerprint_is_stable_and_tells_contents_apart() {
        assert_eq!(hash(b""), "fnv1a64:cbf29ce484222325");
        assert_ne!(hash(b"papier"), hash(b"Papier"));
    }

    #[test]
    fn the_real_record_is_locked_in_tests() {
        assert!(Installed { apps_entry: true, ..Default::default() }.save(&path()).is_err());
    }
}
