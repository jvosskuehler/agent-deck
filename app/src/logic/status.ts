// Farbe je Zustand: die Rechnung hinter dem Aussehen einer Karte.
//
// Werte aus docs/DESIGN.md (Papier-Look, Variante „02 Farbfeld“), Farben in
// logic/palette.ts. Sie stehen hier als reine Funktionen, damit ein Test sie festhält:
// genau an diesen Zahlen ist der C#-Port gescheitert, sie dürfen nicht still wandern.

import type { Status } from "../bindings/Status";
import { PALETTES, toneOf, type Scheme } from "./palette";

/** Wie laut eine Karte ist: atmend, ruhig getönt, gar nicht getönt. */
type Attention = "strong" | "soft" | "none";

function attentionOf(status: Status): Attention {
  switch (status) {
    case "waiting":
    case "thinking":
      return "strong";
    case "done":
    case "lost":
      return "soft";
    default:
      return "none";
  }
}

/** Alles, was die Fläche einer Karte braucht, fertig als CSS-Werte. */
interface TileLook {
  /** Statustext oben links. */
  ink: string;
  /** Tönung über dem Kartengrund; bei einer ruhigen Karte deckend in deren Fläche. */
  tint: string;
  /** Fläche der atmenden Ebene, `transparent` ohne Atmen. */
  pulse: string;
  /** Die Ebene atmet (Deckkraft 0,15 ↔ 1 in 3 s). */
  breathe: boolean;
  /** Karte matt (Remote: Zustand von hier aus nicht sichtbar). */
  matte: boolean;
}

/**
 * Aussehen einer Karte je Zustand: die ganze Karte ist ein Feld in der Statusfarbe,
 * ohne Rand und ohne Leuchten. Rückfrage und denkt pulsieren in der Fläche.
 *
 * idle, none und unknown bekommen statt einer Tönung die ruhige Fläche (Overlay), damit
 * sie als „nichts zu tun“ zurücktreten; unbekannt ist zusätzlich matt, weil die Karte da
 * und bedienbar ist, ihr Zustand aber nicht zu sehen.
 */
export function tileLook(status: Status, scheme: Scheme): TileLook {
  const pal = PALETTES[scheme];
  const tone = toneOf(status, scheme);
  const kind = attentionOf(status);
  return {
    ink: tone.ink,
    tint: kind === "none" ? pal.idleCard : tone.fill,
    pulse: kind === "strong" ? tone.pig + pal.pulse : "transparent",
    breathe: kind === "strong",
    matte: status === "unknown",
  };
}

/** Ring der ausgewählten (zuletzt angeklickten) Karte, mit Abstand in Panelfarbe. */
export function selectedRing(scheme: Scheme): string {
  const pal = PALETTES[scheme];
  return `0 0 0 2px ${pal.panel}, 0 0 0 3.5px ${pal.ring}`;
}

/** Schatten der Karte, die gerade gezogen wird. */
export function liftShadow(scheme: Scheme): string {
  return `0 16px 32px -12px ${PALETTES[scheme].lift}`;
}

/**
 * Zustände, bei deren Erreichen die Fläche einmal aufblüht: die drei, nach denen man
 * hinschauen soll. Nach idle blüht nichts auf (das wäre ein Signal ohne Anlass), nach
 * denkt auch nicht (das atmet ohnehin).
 */
export function bloomsOnEnter(status: Status): boolean {
  return status === "waiting" || status === "done" || status === "lost";
}

/** Was eine Karte vom vorigen Bild weiß, um einen Wechsel zu erkennen. */
export interface StatusStamp {
  status: Status;
  changedAt: number;
}

/**
 * Soll die Karte jetzt aufblühen?
 *
 * Nur bei einem echten Wechsel (neuer Zeitstempel) in einen der Zustände aus
 * `bloomsOnEnter`. Ohne Vorbild (Karte frisch gezeichnet, Panel gerade geöffnet) nie:
 * sonst blitzt beim Aufklappen jede Karte auf, und das sah im Python-Deck nach einem
 * Neuladen aller Agenten aus.
 */
export function shouldBloom(prev: StatusStamp | undefined, next: StatusStamp): boolean {
  if (!prev) return false;
  if (prev.changedAt === next.changedAt) return false;
  return bloomsOnEnter(next.status);
}

/** Fläche des Aufblühens: die Karte färbt sich kurz kräftiger und klingt in ≈ 1 s ab. */
export function bloomFill(status: Status, scheme: Scheme): string {
  return toneOf(status, scheme).pig + PALETTES[scheme].bloom;
}

/** Aussehen des Griffs aus dem dringlichsten Zustand (`null` = alles ruhig). */
interface HandleLook {
  color: string;
  glow: string;
  breathe: boolean;
}

export function handleLook(urgent: Status | null, scheme: Scheme): HandleLook {
  if (!urgent || attentionOf(urgent) === "none") {
    return { color: PALETTES[scheme].handleQuiet, glow: "none", breathe: false };
  }
  const c = toneOf(urgent, scheme).pig;
  return { color: c, glow: `0 0 10px ${c}99`, breathe: attentionOf(urgent) === "strong" };
}

/** Farbe der Modusangabe auf der Karte: Bypass warnt rot, alles andere wie Modell · Effort. */
export function modeColor(mode: string, scheme: Scheme): string {
  const pal = PALETTES[scheme];
  return mode.toLowerCase().startsWith("bypass") ? pal.bypass : pal.meta;
}

/** „Modell · Effort“ oben rechts; leere Teile fallen weg statt einen Punkt zu hinterlassen. */
export function metaLine(model: string, effort: string): string {
  return [model, effort].filter((s) => s.trim() !== "").join(" · ");
}

/** Periode des Atmens in ms (DESIGN.md: 3 s). */
const BREATHE_MS = 3000;

/**
 * Negative Startverzögerung, damit alle atmenden Elemente im selben Takt atmen, egal
 * wann sie erscheinen: die Phase hängt an der Dokument-Uhr, nicht am Einhängen.
 */
export function syncedDelay(nowMs: number, periodMs = BREATHE_MS): string {
  return `-${Math.round(nowMs % periodMs)}ms`;
}
