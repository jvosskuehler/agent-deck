//! Lebt die Oberfläche des Decks noch? Die Rechnung hinter `ui_watch` und
//! `instance_guard` der App.
//!
//! Anlass (11.11.2026): das Deck lief weiter, Leiste und Griff waren aber weg. Jeder
//! Neustart gab sein Signal an genau diese Instanz weiter (Einzelinstanz-Sperre) und
//! endete, es passierte also nichts. Der Herzschlag (`heartbeat.json`) half nicht: ihn
//! schreibt der Poll-Thread, und der lief weiter. Was fehlte, war ein Beweis, dass der
//! Hauptthread Nachrichten abarbeitet UND die Webview des sichtbaren Fensters antwortet.
//!
//! Darum zwei Lebenszeichen, beide in `ui-alive.json` (App-Ordner, atomar):
//! - `main`: zuletzt lief eine Aufgabe auf dem Hauptthread (`run_on_main_thread`).
//! - `ui`: zuletzt antwortete die Seite des Fensters, das gerade zu sehen sein soll
//!   (Panel offen → Panel, sonst der Griff) auf `deck://ping` mit `ui_pong`. Ein
//!   verstecktes Fenster zählt nicht: dessen Seite antwortet auch dann, wenn die
//!   sichtbare tot ist.

use serde::{Deserialize, Serialize};

/// Datei mit den Lebenszeichen der Oberfläche.
pub const UI_ALIVE_FILE: &str = "ui-alive.json";
/// Takt der Prüfung (und des Pings) in Sekunden.
pub const CHECK_EVERY_S: f64 = 5.0;
/// So lange darf ein Lebenszeichen fehlen, bevor das Deck als hängend gilt.
pub const STUCK_AFTER_S: f64 = 60.0;
/// So lange nach dem Start wird nicht geurteilt (Webview lädt noch).
pub const GRACE_S: f64 = 90.0;
/// Ist die eigene Prüfschleife länger als das ausgefallen, schlief der Rechner: alle
/// Lebenszeichen gelten dann ab jetzt neu.
pub const SLEEP_GAP_S: f64 = 20.0;

/// Inhalt von `ui-alive.json`. Zeiten in Unix-Sekunden.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiAlive {
    pub pid: u32,
    /// Letzte Antwort der Seite des sichtbaren Fensters.
    pub ui: f64,
    /// Letzter Durchlauf auf dem Hauptthread.
    pub main: f64,
    /// Wann die Prüfschleife das zuletzt geschrieben hat.
    pub ts: f64,
}

/// Urteil über die eigene Oberfläche.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    Alive,
    /// Hängt: wie lange welches Lebenszeichen fehlt (Sekunden).
    Stuck { main_age: f64, ui_age: f64 },
}

impl Verdict {
    /// Für das Protokoll: was genau fehlt.
    pub fn describe(&self, window: &str) -> String {
        match self {
            Verdict::Alive => "lebt".into(),
            Verdict::Stuck { main_age, ui_age } => format!(
                "Hauptthread seit {main_age:.0} s still, Seite „{window}“ seit {ui_age:.0} s ohne Antwort{}",
                if *main_age > STUCK_AFTER_S { " (Hauptthread hängt)" } else { " (Webview antwortet nicht)" }
            ),
        }
    }
}

/// Urteil über die eigene Oberfläche: `started` = Start der App.
pub fn verdict(now: f64, started: f64, alive: &UiAlive) -> Verdict {
    let (main_age, ui_age) = (now - alive.main, now - alive.ui);
    if now - started < GRACE_S || (main_age <= STUCK_AFTER_S && ui_age <= STUCK_AFTER_S) {
        Verdict::Alive
    } else {
        Verdict::Stuck { main_age, ui_age }
    }
}

/// Was ein neuer Start mit einer schon laufenden Instanz tut.
#[derive(Debug, Clone, PartialEq)]
pub enum Predecessor {
    /// Sie lebt: das Signal geht an sie (Einzelinstanz), dieser Start endet.
    Healthy,
    /// Noch nicht zu entscheiden (ihre Lebenszeichen sind nur etwas alt): kurz warten.
    Wait,
    /// Sie hängt: beenden und selbst übernehmen. Der Text kommt ins Protokoll.
    Stuck(String),
}

/// Wie steht es um die laufende Instanz `pid`?
///
/// - `beat_ts`: Zeit ihres letzten Herzschlags (Poll-Thread).
/// - `age`: wie lange der Prozess schon läuft (Sekunden).
/// - `alive`: ihre `ui-alive.json`, falls vorhanden.
///
/// Eine Instanz ohne `ui-alive.json` (ältere Fassung) wird nur am Herzschlag gemessen:
/// lieber eine hängende stehen lassen als eine gesunde abschießen.
pub fn predecessor(now: f64, pid: u32, beat_ts: f64, age: f64, alive: Option<&UiAlive>) -> Predecessor {
    if age < GRACE_S {
        return Predecessor::Healthy;
    }
    let beat_age = now - beat_ts;
    if beat_age > STUCK_AFTER_S {
        return Predecessor::Stuck(format!("Herzschlag seit {beat_age:.0} s still"));
    }
    let Some(a) = alive.filter(|a| a.pid == pid) else { return Predecessor::Healthy };
    // Schreibt ihre Prüfschleife nicht mehr, sagen die Werte darin nichts mehr aus.
    let newest = a.ts.max(a.main).max(a.ui);
    if now - newest > STUCK_AFTER_S {
        return Predecessor::Stuck(format!("Prüfschleife seit {:.0} s still", now - newest));
    }
    match verdict(now, now - age, a) {
        Verdict::Alive if is_fresh(now, a) => Predecessor::Healthy,
        Verdict::Alive => Predecessor::Wait,
        stuck => Predecessor::Stuck(stuck.describe("sichtbar")),
    }
}

