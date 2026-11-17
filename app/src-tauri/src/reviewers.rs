//! Das Reviewer-Feld im Einstellungsfenster: Namen, Mailadressen oder Logins werden
//! gegen die Mitglieder der GitHub-Organisation aufgelöst (`deck_services::reviewers`,
//! über gh) und als Logins gespeichert (`Deck::apply_reviewers`).
//!
//! gh läuft außerhalb der Deck-Sperre in einem Hintergrund-Thread: ein Abruf dauert ein,
//! zwei Sekunden, der Takt darf so lange nicht stehen. Die Mitgliederliste bleibt zehn
//! Minuten im Speicher, eine Wahl im Fenster fragt GitHub darum nicht erneut.

use std::sync::OnceLock;

use deck_core::dash::reviewers::{ReviewerNote, ReviewerSave, ReviewerStatus};
use deck_core::deck_settings as ds;
use deck_services::reviewers::ReviewerResolver;
use tauri::{AppHandle, Emitter};

use crate::applog::note;
use crate::state::{lock, shared};

static RESOLVER: OnceLock<ReviewerResolver> = OnceLock::new();

/// Eingabe auflösen, Ergebnis speichern; Antwort: neuer Stand und Bericht (mehrdeutige
/// Einträge und Organisationen zur Wahl zeigt das Fenster unter dem Feld).
#[tauri::command]
pub async fn set_reviewers(app: AppHandle, text: String) -> Result<ReviewerSave, String> {
    let s = shared(&app);
    let org = ds::text(&lock(&s.deck).store.settings, ds::GITHUB_ORG);
    let report = tauri::async_runtime::spawn_blocking(move || RESOLVER.get_or_init(ReviewerResolver::system).resolve(&org, &text))
        .await
        .map_err(|e| e.to_string())?;
    // Nur Anzahlen: Namen und Adressen gehören nicht ins Protokoll.
    let count = |st: ReviewerStatus| report.results.iter().filter(|r| r.status == st).count();
    note(format!(
        "Reviewer aufgelöst: {} eindeutig, {} zur Wahl, {} ohne Treffer, Hinweis {:?}",
        count(ReviewerStatus::Unique),
        count(ReviewerStatus::Ambiguous),
        count(ReviewerStatus::None),
        report.note
    ));
    let view = {
        let mut d = lock(&s.deck);
        // Steht die Organisation noch zur Wahl, bleibt die alte Liste: ein Name wie „Leo“
        // wäre ungeprüft als Login gespeichert und träfe womöglich einen Fremden.
        if report.note != ReviewerNote::ChooseOrg {
            d.apply_reviewers(&report);
        }
        d.settings_view()
    };
    let _ = app.emit("deck://settings", &view);
    Ok(ReviewerSave { settings: view, report })
}
