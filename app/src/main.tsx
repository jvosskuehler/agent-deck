// Einstieg: alle Fenster laden dieselbe Seite und wählen ihre Ansicht über den Hash
// (#/panel, #/handle, #/tooltip, #/settings, #/ticket, #/glow, #/keys; siehe docs/IPC.md).

// IBM Plex wie im Terminal-Profil „Agent Deck“: Mono für alles, Sans für Prosa
// (Sessiontitel, Zusammenfassung). Mitgeliefert, nicht vom System erwartet.
import "@fontsource/ibm-plex-mono/400.css";
import "@fontsource/ibm-plex-mono/500.css";
import "@fontsource/ibm-plex-mono/600.css";
import "@fontsource/ibm-plex-mono/700.css";
import "@fontsource/ibm-plex-sans/400.css";
import "@fontsource/ibm-plex-sans/600.css";
import { StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { commands, getBackend, isNative } from "./ipc";
import { GlowView } from "./views/glow/GlowView";
import { HandleView } from "./views/handle/HandleView";
import { KeysView } from "./views/keys/KeysView";
import { PanelView } from "./views/panel/PanelView";
import { SettingsView } from "./views/settings/SettingsView";
import { TicketView } from "./views/ticket/TicketView";
import { TooltipView } from "./views/tooltip/TooltipView";
import "./styles/base.css";

type Route = "panel" | "handle" | "tooltip" | "settings" | "ticket" | "glow" | "keys";

function routeOf(hash: string): Route {
  const name = hash.replace(/^#\/?/, "").split(/[/?]/)[0];
  return name === "handle" || name === "tooltip" || name === "settings" || name === "ticket" || name === "glow" || name === "keys"
    ? name
    : "panel";
}

function App() {
  const [route, setRoute] = useState<Route>(() => routeOf(window.location.hash));
  useEffect(() => {
    const onHash = () => setRoute(routeOf(window.location.hash));
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);

  switch (route) {
    case "handle":
      return <HandleView />;
    case "tooltip":
      return <TooltipView />;
    case "settings":
      return <SettingsView />;
    case "ticket":
      return <TicketView />;
    case "glow":
      return <GlowView />;
    case "keys":
      return <KeysView />;
    default:
      return <PanelView />;
  }
}

// In Tauri gibt es kein Browser-Kontextmenü („Untersuchen“, „Neu laden“): Rechtsklick
// auf eine Karte öffnet das native Menü von Rust, überall sonst passiert nichts.
if (isNative()) {
  window.addEventListener("contextmenu", (e) => e.preventDefault());
  // Lebenszeichen für `ui_watch`: antwortet die Seite des sichtbaren Fensters nicht mehr,
  // startet Rust das Deck neu. Außerhalb von React, damit es auch ohne Render antwortet.
  getBackend().listen("deck://ping", () => commands.uiPong());
}

createRoot(document.getElementById("root") as HTMLElement).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
