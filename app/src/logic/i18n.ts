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
  slideGrip: ["Ziehen verschiebt das Deck entlang der Kante", "Drag to move the deck along the edge"],
  zoomGrip: ["Ziehen zum Vergrößern oder Verkleinern", "Drag to zoom"],
  waitingForAgents: ["Noch keine Agenten.", "No agents yet."],
  usageHead: ["Claude Nutzung", "Claude usage"],
  usageAccount: ["Konto: {account}", "Account: {account}"],
  usageReset: ["Reset {when}", "resets {when}"],
  usageOpen: ["Klick öffnet claude.ai/settings/usage", "Click opens claude.ai/settings/usage"],
  usageStale: ["alter Wert", "old value"],
  // Läuft noch ohne Fenster
  orphansHead: ["Läuft noch ohne Fenster", "Still running without a window"],
  orphansKill: ["Beenden", "Stop"],
  orphansKilling: ["beendet…", "stopping…"],
  orphansKillAll: ["Alle beenden", "Stop all"],
  orphansNote: [
    "Nie angeboten: Claude, Docker, das Deck selbst, Prozesse in Sitzung 0 und alles, was nicht aus einem Agentenfenster stammt.",
    "Never offered: Claude, Docker, the deck itself, session 0 processes and anything not started from an agent window.",
  ],
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
  appearance: ["Hell/Dunkel", "Light/Dark"],
  appearanceFixedHint: [
    "Nur Deck und Agenten. Offene Agentenfenster wechseln beim nächsten Start.",
    "Deck and agents only. Open agent windows switch when they next start.",
  ],
  appearanceSunHint: [
    "Schaltet Windows bei Sonnenaufgang hell und bei Sonnenuntergang dunkel ({place}).",
    "Switches Windows to light at sunrise and to dark at sunset ({place}).",
  ],
  autoCompact: [
    "Automatisch /compact (5 min bevor der Cache abläuft)",
    "Auto /compact (5 min before the cache expires)",
  ],
  account: ["Konto und Ordner", "Account and folders"],
  accountHint: ["gilt für dieses Konto", "applies to this account"],
  reposRoot: ["Repos-Ordner", "Repos folder"],
  agentEnv: ["Agenten laufen in", "Agents run in"],
  agentEnvHint: ["Wechsel gilt nach „Panel neu starten“", "Change applies after “Restart panel”"],
  wslDistro: ["WSL-Distro", "WSL distro"],
  wslNone: ["WSL ist nicht installiert", "WSL is not installed"],
  reposRootEmpty: ["nicht gewählt", "not chosen"],
  reposRootChange: ["Ändern…", "Change…"],
  reposUnset: ["Repos-Ordner fehlt. Wähle den Ordner, in dem deine Repos liegen.", "Repos folder missing. Choose the folder that holds your repos."],
  reposMissing: ["Repos-Ordner nicht gefunden. Wähle ihn neu.", "Repos folder not found. Choose it again."],
  reposNoRepos: ["Im Repos-Ordner liegt kein git-Repo. Wähle den Ordner darüber.", "No git repo in the repos folder. Choose the folder above them."],
  modelDefault: ["Standard", "Default"],
  autoPermissionMode: ["Neue Agenten in Auto schalten", "Switch new agents to auto"],
  autoPermissionModeHint: ["tippt Shift+Tab, bis der Modus auf Auto steht", "types Shift+Tab until the mode reads auto"],
  autoCompactHint: ["schickt Prompts, verbraucht Kontingent", "sends prompts, uses your quota"],
  more: ["Erweitert", "Advanced"],
  less: ["Weniger", "Less"],
  shortcuts: ["Tastenkürzel", "Shortcuts"],
  shortcutsHint: ["Alt+V im Agenten, die übrigen überall", "Alt+V in the agent, the rest everywhere"],
  openGlow: ["Glow einstellen", "Adjust glow"],
  // Glow einstellen
  glowTitle: ["Glow einstellen", "Adjust glow"],
  glowLive: ["Wirkt sofort am gewählten Agenten", "Applies at once to the selected agent"],
  glowLight: ["Hell", "Light"],
  glowDark: ["Dunkel", "Dark"],
  glowActive: ["aktiv", "active"],
  glowReset: ["Zurücksetzen", "Reset"],
  glowColor: ["Farbe", "Color"],
  glowOuter: ["Zweite Farbe", "Second color"],
  glowOuterAuto: ["Zweite Farbe (auto)", "Second color (auto)"],
  glowOuterReset: ["Zweite Farbe wie die erste", "Second color like the first"],
  glowIntensity: ["Stärke", "Strength"],
  glowWidth: ["Breite", "Width"],
  glowSpeed: ["Tempo", "Speed"],
  glowKind: ["Glow-Art", "Glow type"],
  glowAnim: ["Animation", "Animation"],
  glowKind_weichlicht: ["Weichlicht", "Soft light"],
  glowKind_glutsaum: ["Glutsaum", "Ember rim"],
  glowKind_zwielicht: ["Zwielicht", "Twilight"],
  glowKind_bodenlicht: ["Bodenlicht", "Floor light"],
  glowKind_ecklicht: ["Ecklicht", "Corner light"],
  glowAnim_still: ["Still", "Still"],
  glowAnim_atmen: ["Atmen", "Breathe"],
  glowAnim_umlauf: ["Umlauf", "Orbit"],
  glowAnim_stroemung: ["Strömung", "Flow"],
  glowAnim_farbwandern: ["Farbwandern", "Color drift"],
  glowAnim_funken: ["Funken", "Sparks"],
  glowPresets: ["Vorlagen", "Presets"],
  glowSaved: ["Gemerkt", "Saved"],
  glowSave: ["Merken", "Save"],
  glowSavedEmpty: ["Noch nichts gemerkt. „Merken“ legt den jetzigen Glow hier ab.", "Nothing saved yet. “Save” keeps the current glow here."],
  glowSavedLoad: ["Diesen Glow einstellen", "Use this glow"],
  glowSavedDelete: ["Vergessen", "Forget"],
  // Ticket-Dialog
  ticketNumber: ["Ticketnummer", "Ticket number"],
  assign: ["Zuweisen", "Assign"],
  cancel: ["Abbrechen", "Cancel"],
} as const satisfies Record<string, readonly [string, string]>;

export type TextKey = keyof typeof TEXTS;

/**
 * Eine Übersetzungsfunktion für eine eigene Tabelle (Erweiterungen bringen ihre Texte
 * mit, `extensions.ts`). Gleiche Regeln: Platzhalter `{name}`, keine Gedankenstriche.
 */
export function translator<K extends string>(texts: Record<K, readonly [string, string]>) {
  return (lang: Lang, key: K, vars: Record<string, string> = {}): string => {
    const raw: string = texts[key][lang === "en" ? 1 : 0];
    return raw.replace(/\{(\w+)\}/g, (_, k: string) => vars[k] ?? "");
  };
}

/** Text in der gewählten Sprache; `{name}`-Platzhalter werden ersetzt. */
export const t: (lang: Lang, key: TextKey, vars?: Record<string, string>) => string = translator(TEXTS);
