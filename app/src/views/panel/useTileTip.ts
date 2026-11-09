// Hover über einer Karte → nach ≈ 500 ms den Tooltip bei Rust anfordern (hover_tile),
// beim Verlassen wieder verbergen.
//
// Ist eben noch ein Tooltip gestanden (Zeiger wandert von Karte zu Karte), kommt der
// nächste schneller: wer liest, soll nicht bei jeder Karte neu warten.

import { useCallback, useEffect, useRef } from "react";
import { commands } from "../../ipc";

const SHOW_MS = 500;
const WARM_MS = 120;
/** So lange nach dem Ausblenden gilt der Tooltip noch als „warm“. */
const WARM_WINDOW_MS = 400;

export function useTileTip(enabled: boolean, onShown?: (slot: string, el: HTMLElement) => void) {
  const timer = useRef<number | null>(null);
  const hovering = useRef<string | null>(null);
  const shown = useRef<string | null>(null);
  const hiddenAt = useRef(0);

  const clearTimer = () => {
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = null;
  };

  const hide = useCallback(() => {
    clearTimer();
    if (shown.current !== null) {
      shown.current = null;
      hiddenAt.current = performance.now();
      void commands.hoverTile(null, null);
    }
  }, []);

  const enter = useCallback(
    (slot: string, el: HTMLElement) => {
      if (!enabled) return;
      hovering.current = slot;
      clearTimer();
      const warm = performance.now() - hiddenAt.current < WARM_WINDOW_MS;
      timer.current = window.setTimeout(
        () => {
          timer.current = null;
          if (hovering.current !== slot || !el.isConnected) return;
          shown.current = slot;
          void commands.hoverTile(slot, el);
          onShown?.(slot, el);
        },
        warm ? WARM_MS : SHOW_MS,
      );
    },
    [enabled, onShown],
  );

  const leave = useCallback(
    (slot: string) => {
      if (hovering.current === slot) hovering.current = null;
      hide();
    },
    [hide],
  );

  /** Beim Ziehen, Einklappen oder Schließen: sofort weg, nichts nachkommen lassen. */
  const cancel = useCallback(() => {
    hovering.current = null;
    hide();
  }, [hide]);

  useEffect(() => {
    if (!enabled) cancel();
  }, [enabled, cancel]);

  useEffect(() => cancel, [cancel]);

  return { enter, leave, cancel };
}
