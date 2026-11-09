//! Bilanz einer Prüfung oder Einrichtung: Zeilen mit Grad, dazu die Zählung.
//!
//! Die Python-Fassung schrieb ihre Urteile als Text und hängte für `install.ps1` die
//! Zeile `## fails=N warns=N` an; die Zählung musste auf JEDEM Rückgabeweg stehen, sonst
//! meldete der Lauf null Probleme. Hier ist sie eine Struktur, die App zeigt sie an.

use serde::Serialize;

/// Wie schwer eine Zeile wiegt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Grade {
    /// Geprüft und in Ordnung.
    Ok,
    /// Getan (eine Änderung, ein Hinweis).
    Info,
    /// Bewusst ausgelassen oder auffällig, aber nicht kaputt.
    Warn,
    /// Kaputt: so lädt bzw. meldet nichts.
    Fail,
}

impl Grade {
    /// Aus den Gradnamen von `deck-core` (`ok`, `info`, `warn`, `fail`).
    pub fn parse(s: &str) -> Grade {
        match s {
            "ok" => Grade::Ok,
            "warn" => Grade::Warn,
            "fail" => Grade::Fail,
            _ => Grade::Info,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Line {
    pub grade: Grade,
    pub text: String,
}

/// Die Bilanz.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Report {
    pub lines: Vec<Line>,
    /// Wurde eine Datei geschrieben?
    pub changed: bool,
    /// Sicherung der Datei vor dem Schreiben, falls angelegt.
    pub backup: Option<String>,
}

impl Report {
    pub fn push(&mut self, grade: Grade, text: impl Into<String>) {
        self.lines.push(Line { grade, text: text.into() });
    }

    /// Zeilen aus `deck-core` (`(grad, text)`) übernehmen.
    pub fn extend_notes<S: AsRef<str>>(&mut self, notes: &[(&str, S)]) {
        for (g, t) in notes {
            self.push(Grade::parse(g), t.as_ref());
        }
    }

    pub fn fails(&self) -> usize {
        self.lines.iter().filter(|l| l.grade == Grade::Fail).count()
    }

    pub fn warns(&self) -> usize {
        self.lines.iter().filter(|l| l.grade == Grade::Warn).count()
    }

    /// Ohne `fail`?
    pub fn is_ok(&self) -> bool {
        self.fails() == 0
    }

    /// Zwei Bilanzen hintereinander (Dateien, dann Registratur).
    pub fn append(&mut self, other: Report) {
        self.lines.extend(other.lines);
        self.changed |= other.changed;
        if self.backup.is_none() {
            self.backup = other.backup;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_count_follows_the_lines() {
        let mut r = Report::default();
        r.extend_notes(&[("ok", "a"), ("warn", "b"), ("fail", "c"), ("info", "d"), ("fail", "e")]);
        assert_eq!((r.fails(), r.warns(), r.is_ok()), (2, 1, false));
    }
}
