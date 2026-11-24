//! Reserve-Agent: ein fertig gestarteter Claude-Agent, der versteckt wartet, bis jemand
//! einen neuen haben will. Der Start eines Agenten kostet 3 bis 5 s (Terminal, Shell,
//! Claude Code lädt Einstellungen, Plugins, Befehle); mit Reserve legt das Deck nur noch
//! ein fertiges Fenster an seinen Platz.
//!
//! Hier steht nur die Rechnung: wann eine Reserve angelegt, wann sie ausgetauscht wird und
//! wann sie zu einer Anfrage passt. Fenster und Prozesse fasst `app/src-tauri/src/wt_reserve.rs`
//! an. Ob es überhaupt eine Reserve gibt, entscheidet eine Erweiterung
//! (`DeckExtension::reserve_agent`); die Team-Ausgabe hat keine und legt nie eine an.
//!
//! Warum nur bei Ruhe: Windows Terminal holt jedes neue Fenster nach vorn, auch wenn es
//! außerhalb des Bildschirms aufgehen soll, und kennt keinen Start ohne Fenster (geprüft
//! am 20.11.2026 mit WT 1.25 in einer Test-VM). Das Deck legt die Reserve darum unter den
//! Vorhang, versteckt sie und gibt den Fokus zurück. Wer in diesem Moment tippt, tippt in
//! die Reserve. Bis 24.11.2026 reichten 5 s Tipp-Pause, und genau das passierte: wer kurz
//! nachdachte und weiterschrieb, landete im versteckten Agenten. Darum jetzt drei Riegel:
//! eine Minute ohne Eingabe ([`IDLE_BEFORE_SPAWN_S`]), kein Terminal vorn ([`quiet`]: wer
//! einem Agenten beim Arbeiten zusieht, schreibt gleich wieder), und kam während des
//! Anlegens doch eine Eingabe, wird die Reserve verworfen ([`typed_while_spawning`]), damit
//! halb Getipptes nie in einem übernommenen Agenten steht.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// So lange muss niemand Tastatur oder Maus angefasst haben, bevor das Deck eine Reserve
/// öffnet oder austauscht (s). Wer eine Minute nichts getan hat, ist meist weg oder liest;
/// die Reserve kommt dann immer noch lange vor dem nächsten neuen Agenten.
pub const IDLE_BEFORE_SPAWN_S: f64 = 60.0;
/// Danach wird eine unbenutzte Reserve ausgetauscht (s): Claude Code aktualisiert sich
/// selbst, und der Abgleich beim Start soll nicht Stunden alt sein.
pub const MAX_AGE_S: f64 = 3600.0;
/// So lange darf eine angelegte Reserve brauchen, bis ihre Shell lebt (s).
pub const SPAWN_TIMEOUT_S: f64 = 30.0;
/// Nach einem Fehlschlag (kein Fenster, Shell tot) so lange keinen neuen Versuch (s).
pub const FAIL_COOLDOWN_S: f64 = 600.0;
/// Datei neben `wt_hosts.json`: welcher Slot gerade Reserve ist. Überlebt einen Neustart
/// des Decks (Installation), sonst stünde die versteckte Reserve danach als gewöhnlicher
/// Agent ohne Fenster im Deck.
pub const FILE: &str = "wt_reserve.json";

/// Fensterklassen, bei denen vorn jemand in eine Konsole schreibt: Windows Terminal (auch
/// jeder Agent), die alte Konsole, Git Bash (mintty), ConEmu. Ist eine davon vorn, legt das
/// Deck keine Reserve an, auch nach einer Minute Ruhe nicht.
pub const TERMINAL_CLASSES: &[&str] =
    &[crate::wt::WINDOW_CLASS, "ConsoleWindowClass", "PseudoConsoleWindow", "mintty", "VirtualConsoleClass"];

/// Was ein Agent zum Start braucht. Eine Reserve passt nur zu einer Anfrage mit genau
/// demselben Rezept: anderer Block, Ordner, Modell, Effort, Farbschema oder eine andere
/// Umgebung heißt kalt starten wie bisher.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recipe {
    /// Buchstabe des Blocks (`B`).
    pub win: String,
    pub folder: String,
    /// Argumente für `claude`, wie sie ins Startskript gehen.
    pub argv: Vec<String>,
    /// Farbschema des Fensters (`--colorScheme`), `None` = nach Thema.
    pub scheme: Option<String>,
    /// WSL-Distro, in der der Agent läuft; `None` = Windows. Fehlt in Dateien vom
    /// 20.11.2026 (damals nur Windows), darum mit Vorgabe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wsl: Option<String>,
}

