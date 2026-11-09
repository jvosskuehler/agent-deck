//! Was die Oberfläche zu sehen bekommt: das Anzeigemodell des Decks.
//!
//! Diese Typen sind der Vertrag zwischen Rust und der React-Oberfläche. Die
//! TypeScript-Seite wird daraus erzeugt (`ts-rs`, nach `app/src/bindings/`), es gibt
//! sie also nur einmal. Wer hier ein Feld ändert, erzeugt die Bindings neu:
//!
//! ```text
//! cargo test -p deck-core export_bindings
//! ```
//!
//! Das Modell ist bewusst *fertig gerechnet*: Farben, Texte und Reihenfolge stehen
//! hier schon so, wie sie gezeichnet werden. Die Oberfläche rechnet nichts nach, sie
//! zeichnet. Rechnen gehört in `deck-core` und wird dort getestet.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Zustand eines Agenten, wie ihn die Kachel zeigt.
///
/// `Lost` meldet kein Hook: das Panel errechnet ihn, wenn das Fenster des Agenten
/// nicht mehr verbunden ist. `Unknown` steht bei Remote-Fenstern, deren Hooks auf
/// der Gegenstelle schreiben (dort kommt hier nie eine Meldung an).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Status {
    Idle,
    Thinking,
    Waiting,
    Done,
    Lost,
    Unknown,
    None,
}

/// Wo ein Repo-Block lebt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Host {
    /// Windows Terminal: ein Fenster je Agent, hohe Kachel mit Sessiontitel.
    Terminal,
    /// VS Code: Terminals in einem Editorfenster, flache Kachel.
    Vscode,
}

/// Eine Agentenkachel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TileView {
    /// Slot-Name, z. B. `B3`: Buchstabe des Blocks plus laufende Nummer.
    pub slot: String,
    pub status: Status,
    /// Angezeigter Statustext in der aktuellen Sprache („Rückfrage“, „denkt“ …).
    pub status_label: String,
    /// Kurzname des Modells aus der statusLine, z. B. „Opus 5.5“. Leer = noch unbekannt.
    pub model: String,
    /// Effort, wie er auf der Karte steht („xhigh“, „ultracode“). Leer = unbekannt.
    pub effort: String,
    /// Permission-Modus in Deck-Namen („auto“, „plan“, „bypass“ …). Leer = unbekannt.
    pub mode: String,
    /// Sessiontitel (`session_name` der statusLine). Nur Terminal-Kacheln zeigen ihn.
    pub title: Option<String>,
    /// Ticket- und/oder PR-Bezug für die Karte, schon gekürzt.
    pub reference: Option<String>,
    /// `true`: Bezug nur aus dem Chat erkannt, nicht zugewiesen (gedimmt zeichnen).
    pub reference_detected: bool,
    /// Die Kachel ist ausgewählt (zuletzt angeklickt bzw. fokussiert): weißer Ring.
    pub selected: bool,
    /// Zeitpunkt (Unix-Sekunden) des letzten Statuswechsels; die Oberfläche lässt die
    /// Kachel kurz aufblühen, wenn er sich ändert.
    pub status_changed_at: f64,
}

/// Ein Repo-Block: Kopfzeile plus Kachelreihe(n).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BlockView {
    /// Fenster-Buchstabe (A … O).
    pub letter: String,
    /// Angezeigter Name: Repo bzw. Workspace des Fensters.
    pub name: String,
    pub host: Host,
    /// Heimat-Block (Windows Terminal in `Repos`): steht immer oben.
    pub home: bool,
    /// Fenster verbunden? Ohne Verbindung sind alle Kacheln „getrennt“.
    pub connected: bool,
    /// Remote-Fenster (SSH, WSL, Container): Zustand von hier aus nicht sichtbar.
    pub remote: bool,
    pub tiles: Vec<TileView>,
    /// Reihenlängen, wenn die Karten so liegen sollen wie die Fenster auf dem Bildschirm
    /// (Windows Terminal): `[3, 3]` = zwei Reihen zu drei. Karten jenseits der Summe
    /// (minimierte Fenster) folgen danach im normalen Umbruch. Leer = normaler Umbruch.
    pub rows: Vec<u32>,
    /// So viele Karten passen in eine Reihe, wie Fenster auf dem Monitor des Blocks
    /// nebeneinander passen (Windows Terminal). Bis dahin steht die ＋-Karte neben der
    /// letzten Karte, danach am Anfang der nächsten Reihe. 0 = unbekannt.
    pub screen_cols: u32,
    /// Der Block wartet darauf, dass ein VS-Code-Fenster angeklickt wird („Mit Fenster
    /// verbinden“ im Kontextmenü des Kopfes, `connect.py: start_bind`).
    pub binding: bool,
}

/// Ein Nutzungslimit (Session, Woche …) für die untere Leiste.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LimitView {
    /// Anzeigename in der aktuellen Sprache („Session“, „Woche“).
    pub label: String,
    /// Auslastung in Prozent; `None` = unbekannt oder hinter dem Reset (graues „—“).
    pub percent: Option<f64>,
    /// Ampelfarbe als CSS-Farbe, unverdunkelt (ist `UsageView::stale`, matt zeichnen).
    pub color: String,
    /// Restzeit bis zum Reset wie in der Python-Fassung („2 Std. 30 Min.“, „3 Tg.“,
    /// „jetzt“), leer wenn unbekannt.
    pub resets: String,
}

