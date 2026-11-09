// Beispieldaten für den Browser ohne Tauri: so, wie Rust sie schicken würde.
//
// Ein Heimat-Block „Repos“ (Windows Terminal) mit fünf Agenten in allen Zuständen und
// ein VS-Code-Block mit drei Agenten, einer davon getrennt. Die Texte entsprechen
// denen der Vorlagen in docs/design-refs, damit Screenshots vergleichbar sind.

import type { BlockView } from "../../bindings/BlockView";
import type { SettingsView } from "../../bindings/SettingsView";
import type { Status } from "../../bindings/Status";
import type { TileView } from "../../bindings/TileView";
import type { UsageView } from "../../bindings/UsageView";
import type { Lang } from "../../logic/i18n";

const STATUS_TEXT: Record<Status, [string, string]> = {
  waiting: ["Rückfrage", "needs input"],
  thinking: ["denkt", "thinking"],
  done: ["ungelesen", "unread"],
  lost: ["getrennt", "disconnected"],
  idle: ["idle", "idle"],
  unknown: ["unbekannt", "unknown"],
  none: ["", ""],
};

export function statusText(status: Status, lang: Lang): string {
  return STATUS_TEXT[status][lang === "en" ? 1 : 0];
}

/** Dringlichkeit wie im Python-Deck: Rückfrage vor getrennt vor ungelesen vor denkt. */
const RANK: Partial<Record<Status, number>> = { waiting: 0, lost: 1, done: 2, thinking: 3 };

export function urgentOf(blocks: readonly BlockView[]): Status | null {
  let best: Status | null = null;
  for (const b of blocks) {
    for (const t of b.tiles) {
      const r = RANK[t.status];
      if (r === undefined) continue;
      if (best === null || r < (RANK[best] ?? 99)) best = t.status;
    }
  }
  return best;
}

type TileSeed = Omit<TileView, "statusLabel" | "statusChangedAt" | "selected">;

const HOME: TileSeed[] = [
  { slot: "B1", status: "waiting", model: "Opus 5.5", effort: "xhigh", mode: "auto", title: "ORPH-4062 Gratismonat: Testuhr in Stripe vorstellen", reference: "ORPH-4062", referenceDetected: false },
  { slot: "B2", status: "thinking", model: "Opus 5.5", effort: "ultracode", mode: "bypass", title: "ORPH-3773 Rechtekonzept Integrationstests", reference: "ORPH-3773", referenceDetected: false },
  { slot: "B3", status: "done", model: "Fable 5.1", effort: "high", mode: "auto", title: "PR 180 Review-Threads schließen", reference: "#180", referenceDetected: false },
  { slot: "B4", status: "lost", model: "Opus 5.5", effort: "xhigh", mode: "auto", title: "ORPH-3192 Mehrfach-RDS auf der Test-VM", reference: "ORPH-3192", referenceDetected: false },
  { slot: "B5", status: "idle", model: "Opus 5.5", effort: "max", mode: "auto", title: "Agent Deck neues Design", reference: "ORPH-4116", referenceDetected: true },
  { slot: "B6", status: "idle", model: "Opus 5.5", effort: "high", mode: "auto", title: "Makropad-Raster", reference: null, referenceDetected: false },
];

const VSCODE: TileSeed[] = [
  { slot: "A1", status: "thinking", model: "Opus 5.5", effort: "xhigh", mode: "auto", title: null, reference: "ORPH-4107", referenceDetected: false },
  { slot: "A2", status: "idle", model: "Fable 5.1", effort: "high", mode: "plan", title: null, reference: null, referenceDetected: false },
  { slot: "A3", status: "lost", model: "Opus 5.5", effort: "xhigh", mode: "auto", title: null, reference: "ORPH-4105", referenceDetected: false },
];

function tile(seed: TileSeed, lang: Lang, selected: string, now: number): TileView {
  return { ...seed, statusLabel: statusText(seed.status, lang), selected: seed.slot === selected, statusChangedAt: now };
}

