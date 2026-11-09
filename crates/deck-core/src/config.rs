//! Die Einstellknöpfe des Decks mit den heutigen Werten der Python-Fassung
//! (`deck/domain/config.py`). Lange Texte (Ticket-Prompts) stehen in `prompts.rs`.
//!
//! Die Werte werden direkt gelesen (`config::POLL_MS`), nicht mit Ersatzwert kopiert:
//! ein Ersatzwert verbirgt einen Tippfehler im Namen und lässt das Deck still mit einer
//! anderen Zahl laufen als hier steht.

use std::path::PathBuf;

/// Branch-Name eines Ticket-worktree = dieses Präfix + kleingeschriebener Ticket-Slug.
pub const TICKET_BRANCH_PREFIX: &str = "ticket/";
/// Jira-Projekt, das einer reinen Nummer vorangestellt wird („2701“ → „PROJ-2701“).
/// Leer: Nummern bleiben roh. Überschreibbar per deck_settings.json (`jira_prefix`).
pub const JIRA_PROJECT_KEY: &str = "PROJ";

/// Permission-Modi in der Shift+Tab-Reihenfolge von Claude Code (Deck-Namen). Claude
/// Code hat keine Slash-Kommandos für die Modi, das Deck zählt also Shift+Tab. Bypass
/// fügt `status_model::mode_cycle` selbst hinter `plan` ein; hier nicht eintragen.
pub const MODE_CYCLE: [&str; 4] = ["manual", "accept", "plan", "auto"];
/// Modus, in dem ein frischer Chat startet.
pub const MODE_START: &str = "manual";
/// Startmodus für NEU per ＋ angelegte Agenten, einmalig nach dem ersten Hook-Event
/// angefahren. Leer schaltet die Automatik ab.
pub const NEW_AGENT_MODE: &str = "auto";
/// Sekunden nach dem ersten Hook warten, bevor getippt wird: der SessionStart-Hook
/// feuert, bevor die TUI Shift+Tab sicher annimmt.
pub const AUTO_READY_GRACE: f64 = 1.5;
/// So oft nachtreiben, wenn ein Hook zeigt, dass der Modus kurz vor dem Ziel hängt.
pub const AUTO_MAX_TRIES: u32 = 3;
/// Sekunden, die auf den ersten Hook eines neuen Agenten gewartet wird.
pub const PENDING_AUTO_TTL: f64 = 300.0;

/// Abfrage-Takt der Nutzung in Sekunden (Untergrenze 30); schont das enge Rate-Limit.
pub const USAGE_POLL_SECONDS: u32 = 120;

/// KI-Kurzzusammenfassung im Hover statt der letzten Frage.
pub const HOVER_SUMMARY: bool = true;
/// Modell der Zusammenfassung (günstig und schnell genügt).
pub const HOVER_SUMMARY_MODEL: &str = "haiku";
/// Zusammenfassungen offener Agenten vorab erzeugen, damit der Hover sofort steht.
pub const HOVER_SUMMARY_PREFETCH: bool = true;

/// Ticket- und PR-Nummer per Regex aus dem Transcript lesen (kein Modell, keine Kosten).
pub const TICKET_AUTODETECT: bool = true;
/// Das Erkannte auch auf der Karte zeigen (gedimmt), solange kein zugewiesenes Ticket
/// mit worktree die Zeile belegt.
pub const TICKET_AUTODETECT_ON_CARD: bool = true;

/// Titelstück, an dem VS-Code-Fenster erkannt werden.
pub const VSCODE_MARKER: &str = "Visual Studio Code";

/// Broker der VS-Code-Extensions (muss zu agentDeck.host/port der Extension passen).
pub const BROKER_HOST: &str = "127.0.0.1";
pub const BROKER_PORT: u16 = 8765;

/// Heimat-Block: Windows-Terminal-Block über dem Ordner aller Repos, steht immer oben.
/// Überschreibbar per deck_settings.json (`terminal_home`); ein fehlender Ordner
/// schaltet ihn ab.
pub fn terminal_home() -> PathBuf {
    crate::paths::home_dir().join("Repos")
}

/// Sprung-Taste zum am längsten Wartenden (`attention.rs`). Strg+< liegt links
/// einhändig und nimmt keinem Programm ein Kürzel weg. Leer schaltet sie ab.
pub const JUMP_HOTKEY: &str = "ctrl+<";

/// Alle nach vorn (`front.rs`): Strg+Shift+< liegt auf denselben Tasten wie die
/// Sprung-Taste (ohne Shift einer, mit Shift alle), F20 ist der gedrückte Drehregler am
/// Makropad. Nicht Strg+Alt+<: das ist auf der deutschen Tastatur AltGr+< und tippt den
/// senkrechten Strich. Überschreibbar per `front_hotkeys` (Text oder Liste), leer = aus.
pub const FRONT_HOTKEYS: [&str; 2] = ["ctrl+shift+<", "f20"];

/// Fenster-Buchstaben, bewusst mit Reserve: ist keiner mehr frei, erscheint ein neues
/// Fenster still nicht im Deck.
pub const WINDOWS: [&str; 15] = ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O"];

/// Takt, in dem Status und Verbindungen neu gelesen werden (ms).
pub const POLL_MS: u64 = 400;
/// So lange (s) ohne Meldung gilt „denkt“ als eingeschlafen (idle).
pub const STALE_S: f64 = 900.0;
/// So lange (s) darf ein gebundenes Fenster getrennt UND ohne offenes VS-Code-Fenster
/// sein, bevor seine Bindung abgeräumt wird. Ein Reload trifft das nie.
pub const STALE_WINDOW_S: f64 = 3.0;
/// So lange (s) darf ein worktree-Marker ohne lebenden Agenten verwaisen, bevor der
/// worktree abgeräumt wird (Puffer gegen kurze Terminal-Listen-Aussetzer).
pub const WT_ORPHAN_GRACE_S: f64 = 20.0;
/// So oft (s) werden die `<repo>.wt`-Ordner direkt auf der Platte abgesucht.
pub const WT_DISK_SWEEP_INTERVAL_S: f64 = 60.0;
/// So lange (s) muss ein `.wt`-worktree ununterbrochen ohne Agenten gesehen werden,
/// bevor er fällt (bei 60 s Takt: zwei unabhängige Sichtungen).
pub const WT_DISK_ORPHAN_GRACE_S: f64 = 90.0;