/// Nutzungsanzeige unten links.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UsageView {
    /// Die zwei Hauptlimits, die immer in der Leiste stehen.
    pub primary: Vec<LimitView>,
    /// Weitere Limits, nur im Tooltip.
    pub extra: Vec<LimitView>,
    /// Wert ist alt: matt zeichnen, Tooltip nennt den Stand.
    pub stale: bool,
    /// Konto, zu dem die Zahlen gehören (E-Mail, klein); `None` = unbekannt. Seit es
    /// zwei Konten gibt (2026-11-08), die erste Frage im Tooltip: wessen Zahl ist das?
    pub account: Option<String>,
    /// Letzter Fehler beim Abruf, für den Tooltip.
    pub error: Option<String>,
    /// Stand für den Tooltip: „Stand vor 1 Std.“ (Daten älter als 15 Min.), „letzter
    /// Wert“ (Fehler ohne bekannte Datenzeit), sonst leer.
    pub as_of: String,
}

/// Der komplette Bildschirminhalt des Panels. Rust schickt ihn als Ereignis
/// `deck://view`, sobald sich etwas ändert.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DeckView {
    /// Blöcke in Anzeigereihenfolge (Heimat-Block zuerst).
    pub blocks: Vec<BlockView>,
    pub usage: Option<UsageView>,
    /// Farbe des dringlichsten Zustands (für den Griff); `None` = alles ruhig.
    pub urgent: Option<Status>,
    /// Oberflächensprache, „de“ oder „en“.
    pub lang: String,
    /// „Läuft noch ohne Fenster“: Prozesse aus geschlossenen Agentenfenstern, fertig
    /// sortiert. Leer = der Hinweis in der Leiste fehlt ganz.
    #[serde(default)]
    pub orphans: Vec<OrphanView>,
}

/// Eine Zeile in „Läuft noch ohne Fenster“: ein Prozessbaum aus einem geschlossenen
/// Agentenfenster (`deck_core::orphans`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OrphanView {
    /// Kennung für `kill_orphan` (`pid-startzeit`, als Text: die Startzeit ist größer,
    /// als eine JavaScript-Zahl genau fasst).
    pub id: String,
    pub pid: u32,
    /// Lauschende TCP-Ports des ganzen Baums, aufsteigend.
    pub ports: Vec<u16>,
    /// `:3000`, `:3000 :3001`, `:3000 +2` oder „kein Port“.
    pub port_text: String,
    /// Programm ohne `.exe` (`node`, `dotnet`).
    pub program: String,
    /// Kurze Kommandozeile (`pnpm nx serve management-ui`).
    pub command: String,
    /// Volle Kommandozeile für den Hover.
    pub command_full: String,
    /// Herkunft: „A2 · zu seit 14 min“.
    pub origin: String,
    /// Wird gerade beendet.
    pub busy: bool,
    /// Beenden ist gescheitert: warum.
    pub error: Option<String>,
}

/// Inhalt des Hover-Tooltips einer Kachel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TooltipView {
    pub slot: String,
    /// Ticket/PR-Zeile über der Zusammenfassung (erkannt oder zugewiesen).
    pub references: Option<String>,
    /// KI-Kurzzusammenfassung; `None`, solange sie noch berechnet wird.
    pub summary: Option<String>,
    /// Rückfall, solange keine Zusammenfassung da ist: die letzte Frage.
    pub last_prompt: Option<String>,
    /// Herkunft als Kopfzeilen (`hover.py: _origin_lines`): „Repo · Fenster A · A2“,
    /// sitzt der Agent in einem worktree, darunter „↳ wt/<Ordner>“.
    pub origin: Vec<String>,
}

/// Andock-Zustand des Panels. Rust entscheidet (Zeigerposition, Kante), die
/// Oberfläche animiert nur: Ereignis `deck://dock`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DockView {
    /// `top`, `left`, `right`, `bottom` oder `off` (frei schwebend, nie eingeklappt).
    pub edge: String,
    /// `open` (ausgeklappt) oder `closed` (nur der Griff ist zu sehen).
    pub phase: String,
    /// Wirksamer Zoom des Panels (1 = natürliche Größe): der vom Nutzer gezogene Zoom,
    /// verkleinert, falls das Panel sonst nicht auf den Monitor passte (`layout.py`).
    /// Rust setzt ihn als Zoom der Webview; die Oberfläche braucht ihn nur zum Rechnen
    /// in Bildschirmpixeln.
    pub zoom: f64,
}

/// Werte des Einstellungsfensters. Was hier steht, gilt sofort beim Ändern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SettingsView {
    /// Modell-Wert für neue Agenten (`opus[1m]`, `fable` …).
    pub model: String,
    /// `permissions.defaultMode` (`auto`, `default`, `acceptEdits`, `plan`, `bypassPermissions`).
    pub mode: String,
    /// Effort-Auswahl als Label (`Ultracode`, `max`, `xhigh` …).
    pub effort: String,
    /// `german` oder `english`: Antwortsprache der Agenten UND Sprache des Decks.
    pub language: String,
    /// Rand, an dem das Deck andockt: `top`, `left`, `right`, `bottom` oder `off`.
    pub dock_edge: String,
    /// Automatisch `/compact`, kurz bevor der Prompt-Cache eines wartenden Agenten
    /// abläuft (`deck_settings.json` `auto_compact`, Standard an).
    pub auto_compact: bool,
}

/// Inhalt des Dialogs „Ticket zuweisen“ (Kontextmenü einer Kachel). Ein Feld, die
/// Ticketnummer; vorbelegt mit dem schon zugewiesenen Ticket.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TicketView {
    pub slot: String,
    /// Schon zugewiesenes Ticket, sonst leer.
    pub current: String,
    /// Oberflächensprache, „de“ oder „en“.
    pub lang: String,
}
