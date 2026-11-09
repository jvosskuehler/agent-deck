//! Der Dialog „Ticket zuweisen“ (`_ticket_dialog` in `deck/ui/ticket.py`): ein Feld,
//! die Ticketnummer; Enter = Zuweisen, Escape = Abbrechen, leer = verwerfen. Solange er
//! offen ist, fokussiert das Deck keinen neuen Agenten (er nähme ihm den Fokus).

use deck_core::view::TicketView;
use tauri::{AppHandle, Emitter};

use crate::state::{lock, shared};
use crate::ui_windows::{TICKET, get, present};

/// Inhalt für den offenen bzw. zuletzt geöffneten Slot.
pub fn view(app: &AppHandle) -> Option<TicketView> {
    let s = shared(app);
    let slot = lock(&s.ticket_slot).clone()?;
    let d = lock(&s.deck);
    Some(TicketView { current: d.store.tickets.get(&slot).cloned().unwrap_or_default(), lang: d.lang.code().into(), slot })
}

pub fn open(app: &AppHandle, slot: &str) {
    let s = shared(app);
    *lock(&s.ticket_slot) = Some(slot.to_string());
    let Some(v) = view(app) else { return };
    if let Some(w) = get(app, TICKET) {
        let title = if v.lang == "en" { format!("Ticket for {slot}") } else { format!("Ticket für {slot}") };
        let _ = w.set_title(&title);
        let _ = w.center();
    }
    let _ = app.emit("deck://ticket", v);
    present(app, TICKET);
}

/// Zuweisen (`Some`) oder Abbrechen (`None`). Leere Nummer = Abbrechen.
pub fn submit(app: &AppHandle, ticket: Option<String>) {
    let s = shared(app);
    if let Some(w) = get(app, TICKET) {
        let _ = w.hide();
    }
    let slot = lock(&s.ticket_slot).take();
    if let (Some(slot), Some(t)) = (slot, ticket.filter(|t| !t.trim().is_empty())) {
        lock(&s.deck).assign_ticket(&slot, &t);
        crate::poll::nudge(app);
    }
}
