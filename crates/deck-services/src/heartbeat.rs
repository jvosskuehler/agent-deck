//! Der Herzschlag der App: woran der Wächter (`deck-watchdog`) erkennt, ob sie lebt,
//! hängt oder sauber beendet wurde. Die App schreibt, der Wächter liest.
//!
//! Warum eine Datei und kein Fenster (Python `ops/instance.py`, `BEAT_PATH`): angedockt
//! und eingeklappt hat das Deck unter Umständen kein sichtbares Fenster; wer daran „lebt
//! es noch?“ entscheidet, startet ein ZWEITES daneben, und zwei Decks sind der schlimmste
//! Zustand (zwei Pollschleifen, zwei Fokus-Rahmen, doppelte Tastenkürzel). Der Herzschlag erkennt zusätzlich ein
//! HÄNGENDES Deck (Prozess lebt, Schleife steht).
//!
//! Vertrag (in `%LOCALAPPDATA%\claude-agent-deck\`, neben `state\`):
//!
//! | Datei | Inhalt | Wer |
//! |---|---|---|
//! | `heartbeat.json` | `{"pid": 123, "exe": "C:\\…\\agent-deck.exe", "ts": 1730000000.5, "ended": null}` | App alle [`BEAT_EVERY_S`] s, atomar |
//! | `watchdog.log` | `YYYY-mm-dd HH:MM:SS Wächter: …` je Zeile | Wächter |
//! | `watchdog.lock` | PID des Dauerwächters (Text) | Wächter `--loop` |
//!
//! `ts` sind Unix-Sekunden als Float. `ended` setzt die App beim geordneten Ende:
//! `"exit"` (geschlossen, der Wächter lässt sie in Ruhe), `"restart"` (Selbst-Neustart),
//! `"panic"` (unbehandelter Fehler, aus dem Panic-Hook). Fehlt `ended` bei totem Prozess,
//! wurde sie von außen beendet oder ist hart abgestürzt. Die Dateinamen weichen bewusst von
//! denen der Python-Fassung ab (`panel.heartbeat`, `panel.lock`).

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const HEARTBEAT_FILE: &str = "heartbeat.json";
pub const WATCHDOG_LOG_FILE: &str = "watchdog.log";
pub const WATCHDOG_LOCK_FILE: &str = "watchdog.lock";
/// So oft schlägt die App.
pub const BEAT_EVERY_S: f64 = 5.0;
/// So alt darf der letzte Schlag sein, damit „lebt“ gilt.
pub const BEAT_FRESH_S: f64 = 30.0;

/// Warum die App geendet hat (Feld `ended`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    /// Vom Nutzer geschlossen: nicht neu starten.
    Exit,
    /// Selbst-Neustart: das Kind sollte laufen.
    Restart,
    /// Unbehandelter Fehler.
    Panic,
}

impl EndReason {
    pub fn as_str(self) -> &'static str {
        match self {
            EndReason::Exit => "exit",
            EndReason::Restart => "restart",
            EndReason::Panic => "panic",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Heartbeat {
    pub pid: u32,
    /// Voller Pfad der App: damit weiß der Wächter, WAS er neu startet.
    pub exe: String,
    pub ts: f64,
    #[serde(default)]
    pub ended: Option<String>,
}

pub fn heartbeat_path(app_dir: &Path) -> PathBuf {
    app_dir.join(HEARTBEAT_FILE)
}

/// Herzschlag lesen; fehlt oder kaputt → `None`.
pub fn read(app_dir: &Path) -> Option<Heartbeat> {
    serde_json::from_value(deck_core::paths::load_json(&heartbeat_path(app_dir))?).ok()
}

/// Herzschlag atomar schreiben.
pub fn write(app_dir: &Path, beat: &Heartbeat) -> io::Result<()> {
    let raw = serde_json::to_vec(beat).map_err(io::Error::other)?;
    deck_core::paths::atomic_write(&heartbeat_path(app_dir), &raw)
}

/// Der Herzschlag DIESES Prozesses für jetzt.
pub fn own_beat(now: f64) -> Heartbeat {
    let exe = std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    Heartbeat { pid: std::process::id(), exe, ts: now, ended: None }
}

/// Das geordnete Ende vermerken (best effort; ein Fehler darf das Beenden nicht stören).
pub fn mark_ended(app_dir: &Path, reason: EndReason, now: f64) {
    let mut beat = read(app_dir).filter(|b| b.pid == std::process::id()).unwrap_or_else(|| own_beat(now));
    beat.ended = Some(reason.as_str().into());
    beat.ts = now;
    let _ = write(app_dir, &beat);
}

/// Für die Poll-Schleife der App: schreibt höchstens alle [`BEAT_EVERY_S`] Sekunden.
#[derive(Debug, Default)]
pub struct Beater {
    last: Option<f64>,
}

impl Beater {
    /// `true`, wenn geschrieben wurde. Fehler werden geschluckt (best effort).
    pub fn tick(&mut self, app_dir: &Path, now: f64) -> bool {
        if self.last.is_some_and(|t| now - t < BEAT_EVERY_S) {
            return false;
        }
        self.last = Some(now);
        write(app_dir, &own_beat(now)).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_beat_round_trips_and_names_its_own_process() {
        let d = tempfile::tempdir().unwrap();
        assert!(read(d.path()).is_none());
        let mut beater = Beater::default();
        assert!(beater.tick(d.path(), 1000.0));
        assert!(!beater.tick(d.path(), 1000.0 + BEAT_EVERY_S - 0.1), "gedrosselt");
        let b = read(d.path()).unwrap();
        assert_eq!((b.pid, b.ts, b.ended.as_deref()), (std::process::id(), 1000.0, None));
        assert!(b.exe.to_lowercase().ends_with(".exe"));
    }

    #[test]
    fn an_orderly_end_is_noted_in_the_beat() {
        let d = tempfile::tempdir().unwrap();
        Beater::default().tick(d.path(), 5.0);
        mark_ended(d.path(), EndReason::Exit, 6.0);
        let b = read(d.path()).unwrap();
        assert_eq!((b.ended.as_deref(), b.ts), (Some("exit"), 6.0));
        let raw = std::fs::read_to_string(heartbeat_path(d.path())).unwrap();
        assert!(raw.contains("\"pid\"") && raw.contains("\"ended\":\"exit\""), "{raw}");
    }
}
