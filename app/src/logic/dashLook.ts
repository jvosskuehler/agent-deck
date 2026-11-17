// Farben der Info-Karten, die je Zustand gerechnet werden: der Tagesstreifen nimmt die
// Pigmente der Statusfarben (palette.ts), damit „Rückfrage“ im Streifen dieselbe Farbe hat
// wie auf der Agentenkarte. Töne von Text (gut, Achtung, schlecht) stehen fest in dash.css.

import type { Status } from "../bindings/Status";
import { toneOf, type Scheme } from "./palette";

/** Feld im Tagesstreifen: Pigment bei Arbeit, Haarlinie bei Ruhe, fast nichts ohne Agent. */
export function timelineColor(status: Status, scheme: Scheme): string {
  switch (status) {
    case "waiting":
    case "thinking":
    case "done":
      return toneOf(status, scheme).pig;
    case "idle":
    case "lost":
      return "var(--hl-high)";
    case "none":
      return "var(--hl-low)";
  }
}
