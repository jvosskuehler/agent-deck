// Texte der Oberfläche, Deutsch und Englisch.
//
// Statustexte, Limit-Namen und Reset-Zeiten kommen fertig aus Rust (DeckView); hier
// stehen nur die Texte, die die Oberfläche selbst setzt. Keine Gedankenstriche.

export type Lang = "de" | "en";

/** DeckView.lang und TicketView.lang („de“/„en“) → Sprache; alles Unbekannte wird Deutsch. */
export function langFromView(lang: string | undefined): Lang {
  return lang === "en" ? "en" : "de";
}

/** SettingsView.language („german“/„english“) → Sprache. */
export function langFromSetting(language: string | undefined): Lang {
  return language === "english" ? "en" : "de";
}

const TEXTS = {
  settings: ["Einstellungen", "Settings"],
  newAgentIn: ["Neuer Agent in {name}", "New agent in {name}"],
  closeAgent: ["Agent schließen", "Close agent"],
  notConnected: ["nicht verbunden", "not connected"],
  zoomGrip: ["Ziehen zum Vergrößern oder Verkleinern", "Drag to zoom"],
  waitingForAgents: ["Noch keine Agenten.", "No agents yet."],
  usageHead: ["Claude Nutzung", "Claude usage"],
  usageAccount: ["Konto: {account}", "Account: {account}"],
  usageReset: ["Reset {when}", "resets {when}"],
  usageOpen: ["Klick öffnet claude.ai/settings/usage", "Click opens claude.ai/settings/usage"],
  usageStale: ["alter Wert", "old value"],
  // Einstellungen
  savedNote: ["Gespeichert", "Saved"],
  liveNote: ["Änderungen gelten sofort", "Changes apply immediately"],
  newAgents: ["Neue Agenten", "New agents"],
  newAgentsHint: ["Vorgaben beim Start", "Defaults at start"],
  model: ["Modell", "Model"],
  mode: ["Modus", "Mode"],
  effort: ["Effort", "Effort"],
  deck: ["Deck", "Deck"],
  language: ["Sprache (Agenten und Deck)", "Language (agents and deck)"],
  dockEdge: ["Andocken am Bildschirmrand", "Dock to screen edge"],
  restartPanel: ["Panel neu starten", "Restart panel"],
  autoCompact: [
    "Automatisch kompaktieren (5 min bevor der Cache abläuft)",
    "Auto-compact (5 min before the cache expires)",
  ],
  // Ticket-Dialog
  ticketNumber: ["Ticketnummer", "Ticket number"],
  assign: ["Zuweisen", "Assign"],
  cancel: ["Abbrechen", "Cancel"],
} as const satisfies Record<string, readonly [string, string]>;

type TextKey = keyof typeof TEXTS;

/** Text in der gewählten Sprache; `{name}`-Platzhalter werden ersetzt. */
export function t(lang: Lang, key: TextKey, vars: Record<string, string> = {}): string {
  const raw: string = TEXTS[key][lang === "en" ? 1 : 0];
  return raw.replace(/\{(\w+)\}/g, (_, k: string) => vars[k] ?? "");
}
