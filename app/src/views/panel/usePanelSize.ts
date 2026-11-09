// Das Panel misst seinen natürlichen Inhalt und meldet ihn an Rust (panel_content_size).
// Rust setzt daraufhin die Fenstergröße; das Fenster folgt also dem Inhalt, nicht
// umgekehrt (wie im Python-Deck: kommt ein Agent dazu, wächst es, geht einer, rückt es nach).

import { useEffect, useState, type RefObject } from "react";
import { commands } from "../../ipc";

interface Size {
  width: number;
  height: number;
}

export function usePanelSize(ref: RefObject<HTMLElement | null>): Size | null {
  const [size, setSize] = useState<Size | null>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    let last: Size | null = null;
    let frame = 0;
    const report = () => {
      frame = 0;
      // offsetWidth/-Height: Rahmenmaß ohne Transform. Das Gleiten verändert die
      // gemeldete Größe darum nicht, und das Fenster wackelt nicht mit.
      const next = { width: Math.ceil(el.offsetWidth), height: Math.ceil(el.offsetHeight) };
      if (last && last.width === next.width && last.height === next.height) return;
      last = next;
      setSize(next);
      commands.panelContentSize(next.width, next.height);
    };
    // Mehrere Änderungen in einem Bild (Schrift geladen, Karte dazu) nur einmal melden.
    const schedule = () => {
      if (!frame) frame = requestAnimationFrame(report);
    };
    const ro = new ResizeObserver(schedule);
    ro.observe(el);
    // Nach dem Laden der Schrift misst der Text anders; einmal nachmessen.
    document.fonts?.ready.then(schedule).catch(() => undefined);
    schedule();
    return () => {
      ro.disconnect();
      if (frame) cancelAnimationFrame(frame);
    };
  }, [ref]);

  return size;
}
