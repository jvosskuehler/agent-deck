// Hell oder dunkel, wie das System gerade steht. Die Fenster folgen dem System-Thema
// (ui_windows.rs setzt keins fest), also meldet die Webview den Wechsel als Media Query;
// das Windows-Terminal-Profil „Agent Deck“ wechselt sein Schema genauso.

import { useSyncExternalStore } from "react";
import type { Scheme } from "../logic/palette";

const DARK = "(prefers-color-scheme: dark)";

function subscribe(onChange: () => void): () => void {
  const query = window.matchMedia(DARK);
  query.addEventListener("change", onChange);
  return () => query.removeEventListener("change", onChange);
}

function current(): Scheme {
  return window.matchMedia(DARK).matches ? "dark" : "light";
}

export function useScheme(): Scheme {
  return useSyncExternalStore(subscribe, current);
}
