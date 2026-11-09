//! „Läuft noch ohne Fenster“: Prozesse, die aus dem Fenster eines Agenten gestartet
//! wurden (Dev-Server, `dotnet run`, `vitest --watch`, Hintergrundjobs aus Claudes
//! Bash-Werkzeug) und weiterlaufen, nachdem Fenster und Agent zu sind.
//!
//! Hier steht nur die Rechnung, ohne Win32: der Merker ([`tracker`]), was nie angeboten
//! wird ([`rules`]) und die Zeilen samt dem, was „Beenden“ trifft ([`rows`]). Den
//! Schnappschuss, das Warten und das Beenden macht `deck_services::orphans`.

pub mod model;
pub mod rows;
pub mod rules;
pub mod tracker;

pub use model::{Key, ORPHANS_FILE, ProcDetails, ProcRow, Saved, ShellId, ShellRecord, Tracked, parse_row_id, row_id};
pub use rows::{KillState, kill_order, orphan_rows, roots};
pub use rules::{Protected, protected};
pub use tracker::Tracker;
