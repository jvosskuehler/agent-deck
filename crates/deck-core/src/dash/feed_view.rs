//! Das Anzeigemodell des PR-Feeds im Info-Fenster ([`FeedView`]) und des kleinen
//! Kommentar-Fensters ([`CommentPopup`]). Gebaut in `feed.rs`, gezeichnet in
//! `app/src/views/dash/FeedPane.tsx` und `app/src/views/comment/CommentView.tsx`.
//!
//! Zeiten kommen als Unix-Sekunden: die Uhrzeit („15:27“) formatiert die Oberfläche, nur
//! sie kennt die Ortszeit des Browsers. Alle übrigen Texte kommen fertig in der Sprache
//! der Oberfläche an.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Art eines Eintrags in der Zeitleiste; bestimmt nur die Farbe des Punkts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FeedKind {
    /// Gemergt (grün).
    Merged,
    /// Ein Hintergrund-Agent hat gearbeitet (violett).
    Agent,
    /// Ein Mensch hat ein Review abgegeben (blau).
    Review,
    /// Problem: CI rot mit unbekannter Ursache, Konflikt, Agent gescheitert (rot).
    Problem,
    /// Kleinkram: CI grün, rote CI mit bekannter Ursache, Reviewer nachgetragen (grau).
    Minor,
}

/// Ein Eintrag der Zeitleiste „Was passiert ist“.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FeedEvent {
    /// Unix-Sekunden.
    pub at: f64,
    pub kind: FeedKind,
    /// `client#393`; leer bei Einträgen ohne PR.
    pub label: String,
    /// Adresse des PRs (Klick auf das Kürzel).
    pub url: String,
    /// „gemergt“, „CI rot geworden“ …
    pub text: String,
    /// Gedämpft dahinter: Titel oder Ursache.
    pub detail: String,
    /// Bericht des Hintergrund-Agenten (letzte Antwort), aufklappbar; leer = keiner.
    pub report: String,
}

/// Ein Entwurf des Hintergrund-Agenten für eine Antwort an einen Menschen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DraftView {
    pub text: String,
    /// Antwort geht in einen Review-Thread (dann wird er nach dem Senden aufgelöst).
    pub thread: bool,
    /// Senden läuft gerade.
    pub sending: bool,
    /// Letzter Sendeversuch scheiterte (kurz).
    pub error: String,
}

/// Ein Kommentar eines Menschen, auf den ich noch nicht reagiert habe („Braucht dich“).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NeedCard {
    /// Schlüssel des PRs (`owner/repo#12`) für `dash_action`.
    pub key: String,
    /// Kürzel im Kreis (`LH`).
    pub initials: String,
    pub login: String,
    /// `devops#92`.
    pub label: String,
    pub title: String,
    /// Adresse des Kommentars (sonst des PRs).
    pub url: String,
    /// Unix-Sekunden des neuesten Kommentars.
    pub at: f64,
    /// „kommentiert“, „Änderungen gewünscht“, „antwortet im Thread“.
    pub verb: String,
    /// Anfang des Kommentars, eine Zeile.
    pub excerpt: String,
    /// Weitere offene Kommentare auf demselben PR.
    pub more: u32,
    /// Ein Hintergrund-Agent arbeitet daran („Claude setzt um, seit 3 Min.“); leer = keiner.
    pub agent: String,
    /// Fertiger Antwort-Entwurf.
    pub draft: Option<DraftView>,
    /// „Claude soll umsetzen“ ist möglich (kein Agent läuft, kein Entwurf offen).
    pub can_delegate: bool,
}

/// Ein laufender oder wartender Hintergrund-Agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentRow {
    /// Lauf-Id für `agentAbort:<id>`; leer, wenn der Lauf nur wartet.
    pub id: String,
    pub label: String,
    pub url: String,
    /// „Bot-Funde umsetzen“, „rote CI untersuchen“ …
    pub task: String,
    /// „seit 4 Min.“ bzw. „wartet, Session aktiv“.
    pub state: String,
    pub waiting: bool,
}

/// Was die Automatik mit einem eigenen PR vorhat (für „nicht mergen“ / „wieder freigeben“).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AutoRow {
    pub key: String,
    pub label: String,
    pub url: String,
    pub title: String,
    /// „mergt, sobald alles grün ist“, „wartet auf #12“, „gehalten“ …
    pub state: String,
    pub held: bool,
}

/// Der Feed hinter der Kachel „PRs offen“.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FeedView {
    pub needs: Vec<NeedCard>,
    pub agents: Vec<AgentRow>,
    /// Letzte 24 Stunden, neueste zuerst.
    pub events: Vec<FeedEvent>,
    /// Je eigener offener PR, was die Automatik vorhat.
    pub auto: Vec<AutoRow>,
    /// Not-Aus: aus = nur anzeigen, nichts tun.
    pub automation: bool,
    /// Stand bzw. Fehler; leer = alles gut.
    pub note: String,
}

/// Das kleine Fenster, das bei einem neuen Kommentar eines Menschen erscheint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CommentPopup {
    pub card: NeedCard,
    /// Die ersten Zeilen des Kommentars (bis etwa 300 Zeichen).
    pub text: String,
}