/** Zusatzblock für ?blocks=3: ein Remote-Fenster, damit sich Blöcke ziehen lassen. */
const REMOTE: TileSeed[] = [
  { slot: "C1", status: "unknown", model: "Opus 5.5", effort: "high", mode: "auto", title: null, reference: null, referenceDetected: false },
  { slot: "C2", status: "unknown", model: "Fable 5.1", effort: "medium", mode: "plan", title: null, reference: "#347", referenceDetected: false },
];

export function initialBlocks(lang: Lang, now: number, extra = false): BlockView[] {
  const selected = "B2";
  const blocks: BlockView[] = [
    { letter: "B", name: "Repos", host: "terminal", home: true, connected: true, remote: false, binding: false, rows: [3, 3], screenCols: 3, tiles: HOME.map((s) => tile(s, lang, selected, now)) },
    { letter: "A", name: "orpheus-management-api", host: "vscode", home: false, connected: true, remote: false, binding: false, rows: [], screenCols: 0, tiles: VSCODE.map((s) => tile(s, lang, selected, now)) },
  ];
  if (extra) {
    blocks.push({ letter: "C", name: "orpheus-client-electron", host: "vscode", home: false, connected: true, remote: true, binding: false, rows: [], screenCols: 0, tiles: REMOTE.map((s) => tile(s, lang, selected, now)) });
  }
  return blocks;
}

export function usage(lang: Lang): UsageView {
  const de = lang !== "en";
  return {
    primary: [
      { label: "Session", percent: 34, color: "#6ee7a8", resets: "16:40" },
      { label: de ? "Woche" : "Week", percent: 61, color: "#ffc48a", resets: de ? "Mo 09:00" : "Mon 09:00" },
    ],
    extra: [{ label: de ? "Woche Opus" : "Week Opus", percent: 22, color: "#6ee7a8", resets: de ? "Mo 09:00" : "Mon 09:00" }],
    stale: false,
    error: null,
    asOf: de ? "Stand: 23:41" : "As of: 23:41",
  };
}

export function initialSettings(): SettingsView {
  return {
    model: "opus[1m]",
    mode: "auto",
    effort: "xhigh",
    language: "german",
    ring: true,
    jiraPrefix: "ORPH",
    dockEdge: "top",
    terminalHome: "C:\\Users\\Jorrit\\Repos",
  };
}

/** Zusammenfassungen für den Tooltip, wie die KI sie liefern würde. */
export const SUMMARIES: Record<string, { summary: string; lastPrompt: string }> = {
  B1: { summary: "Fragt, ob die Testuhr in der Stripe-Sandbox auf den 1. des Monats vorgestellt werden soll, damit der Gratismonat abläuft.", lastPrompt: "Stell die Testuhr vor und prüf die Rechnung" },
  B2: { summary: "Lässt die Integrationstests zum Rechtekonzept auf dem Titan laufen; 3 von 41 sind noch rot.", lastPrompt: "Mach die Integrationstests grün" },
  B3: { summary: "Alle Review-Threads in PR 180 beantwortet und aufgelöst, Commit 4ca3429 gepusht.", lastPrompt: "Schließ die Review-Threads" },
  B4: { summary: "Zweite RDP-Sitzung auf der Test-VM gestartet; das Fenster antwortet nicht mehr.", lastPrompt: "Teste die zweite Sitzung auf orph3192-ts" },
  B5: { summary: "Neues Aussehen des Decks: Karten nach Variante 05, Farben nach Variante 01.", lastPrompt: "Bau die Oberfläche nach DESIGN.md" },
  A1: { summary: "Rechtstexte als PDF: baut die Vorschau für die Endnutzer-Seite.", lastPrompt: "Bau das PDF für die Rechtstexte" },
  A2: { summary: "Plant die Umstellung der Sessions-Route, wartet auf Freigabe.", lastPrompt: "Mach einen Plan für /sessions" },
  A3: { summary: "Gruppen-Claim: Anleitung für AD FS geschrieben, VS-Code-Fenster getrennt.", lastPrompt: "Schreib die Anleitung für den Gruppen-Claim" },
};
