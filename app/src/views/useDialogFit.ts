// Ein Dialog misst die Höhe seines Inhalts und meldet sie an Rust (dialog_content_size).
// Rust setzt daraufhin die Fensterhöhe: das Fenster wächst mit dem Inhalt (z. B. jedem
// gemerkten Glow) und schrumpft wieder, statt einen Rollbalken zu zeigen.

import { useEffect, type RefObject } from "react";
import { commands, isNative } from "../ipc";

/** `ready`: der Inhalt steht im DOM (vorher gibt es nichts zu messen). */
export function useDialogFit(ref: RefObject<HTMLElement | null>, ready: boolean) {
  useEffect(() => {
    const el = ref.current;
    if (!ready || !el || !isNative()) return;
    let last = 0;
    let frame = 0;
    const report = () => {
      frame = 0;
      const height = Math.ceil(el.offsetHeight);
      if (height === last) return;
      last = height;
      commands.dialogContentSize(height);
    };
    // Mehrere Änderungen in einem Bild (Schrift geladen, Zeile dazu) nur einmal melden.
    const schedule = () => {
      if (!frame) frame = requestAnimationFrame(report);
    };
    const ro = new ResizeObserver(schedule);
    ro.observe(el);
    document.fonts?.ready.then(schedule).catch(() => undefined);
    schedule();
    return () => {
      ro.disconnect();
      if (frame) cancelAnimationFrame(frame);
    };
  }, [ref, ready]);
}
