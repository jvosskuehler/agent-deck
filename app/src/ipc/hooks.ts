// React-Haken auf die Schnittstelle: Startwert per get_*, danach jedes Ereignis.
//
// Jede Ansicht ruft beim Start einmal den passenden get_*-Befehl (IPC.md), damit sie
// nicht auf das erste Ereignis warten muss. Kommt ein Ereignis vor der Antwort an,
// gewinnt das Ereignis: es ist das neuere.

import { useEffect, useState } from "react";
import type { DeckView } from "../bindings/DeckView";
import type { DockView } from "../bindings/DockView";
import type { SettingsView } from "../bindings/SettingsView";
import type { TicketView } from "../bindings/TicketView";
import type { TooltipView } from "../bindings/TooltipView";
import type { EventName, EventPayloads } from "./contract";
import { commands, getBackend } from "./index";

function useLive<E extends EventName>(event: E, initial: () => Promise<EventPayloads[E]>): EventPayloads[E] | null {
  const [value, setValue] = useState<EventPayloads[E] | null>(null);
  useEffect(() => {
    let gotEvent = false;
    let alive = true;
    const off = getBackend().listen(event, (p) => {
      gotEvent = true;
      setValue(p);
    });
    initial()
      .then((v) => {
        if (alive && !gotEvent) setValue(v);
      })
      .catch((err) => console.warn(`[deck] Startwert für ${event} fehlt:`, err));
    return () => {
      alive = false;
      off();
    };
    // initial ist je Ansicht fest; eine neue Funktion je Render soll nicht neu abonnieren.
  }, [event]);
  return value;
}

export function useDeckView(): DeckView | null {
  return useLive("deck://view", commands.getView);
}

export function useDock(): DockView | null {
  return useLive("deck://dock", commands.getDock);
}

export function useSettings(): [SettingsView | null, (s: SettingsView) => void] {
  const live = useLive("deck://settings", commands.getSettings);
  const [local, setLocal] = useState<SettingsView | null>(null);
  // Die Antwort auf set_setting kann vor dem Ereignis ankommen; beides ist derselbe
  // Stand, gezeigt wird der jeweils letzte.
  useEffect(() => setLocal(live), [live]);
  return [local ?? live, setLocal];
}

/** Inhalt des Ticket-Dialogs; jedes Öffnen schickt den Slot neu. */
export function useTicket(): TicketView | null {
  const [ticket, setTicket] = useState<TicketView | null>(null);
  useEffect(() => {
    let gotEvent = false;
    const off = getBackend().listen("deck://ticket", (p) => {
      gotEvent = true;
      setTicket(p);
    });
    commands
      .getTicket()
      .then((v) => {
        if (!gotEvent && v) setTicket(v);
      })
      .catch((err) => console.warn("[deck] Startwert für deck://ticket fehlt:", err));
    return off;
  }, []);
  return ticket;
}

/** Tooltip-Inhalt; `null`, sobald Rust ihn ausblendet. */
export function useTooltip(): TooltipView | null {
  const [tip, setTip] = useState<TooltipView | null>(null);
  useEffect(() => {
    const b = getBackend();
    const offShow = b.listen("deck://tooltip", (p) => setTip(p));
    const offHide = b.listen("deck://tooltip-hide", () => setTip(null));
    return () => {
      offShow();
      offHide();
    };
  }, []);
  return tip;
}
