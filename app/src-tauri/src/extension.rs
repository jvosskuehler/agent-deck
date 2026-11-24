//! Erweiterungen: wie eine andere Ausgabe des Decks eigene Teile anbaut, ohne dass dieser
//! Kern sie kennt.
//!
//! Dieses Repo baut die Ausgabe fürs Team, ohne jede Erweiterung (`main.rs` ruft
//! [`crate::run_with`] mit leerer Liste). Wer mehr will, baut eine eigene Binärdatei, die
//! diese Bibliothek einbindet und ihre Erweiterungen übergibt. Es gibt bewusst keinen
//! Schalter und keine Einstellung, die hier etwas freischaltet: was nicht übergeben wird,
//! ist nicht im Programm.
//!
//! Eine Erweiterung bekommt Haken an den Stellen, die der Kern ohnehin durchläuft:
//! Start, Takt (unter der Sperre des Decks und danach ohne), eigene `invoke`-Befehle,
//! Einstellungen (eigener Abschnitt im Fenster, eigene Schlüssel), Tastenkürzel-Liste,
//! Projektkürzel der Tickets, Ort für den Sonnenstand, Reserve-Agent und das Freigeben
//! eines Slots.
//! Jeder Haken hat eine leere Vorgabe.
//!
//! Sperr-Reihenfolge (`state.rs`): Haken, die `&mut Deck` bekommen, laufen unter der
//! Sperre des Decks. Eine Erweiterung darf darin ihre eigenen Mutexe nehmen, aber nie
//! umgekehrt unter ihrem Mutex das Deck sperren.

use std::sync::Arc;

use deck_core::i18n::Lang;
use deck_core::shortcuts::ShortcutRow;
use deck_core::sun::Place;
use deck_core::view::{DeckView, SettingsSection};
use serde_json::{Map, Value};
use tauri::ipc::Invoke;
use tauri::{AppHandle, Wry};

use crate::deck::Deck;
use crate::settings::Effect;

/// Inhalt von `deck_settings.json`.
pub type Settings = Map<String, Value>;

/// Ein `invoke`-Handler, wie `tauri::generate_handler!` ihn baut.
pub type InvokeHandler = Box<dyn Fn(Invoke<Wry>) -> bool + Send + Sync>;

pub trait DeckExtension: Send + Sync + 'static {
    /// Kurzer Name fürs Log.
    fn name(&self) -> &'static str;

    /// Beim Start, nachdem die Fenster des Kerns stehen: eigene Fenster, Threads, Tasten.
    /// Ein Fehler landet im Log, das Deck läuft ohne die Erweiterung weiter.
    fn setup(&self, _app: &AppHandle) -> Result<(), String> {
        Ok(())
    }

    /// Namen der eigenen `invoke`-Befehle. Nur diese gehen an den Handler aus
    /// [`DeckExtension::invoke_handler`].
    fn commands(&self) -> &'static [&'static str] {
        &[]
    }

    /// Der Handler der eigenen Befehle, typisch `Box::new(tauri::generate_handler![…])`;
    /// einmal beim Start geholt. Als Rückgabe statt als Methode mit `Invoke`: so zieht eine
    /// Erweiterung ohne Befehle (etwa in Tests) die Laufzeit von WebView2 nicht mit hinein,
    /// deren Test-Programme sonst ohne Manifest nicht starten (STATUS_ENTRYPOINT_NOT_FOUND).
    fn invoke_handler(&self) -> Option<InvokeHandler> {
        None
    }

    /// Je Takt unter der Sperre des Decks, nachdem die Ansicht gebaut ist.
    fn on_tick(&self, _deck: &mut Deck, _view: &DeckView, _now: f64) {}

    /// Je Takt ohne Sperre, nachdem die Ansicht gesendet ist (Fenster legen, Ereignisse).
    fn after_tick(&self, _app: &AppHandle) {}

    /// Projektkürzel für reine Ticketnummern (`ABC` macht aus `4116` `ABC-4116`). Ohne
    /// erkennt der Kern nur ausgeschriebene Keys wie `ABC-123`.
    fn ticket_prefix(&self, _settings: &Settings) -> Option<String> {
        None
    }

    /// Ort für „Hell/Dunkel: Sonnenstand“; ohne gilt der aus der Zeitzone.
    fn sun_place(&self, _settings: &Settings) -> Option<Place> {
        None
    }

    /// Eigener Abschnitt im Einstellungsfenster (die Oberfläche braucht dazu eine
    /// registrierte Ansicht mit derselben Kennung).
    fn settings_section(&self, _deck: &Deck) -> Option<SettingsSection> {
        None
    }

    /// Eine Einstellung setzen, die der Kern nicht kennt. `None` = nicht meine.
    fn set_setting(&self, _deck: &mut Deck, _key: &str, _value: &Value) -> Option<Result<Effect, String>> {
        None
    }

    /// Zusätzliche Zeilen im Fenster „Tastenkürzel“.
    fn shortcuts(&self, _settings: &Settings, _lang: Lang) -> Vec<ShortcutRow> {
        Vec::new()
    }

    /// Ein Slot wird frei (Agent geschlossen oder neues Ticket): was an ihm hing, kann
    /// weg. Unter der Sperre des Decks; Langes in einen eigenen Thread.
    fn on_slot_released(&self, _deck: &Deck, _slot: &str, _ticket: Option<&str>) {}

    /// Einen Agenten versteckt vorwärmen, damit ein neuer sofort da ist
    /// (`deck_core::reserve`, `reserve_agent.rs`). Der Kern kann es, schaltet es aber nie
    /// selbst ein: die Team-Ausgabe hat weder Schalter noch Reserve. Gefragt alle 2 s.
    fn reserve_agent(&self, _settings: &Settings) -> bool {
        false
    }
}

