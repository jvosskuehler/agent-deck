// Hell oder dunkel, wie das System gerade steht. Die Fenster folgen dem System-Thema
// (ui_windows.rs setzt keins fest), also meldet die Webview den Wechsel als Media Query;
// das Windows-Terminal-Profil „Agent Deck“ wechselt sein Schema genauso.
//
// Eine Media Query und ein Listener für alle: jede Karte fragt das Schema ab, und ein
// eigener Listener je Karte wäre bei jedem Einhängen ein neues Abonnement beim Browser.

import { useSyncExternalStore } from "react";
import type { Scheme } from "../logic/palette";

const DARK = "(prefers-color-scheme: dark)";

let query: MediaQueryList | null = null;
const listeners = new Set<() => void>();

function notify() {
  listeners.forEach((l) => l());
}

function subscribe(onChange: () => void): () => void {
  query ??= window.matchMedia(DARK);
  if (listeners.size === 0) query.addEventListener("change", notify);
  listeners.add(onChange);
  return () => {
    listeners.delete(onChange);
    if (listeners.size === 0) query?.removeEventListener("change", notify);
  };
}

function current(): Scheme {
  query ??= window.matchMedia(DARK);
  return query.matches ? "dark" : "light";
}

export function useScheme(): Scheme {
  return useSyncExternalStore(subscribe, current);
}
