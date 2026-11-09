//! Ein Ticket an einen Agenten geben (`deck/ui/ticket.py`); der Agent legt sich selbst
//! einen git worktree an und meldet dessen Pfad in `state\<slot>.worktree`. Beim
//! Schließen räumt das Deck genau diesen worktree wieder ab, nie den eines anderen Slots.

use std::fs;
use std::path::Path;

use deck_core::paths::{found_ticket_path, worktree_marker_path};
use deck_core::prompts::ticket_prompt;

use crate::deck::Deck;

impl Deck {
    /// Ordner des Blocks, wenn er in Windows Terminal über einem Ordner OHNE eigenes
    /// Git-Repo läuft (Heimat-Block): dort führte der normale Prompt ins Leere.
    fn hub_root(&self, slot: &str) -> Option<String> {
        let folder = self.wt.folder(&Deck::letter(slot))?;
        (!Path::new(&folder).join(".git").exists()).then_some(folder)
    }

    /// Ticket zuweisen („Zuweisen“ im Dialog): sofort auf der Karte, Prompt mit festem
    /// Branch. Ein vorheriger worktree dieses Slots wird erst abgeräumt, solange altes
    /// Ticket und Marker seinen Pfad noch verraten.
    pub fn assign_ticket(&mut self, slot: &str, ticket: &str) {
        let ticket = ticket.trim();
        if ticket.is_empty() || !self.wt.has(&Deck::letter(slot)) {
            return;
        }
        self.active_slot = Some(slot.to_string());
        self.cleanup_worktrees(slot);
        self.store.tickets.insert(slot.to_string(), ticket.to_string());
        self.store.save_tickets();
        self.clear_found_ticket(slot);
        self.clear_worktree_marker(slot);
        let prefix = self.jira_prefix();
        let hub = self.hub_root(slot);
        let text = ticket_prompt(&self.paths.state_dir, slot, ticket, "", &prefix, hub.as_deref());
        self.wt.send_text(slot, &text, true);
    }

    /// Zugewiesenes bzw. gemeldetes Ticket vergessen (nur Anzeige; Agent und worktree
    /// bleiben).
    pub fn clear_ticket(&mut self, slot: &str) {
        if self.store.tickets.remove(slot).is_some() {
            self.store.save_tickets();
        }
        self.clear_found_ticket(slot);
        self.found.remove(slot);
    }

    pub fn clear_found_ticket(&self, slot: &str) {
        let _ = fs::remove_file(found_ticket_path(&self.paths.state_dir, slot));
    }

    pub fn clear_worktree_marker(&self, slot: &str) {
        let _ = fs::remove_file(worktree_marker_path(&self.paths.state_dir, slot));
    }

    /// Beim Schließen die worktree(s) des Slots entfernen. Die Signale (Marker, Ticket,
    /// cwd) werden jetzt gelesen, solange sie noch da sind; das Entfernen (git,
    /// blockierend) läuft im Hintergrund. Pfade, die ein ANDERER Slot per Marker
    /// beansprucht, sind tabu: zwei Agenten am selben Ticket teilen sich einen.
    pub fn cleanup_worktrees(&mut self, slot: &str) {
        let ticket = self.store.tickets.get(slot).or_else(|| self.found.get(slot)).cloned();
        self.services.cleanup_slot(slot, ticket.as_deref());
        self.clear_worktree_marker(slot);
    }
}