/// Die Erweiterungen dieser Ausgabe, in der übergebenen Reihenfolge.
#[derive(Clone, Default)]
pub struct Extensions(Vec<Arc<dyn DeckExtension>>);

impl Extensions {
    pub fn new(list: Vec<Box<dyn DeckExtension>>) -> Extensions {
        Extensions(list.into_iter().map(Arc::from).collect())
    }

    pub fn iter(&self) -> impl Iterator<Item = &Arc<dyn DeckExtension>> {
        self.0.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Das erste Projektkürzel, das eine Erweiterung kennt; sonst leer.
    pub fn ticket_prefix(&self, settings: &Settings) -> String {
        self.0.iter().find_map(|e| e.ticket_prefix(settings)).map(|p| p.trim().to_uppercase()).unwrap_or_default()
    }

    /// Will eine Erweiterung den Reserve-Agenten?
    pub fn reserve_agent(&self, settings: &Settings) -> bool {
        self.0.iter().any(|e| e.reserve_agent(settings))
    }

    pub fn sun_place(&self, settings: &Settings) -> Option<Place> {
        self.0.iter().find_map(|e| e.sun_place(settings))
    }

    pub fn shortcuts(&self, settings: &Settings, lang: Lang) -> Vec<ShortcutRow> {
        self.0.iter().flat_map(|e| e.shortcuts(settings, lang)).collect()
    }

    /// Wer den Befehl `cmd` ausführt.
    pub fn owner_of(&self, cmd: &str) -> Option<&Arc<dyn DeckExtension>> {
        self.0.iter().find(|e| e.commands().contains(&cmd))
    }

    /// Die Handler aller Erweiterungen mit Befehlen, je mit deren Namen.
    pub fn handlers(&self) -> Vec<(&'static [&'static str], InvokeHandler)> {
        self.0.iter().filter_map(|e| e.invoke_handler().map(|h| (e.commands(), h))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Prefix(&'static str);

    impl DeckExtension for Prefix {
        fn name(&self) -> &'static str {
            "prefix"
        }
        fn commands(&self) -> &'static [&'static str] {
            &["own_command"]
        }
        fn ticket_prefix(&self, _: &Settings) -> Option<String> {
            Some(self.0.into())
        }
    }

    #[test]
    fn without_extensions_there_is_no_prefix_no_place_and_no_extra_command() {
        let none = Extensions::default();
        assert!(none.is_empty());
        assert_eq!(none.ticket_prefix(&Settings::new()), "");
        assert!(none.sun_place(&Settings::new()).is_none());
        assert!(none.owner_of("get_view").is_none());
        assert!(!none.reserve_agent(&Settings::new()), "die Team-Ausgabe wärmt keinen Agenten vor");
    }

    #[test]
    fn the_first_extension_with_a_prefix_wins_and_owns_only_its_commands() {
        let ext = Extensions::new(vec![Box::new(Prefix(" abc ")), Box::new(Prefix("XYZ"))]);
        assert_eq!(ext.ticket_prefix(&Settings::new()), "ABC");
        assert_eq!(ext.owner_of("own_command").map(|e| e.name()), Some("prefix"));
        assert!(ext.owner_of("get_view").is_none());
    }
}
