// React-Haken auf die Schnittstelle: Startwert per get_*, danach jedes Ereignis.
//
// Jede Ansicht ruft beim Start einmal den passenden get_*-Befehl (IPC.md), damit sie
// nicht auf das erste Ereignis warten muss. Kommt ein Ereignis vor der Antwort an,
// gewinnt das Ereignis: es ist das neuere.

import { useEffect, useState } from "react";
import type { DeckView } from "../bindings/DeckView";
import type { DockView } from "../bindings/DockView";
import type { TicketView } from "../bindings/TicketView";
import type { TooltipView } from "../bindings/TooltipView";
import type { EventName, EventPayloads } from "./contract";
import { commands, getBackend } from "./index";

type Live<E extends EventName> = [EventPayloads[E] | null, (value: EventPayloads[E]) => void];

/**
 * Startwert plus jedes Ereignis. Ein Startwert null (es gibt noch nichts, etwa keinen
 * Ticket-Dialog) lässt den Wert null. Der Setzer gibt einen eigenen Stand vor, bis das
 * nächste Ereignis kommt.
 */
function useLive<E extends EventName>(event: E, initial: () => Promise<EventPayloads[E] | null>): Live<E> {
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
  return [value, setValue];
}

export function useDeckView(): DeckView | null {
  return useLive("deck://view", commands.getView)[0];
}

export function useDock(): DockView | null {
  return useLive("deck://dock", commands.getDock)[0];
}

/**
 * Einstellungen samt Setzer für das sofortige Zeigen einer Auswahl. Die Antwort auf
 * set_setting kann vor dem Ereignis ankommen; beides ist derselbe Stand, gezeigt wird
 * der jeweils letzte. Darum genügt ein Stand für beides.
 */
export function useSettings(): Live<"deck://settings"> {
  return useLive("deck://settings", commands.getSettings);
}

/** Inhalt des Ticket-Dialogs; jedes Öffnen schickt den Slot neu. */
export function useTicket(): TicketView | null {
  return useLive("deck://ticket", commands.getTicket)[0];
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
