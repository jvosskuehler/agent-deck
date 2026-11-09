//! Die Herkunftsmarke: eine Umgebungsvariable, die jeder Nachkomme einer Agenten-Shell erbt.
//!
//! Die Eltern-Kette reißt, wenn ein Zwischenglied endet, bevor ein Schnappschuss es sah.
//! Genau das ist der Normalfall bei Claudes Werkzeugen: jeder Befehl läuft in einer
//! frischen Shell (`Start-Process npm run dev`, `pnpm dev &`), die nach Millisekunden
//! wieder weg ist. Die Umgebung dagegen erbt jeder Prozess von dem, der ihn startet, egal
//! wer danach stirbt. `wt_agent.ps1` setzt darum vor claude
//! `AGENT_DECK_SHELL=<pid>.<created>` (dieselben Werte wie in der `.wtpid`), und das Deck
//! liest die Variable aus der Umgebung neuer Prozesse, die die Eltern-Kette nicht zuordnet.
//!
//! Warum kein Job-Objekt: auf dem Rechner, für den das Deck gebaut ist, steckt schon jeder
//! Prozess in einem Job, und Kinder erben einen darunter angelegten Job nicht (gemessen
//! 09.11.2026: CreateProcess ohne Flags, `Start-Process`, `Process.Start`, auch mit einem
//! Job ohne Breakaway-Erlaubnis und aus einem über den Explorer gestarteten Prozess).
//!
//! Wer die Umgebung für ein Kind bewusst leert, fällt heraus; dann bleibt die Eltern-Kette.

use crate::wt::same_start;

/// Name der Variable.
pub const MARKER_VAR: &str = "AGENT_DECK_SHELL";

/// Wert der Variable für eine Shell (`created` wie in der `.wtpid`).
pub fn marker_value(pid: u32, created: i64) -> String {
    format!("{pid}.{created}")
}

/// Umkehrung von [`marker_value`].
pub fn parse_marker(value: &str) -> Option<(u32, i64)> {
    let (pid, created) = value.trim().split_once('.')?;
    let pid: u32 = pid.parse().ok()?;
    (pid > 0).then_some(())?;
    Some((pid, created.parse().ok()?))
}

/// Die Marke aus einem Umgebungsblock (`NAME=wert`, durch `\0` getrennt, wie Windows ihn
/// im Prozess hält). Der Name zählt ohne Rücksicht auf Groß- und Kleinschreibung, wie
/// Windows selbst Variablen vergleicht.
pub fn marker_in_env(block: &str) -> Option<(u32, i64)> {
    block.split('\0').find_map(|entry| {
        let (name, value) = entry.split_once('=')?;
        name.eq_ignore_ascii_case(MARKER_VAR).then(|| parse_marker(value))?
    })
}

/// Passt die Marke zu dieser Shell? `start` ist die genaue Startzeit aus dem Schnappschuss,
/// `created` aus PowerShell nur auf etwa eine Sekunde genau (siehe [`same_start`]).
pub fn marks_shell(marker: (u32, i64), pid: u32, start: u64) -> bool {
    marker.0 == pid && same_start(start as i64, marker.1)
}