/// Frisch: beide Lebenszeichen höchstens drei Takte alt. Die Antwort auf einen Ping
/// landet erst im nächsten Durchlauf in der Datei, normal sind also 5 bis 10 s.
pub fn is_fresh(now: f64, alive: &UiAlive) -> bool {
    let limit = 3.0 * CHECK_EVERY_S;
    now - alive.ui <= limit && now - alive.main <= limit
}

/// Nach dem Warten (`Wait`) war die Instanz nicht frisch: was ins Protokoll kommt.
pub fn after_wait(now: f64, alive: &UiAlive) -> String {
    let (ui_age, main_age) = (now - alive.ui, now - alive.main);
    format!("antwortet nicht auf den neuen Start (Hauptthread {main_age:.0} s, sichtbare Seite {ui_age:.0} s still)")
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: f64 = 10_000.0;

    fn alive(ui_age: f64, main_age: f64) -> UiAlive {
        UiAlive { pid: 7, ui: NOW - ui_age, main: NOW - main_age, ts: NOW - 1.0 }
    }

    #[test]
    fn a_fresh_start_is_never_judged() {
        assert_eq!(verdict(NOW, NOW - 10.0, &alive(500.0, 500.0)), Verdict::Alive);
    }

    #[test]
    fn a_silent_visible_page_or_main_thread_is_stuck() {
        assert!(matches!(verdict(NOW, 0.0, &alive(STUCK_AFTER_S + 1.0, 1.0)), Verdict::Stuck { .. }));
        assert!(matches!(verdict(NOW, 0.0, &alive(1.0, STUCK_AFTER_S + 1.0)), Verdict::Stuck { .. }));
        assert_eq!(verdict(NOW, 0.0, &alive(STUCK_AFTER_S, STUCK_AFTER_S)), Verdict::Alive);
    }

    #[test]
    fn the_description_names_the_main_thread_when_it_hangs() {
        let v = verdict(NOW, 0.0, &alive(200.0, 200.0));
        assert!(v.describe("panel").contains("Hauptthread hängt"), "{}", v.describe("panel"));
        let v = verdict(NOW, 0.0, &alive(200.0, 1.0));
        assert!(v.describe("handle").contains("Webview"), "{}", v.describe("handle"));
    }

    #[test]
    fn a_predecessor_that_answers_keeps_the_deck() {
        assert_eq!(predecessor(NOW, 7, NOW - 2.0, 1000.0, Some(&alive(3.0, 3.0))), Predecessor::Healthy);
    }

    #[test]
    fn a_young_predecessor_is_left_alone() {
        assert_eq!(predecessor(NOW, 7, NOW - 500.0, 10.0, None), Predecessor::Healthy);
    }

    #[test]
    fn a_predecessor_without_beat_is_stuck() {
        assert!(matches!(predecessor(NOW, 7, NOW - 300.0, 1000.0, None), Predecessor::Stuck(_)));
    }

    #[test]
    fn an_old_version_without_ui_file_is_judged_by_the_beat_only() {
        assert_eq!(predecessor(NOW, 7, NOW - 2.0, 1000.0, None), Predecessor::Healthy);
        // Datei einer anderen Instanz zählt nicht.
        let other = UiAlive { pid: 8, ..alive(500.0, 500.0) };
        assert_eq!(predecessor(NOW, 7, NOW - 2.0, 1000.0, Some(&other)), Predecessor::Healthy);
    }

    #[test]
    fn a_predecessor_with_a_dead_page_is_stuck() {
        assert!(matches!(predecessor(NOW, 7, NOW - 2.0, 1000.0, Some(&alive(300.0, 2.0))), Predecessor::Stuck(_)));
    }

    #[test]
    fn a_predecessor_whose_check_loop_stopped_is_stuck() {
        let a = UiAlive { pid: 7, ui: NOW - 200.0, main: NOW - 200.0, ts: NOW - 200.0 };
        assert!(matches!(predecessor(NOW, 7, NOW - 2.0, 1000.0, Some(&a)), Predecessor::Stuck(_)));
    }

    #[test]
    fn a_slightly_late_predecessor_gets_a_moment() {
        assert_eq!(predecessor(NOW, 7, NOW - 2.0, 1000.0, Some(&alive(30.0, 2.0))), Predecessor::Wait);
        assert!(!is_fresh(NOW, &alive(30.0, 2.0)));
        // Normales Antwortalter (Antwort steht erst im nächsten Durchlauf in der Datei).
        assert!(is_fresh(NOW, &alive(10.0, 10.0)));
        assert_eq!(predecessor(NOW, 7, NOW - 2.0, 1000.0, Some(&alive(10.0, 10.0))), Predecessor::Healthy);
    }
}
