// Die echte Anbindung an Rust über Tauri.

import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { Backend, CommandName, Commands, EventName, EventPayloads, ScreenRect, Unlisten } from "./contract";

export function createTauriBackend(): Backend {
  // Zoom der Webview (Rust setzt ihn, wenn das Panel zu groß wäre oder an der Ecke
  // gezogen wurde): eine CSS-Einheit ist dann scaleFactor × zoom Bildschirmpixel.
  let zoom = 1;
  return {
    native: true,

    invoke<K extends CommandName>(cmd: K, args: Commands[K]["args"]) {
      return tauriInvoke<Commands[K]["ret"]>(cmd, args as Record<string, unknown>);
    },

    listen<E extends EventName>(event: E, handler: (payload: EventPayloads[E]) => void): Unlisten {
      // Tauri meldet das Abmelden erst asynchron zurück. Wer vorher abmeldet (React
      // räumt im StrictMode sofort wieder ab), darf trotzdem keinen Hörer übrig lassen.
      let off: (() => void) | null = null;
      let cancelled = false;
      tauriListen<EventPayloads[E]>(event, (e) => handler(e.payload)).then((fn) => {
        if (cancelled) fn();
        else off = fn;
      });
      return () => {
        cancelled = true;
        off?.();
      };
    },

    async screenRect(el: Element): Promise<ScreenRect> {
      // Physische Bildschirm-Pixel: Innenecke des Fensters (physisch) plus die CSS-Lage
      // der Karte mal Skalierung. window.screenX taugt dafür nicht, Chromium rechnet
      // bei Monitoren mit verschiedener Skalierung in eigenen Einheiten.
      const win = getCurrentWindow();
      const [pos, scale] = await Promise.all([win.innerPosition(), win.scaleFactor()]);
      const r = el.getBoundingClientRect();
      const k = scale * zoom;
      return {
        x: Math.round(pos.x + r.left * k),
        y: Math.round(pos.y + r.top * k),
        width: Math.round(r.width * k),
        height: Math.round(r.height * k),
      };
    },

    setZoom(z: number) {
      if (Number.isFinite(z) && z > 0) zoom = z;
    },
  };
}