/// Eine angelegte Reserve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reserve {
    /// Slot-Name, unter dem sie läuft (`B69`). Bleibt beim Übernehmen derselbe: Hooks und
    /// Fenstertitel kennen nur ihn.
    pub slot: String,
    pub recipe: Recipe,
    /// Wann angelegt (Unix-Sekunden).
    pub since: f64,
    /// Fenster gefunden und versteckt: erst dann taugt sie zum Übernehmen.
    #[serde(default)]
    pub ready: bool,
}

/// Was in diesem Takt mit der Reserve geschieht.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Nichts tun.
    Keep,
    /// Eine neue anlegen.
    Spawn,
    /// Die vorhandene beenden; mit Grund fürs Log.
    Retire(&'static str),
}

/// Lage, nach der [`decide`] entscheidet.
#[derive(Debug, Clone)]
pub struct Situation<'a> {
    pub now: f64,
    /// Sekunden seit der letzten Eingabe.
    pub idle_s: f64,
    /// Fensterklasse des Vordergrundfensters, falls eins.
    pub front_class: Option<&'a str>,
    /// Die gewünschte Reserve; `None` = keine (abgeschaltet, kein Block).
    pub want: Option<&'a Recipe>,
    pub current: Option<&'a Reserve>,
    /// Lebt die Shell der vorhandenen Reserve?
    pub alive: bool,
    /// Vor diesem Zeitpunkt kein neuer Versuch (nach Fehlschlag).
    pub retry_after: f64,
}

/// Darf das Deck jetzt ein Fenster öffnen, das kurz den Fokus nimmt? Nur nach
/// [`IDLE_BEFORE_SPAWN_S`] ohne Eingabe und ohne Terminal vorn ([`TERMINAL_CLASSES`]).
/// Gefragt wird zweimal: im Takt und noch einmal direkt vor dem Start, falls der
/// Arbeiter-Thread erst später dran ist.
pub fn quiet(idle_s: f64, front_class: Option<&str>) -> bool {
    idle_s >= IDLE_BEFORE_SPAWN_S && !front_class.is_some_and(|c| TERMINAL_CLASSES.contains(&c))
}

/// Kam seit dem Start der Reserve (`spawn_s` Sekunden her) eine Eingabe? Dann hatte das
/// neue Fenster womöglich die Tastatur, und was getippt wurde, steht in der Reserve.
pub fn typed_while_spawning(idle_s: f64, spawn_s: f64) -> bool {
    idle_s < spawn_s
}

/// Ein Takt der Reserve-Pflege.
///
/// Beenden geht jederzeit (das Fenster ist versteckt, niemand sieht es). Anlegen nur bei
/// Ruhe ([`quiet`]) und nicht in der Pause nach einem Fehlschlag. Eine noch nicht fertige
/// Reserve bekommt [`SPAWN_TIMEOUT_S`], um zu leben.
pub fn decide(s: &Situation) -> Step {
    let calm = quiet(s.idle_s, s.front_class);
    if let Some(cur) = s.current {
        let age = s.now - cur.since;
        if s.want != Some(&cur.recipe) {
            return Step::Retire(if s.want.is_none() { "abgeschaltet" } else { "Einstellungen geändert" });
        }
        if !s.alive {
            // Eine frische hat ihre PID-Datei womöglich noch nicht geschrieben, auch wenn
            // ihr Fenster schon steht (PowerShell kalt: 4 s, gemessen 20.11.2026). Erst
            // nach der Frist gilt sie als tot.
            return if age > SPAWN_TIMEOUT_S { Step::Retire("Shell lebt nicht") } else { Step::Keep };
        }
        // Austauschen kostet ein Anlegen, das nur bei Ruhe geht: also auch das Beenden
        // erst dann, sonst stünde man eine Weile ohne Reserve da.
        if age > MAX_AGE_S && cur.ready && calm {
            return Step::Retire("zu alt");
        }
        return Step::Keep;
    }
    if s.want.is_some() && s.now >= s.retry_after && calm {
        return Step::Spawn;
    }
    Step::Keep
}

/// Passt die Reserve zu dieser Anfrage?
pub fn fits(reserve: Option<&Reserve>, ask: &Recipe) -> bool {
    reserve.is_some_and(|r| r.ready && r.recipe == *ask)
}

/// Gemerkte Reserve aus [`FILE`] lesen; Unlesbares = keine.
pub fn load(v: Option<Value>) -> Option<Reserve> {
    v.and_then(|v| serde_json::from_value(v).ok())
}

#[cfg(test)]
#[path = "reserve_tests.rs"]
mod tests;
