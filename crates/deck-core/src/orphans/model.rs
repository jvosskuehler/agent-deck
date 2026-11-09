//! Die Daten des Merkers: was ein Schnappschuss liefert, was über Agenten-Shells und ihre
//! Nachkommen gemerkt wird und wie das auf der Platte steht (`orphans.json` im
//! App-Ordner, damit ein Neustart des Decks nichts vergisst).

use serde::{Deserialize, Serialize};

/// Datei des Merkers im App-Ordner.
pub const ORPHANS_FILE: &str = "orphans.json";

/// Ein Prozess, wie ihn der Toolhelp-Schnappschuss liefert. Ohne Startzeit (kein Zugriff)
/// taugt er nicht als Beweis und wird übergangen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcRow {
    pub pid: u32,
    pub ppid: u32,
    /// Exe-Name ohne Pfad (`node.exe`).
    pub exe: String,
    /// Startzeit als FILETIME (100-ns-Schritte seit 1601).
    pub start: Option<u64>,
}

/// Was nur für gemerkte Prozesse nachgefragt wird (je Prozess ein Handle).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProcDetails {
    pub cmdline: Option<String>,
    /// Voller Exe-Pfad.
    pub image: Option<String>,
    /// Windows-Sitzung, 0 = Dienste.
    pub session: Option<u32>,
}

/// Eine Agenten-Shell, wie ihre `.wtpid`-Datei sie nennt (`created` ist die Startzeit, die
/// PowerShell über DateTime umgerechnet hat, also nur auf eine Sekunde genau).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellId {
    pub slot: String,
    pub pid: u32,
    pub created: i64,
}

/// PID samt genauer Startzeit: erst beides zusammen bezeichnet einen Prozess.
pub type Key = (u32, u64);

/// Eine gemerkte Agenten-Shell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShellRecord {
    pub slot: String,
    pub pid: u32,
    /// Genaue Startzeit aus dem Schnappschuss (FILETIME).
    pub start: u64,
    /// Zuletzt lebend gesehen (Unix-Sekunden).
    pub last_seen: f64,
    /// Seit wann das Fenster zu ist (Unix-Sekunden); `None` = lebt.
    #[serde(default)]
    pub closed_at: Option<f64>,
}

impl ShellRecord {
    pub fn key(&self) -> Key {
        (self.pid, self.start)
    }
}

/// Ein gemerkter Nachkomme einer Agenten-Shell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tracked {
    pub pid: u32,
    pub start: u64,
    pub ppid: u32,
    pub exe: String,
    #[serde(default)]
    pub cmdline: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub session: Option<u32>,
    /// Die Shell, unter der er gesehen wurde.
    pub shell_pid: u32,
    pub shell_start: u64,
    /// Zuerst gesehen (Unix-Sekunden).
    pub first_seen: f64,
    /// Zuerst vermisst (Unix-Sekunden); `None` = lebte beim letzten Blick.
    #[serde(default)]
    pub gone_at: Option<f64>,
}

impl Tracked {
    pub fn key(&self) -> Key {
        (self.pid, self.start)
    }

    pub fn shell_key(&self) -> Key {
        (self.shell_pid, self.shell_start)
    }

    pub fn alive(&self) -> bool {
        self.gone_at.is_none()
    }
}

/// Der Merker, wie er in `orphans.json` steht.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Saved {
    #[serde(default)]
    pub saved_at: f64,
    #[serde(default)]
    pub shells: Vec<ShellRecord>,
    #[serde(default)]
    pub procs: Vec<Tracked>,
}

impl Saved {
    /// Tolerant lesen: kaputt oder fremd = leer (der Merker beginnt dann von vorn).
    pub fn parse(bytes: &[u8]) -> Saved {
        serde_json::from_slice(bytes).unwrap_or_default()
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }
}

/// Abstand von 1601 zu 1970 in FILETIME-Schritten.
const EPOCH_DIFF: u64 = 116_444_736_000_000_000;

/// FILETIME → Unix-Sekunden.
pub fn filetime_to_unix(ft: u64) -> f64 {
    (ft as f64 - EPOCH_DIFF as f64) / 10_000_000.0
}

/// Unix-Sekunden → FILETIME.
pub fn unix_to_filetime(unix: f64) -> u64 {
    (unix.max(0.0) * 10_000_000.0) as u64 + EPOCH_DIFF
}

/// Kennung einer Zeile für die Oberfläche: `pid-start`. Als Text, weil eine FILETIME
/// größer ist, als eine JavaScript-Zahl genau fasst.
pub fn row_id(key: Key) -> String {
    format!("{}-{}", key.0, key.1)
}

/// Umkehrung von [`row_id`].
pub fn parse_row_id(id: &str) -> Option<Key> {
    let (pid, start) = id.split_once('-')?;
    Some((pid.parse().ok()?, start.parse().ok()?))
}
