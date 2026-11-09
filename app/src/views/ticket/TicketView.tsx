// Ansicht `#/ticket`: der Dialog „Ticket zuweisen“ aus dem Kontextmenü einer Kachel
// (Python ui/ticket.py). Ein Feld, die Ticketnummer; Enter weist zu, Escape bricht ab,
// leer bestätigt verwirft. Den Rest (Prompt an den Agenten, worktree) macht Rust.

import { useEffect, useRef, useState } from "react";
import type { TicketView as Ticket } from "../../bindings/TicketView";
import { commands, isNative } from "../../ipc";
import { useTicket } from "../../ipc/hooks";
import { langFromView, t } from "../../logic/i18n";
import "../../styles/settings.css";
import "../../styles/ticket.css";

export function TicketView() {
  const ticket = useTicket();
  if (!ticket) return <div className="settings" />;
  // Neuer Slot = neues Formular: der Entwurf des letzten Dialogs gehört nicht hierher.
  const body = <TicketForm key={ticket.slot} ticket={ticket} />;
  if (isNative()) return body;
  return <div className="settings-stage">{body}</div>;
}

function TicketForm({ ticket }: { ticket: Ticket }) {
  const lang = langFromView(ticket.lang);
  const [draft, setDraft] = useState(ticket.current);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    input.current?.focus();
    input.current?.select();
  }, []);

  const submit = () => commands.submitTicket(draft.trim() || null);
  const cancel = () => commands.submitTicket(null);

  return (
    <form
      className="settings ticket"
      onSubmit={(e) => {
        e.preventDefault();
        submit();
      }}
      onKeyDown={(e) => {
        if (e.key === "Escape") cancel();
      }}
    >
      <label htmlFor="ticket-id" className="set-label">
        {t(lang, "ticketNumber")}
      </label>
      <input
        id="ticket-id"
        ref={input}
        type="text"
        spellCheck={false}
        autoComplete="off"
        className="set-input ticket-input"
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
      />
      <div className="ticket-buttons">
        <button type="button" className="confirm-cancel" onClick={cancel}>
          {t(lang, "cancel")}
        </button>
        <button type="submit" className="confirm-go ticket-go">
          {t(lang, "assign")}
        </button>
      </div>
    </form>
  );
}
