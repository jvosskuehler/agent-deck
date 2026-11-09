//! Was der Kopf eines Blocks über das ganze Fenster kann (`deck/ui/connect.py`): seine
//! Bindung vergessen und ein VS-Code-Fenster per Klick an den Buchstaben binden.
//!
//! Fenster werden über den Workspace-NAMEN gebunden, nicht über ein Handle: ein Handle
//! überlebt das Neuladen des Fensters nicht.

use deck_core::binding::{is_placeholder_ws, repo_from_title, window_from_title};
use deck_core::config::VSCODE_MARKER;

use crate::deck::Deck;

/// So lange wartet „Mit Fenster verbinden“ auf den Klick in ein VS-Code-Fenster (s).
pub const BIND_TIMEOUT_S: f64 = 20.0;

/// Ein laufendes „Mit Fenster verbinden“.
#[derive(Debug, Clone, PartialEq)]
pub struct BindRequest {
    pub win: String,
    pub deadline: f64,
}

impl Deck {
    /// „Mit Fenster verbinden“: der nächste Klick in ein VS-Code-Fenster (binnen 20 s)
    /// bindet es an diesen Buchstaben. Ein zweiter Aufruf für denselben Block bricht ab.
    pub fn start_bind(&mut self, win: &str, now: f64) {
        if self.bind.as_ref().is_some_and(|b| b.win == win) {
            self.bind = None;
            return;
        }
        if self.hosts.is_wt(win) {
            return;
        }
        self.bind = Some(BindRequest { win: win.to_string(), deadline: now + BIND_TIMEOUT_S });
    }

    pub fn cancel_bind(&mut self) {
        self.bind = None;
    }

    /// Im Takt: steht jetzt ein VS-Code-Fenster vorn, wird es gebunden. Ein Fenster
    /// ohne Ordner (nur „Visual Studio Code“ im Titel) wird nicht gebunden: es verbände
    /// sich nie und bliebe als Phantom stehen.
    pub fn poll_bind(&mut self, now: f64) {
        let Some(req) = self.bind.clone() else { return };
        if now > req.deadline {
            self.bind = None;
            return;
        }
        let Some(fg) = self.desktop.foreground() else { return };
        if !fg.title.contains(VSCODE_MARKER) {
            return;
        }
        self.bind = None;
        // MIT Remote-Anhängsel binden (so meldet es auch die Extension), aber OHNE prüfen:
        // ein Remote-Fenster ohne Ordner hat nur das Anhängsel im Titel.
        let repo = window_from_title(&fg.title);
        let bare = repo_from_title(&fg.title);
        if is_placeholder_ws(Some(&bare)) || repo == VSCODE_MARKER || repo.is_empty() {
            return;
        }
        // Derselbe Repo-Name an zwei Buchstaben gäbe zwei Blöcke: eine alte Bindung lösen.
        let other: Vec<String> = self
            .store
            .bindings
            .iter()
            .filter(|(k, r)| **k != req.win && r.to_lowercase() == repo.to_lowercase())
            .map(|(k, _)| k.clone())
            .collect();
        for k in other {
            self.store.bindings.remove(&k);
        }
        self.store.bindings.insert(req.win.clone(), repo.clone());
        self.store.save_bindings();
        self.hosts.assign(&repo, &req.win);
    }

    /// Wartet dieser Block gerade auf den Klick in ein Fenster?
    pub fn binding(&self, win: &str) -> bool {
        self.bind.as_ref().is_some_and(|b| b.win == win)
    }
}
