// Einstieg: alle Fenster laden dieselbe Seite und wählen ihre Ansicht über den Hash
// (#/panel, #/handle, #/tooltip, #/settings; siehe docs/IPC.md).

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
import { isNative } from "./ipc";
import { HandleView } from "./views/handle/HandleView";
import { PanelView } from "./views/panel/PanelView";
import { SettingsView } from "./views/settings/SettingsView";
import { TicketView } from "./views/ticket/TicketView";
import { TooltipView } from "./views/tooltip/TooltipView";
import "./styles/base.css";

type Route = "panel" | "handle" | "tooltip" | "settings" | "ticket";

function routeOf(hash: string): Route {
  const name = hash.replace(/^#\/?/, "").split(/[/?]/)[0];
  return name === "handle" || name === "tooltip" || name === "settings" || name === "ticket" ? name : "panel";
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
    default:
      return <PanelView />;
  }
}

// In Tauri gibt es kein Browser-Kontextmenü („Untersuchen“, „Neu laden“): Rechtsklick
// auf eine Karte öffnet das native Menü von Rust, überall sonst passiert nichts.
if (isNative()) {
  window.addEventListener("contextmenu", (e) => e.preventDefault());
}

createRoot(document.getElementById("root") as HTMLElement).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
