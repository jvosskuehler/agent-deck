// Papier-Look (Variante „02 Farbfeld“, gewählt am 09.11.2026): hell Rosé Pine Dawn
// („Ebenen“), dunkel „Graphit“, wie die Terminal-Agenten. Welche Hälfte gilt, entscheidet das
// System (prefers-color-scheme), genau wie beim Windows-Terminal-Profil „Agent Deck“.
//
// Hier stehen nur die Farben, die je Zustand gerechnet werden (Karte, Griff, Punkte der
// Leiste). Alles Feste steht als CSS-Variable in styles/base.css, dieselben Werte.

import type { Status } from "../bindings/Status";

export type Scheme = "light" | "dark";

/**
 * Eine Statusfarbe in drei Stärken: `fill` ist die Kartenfläche (fest gemischt, je Modus
 * gewählt), `pig` (Pigment) färbt Atmen, Aufblühen, Griff und Punkte, `ink` ist der Text
 * in dieser Farbe (Kontrast ≥ 4,5 : 1 auf der Fläche).
 *
 * Variante „01 Klar“ (09.11.2026): bis dahin war die Fläche Pigment mit Alpha über dem
 * Kartengrund. Auf Graphit wurde Gold so braun und Grün oliv; feste Flächen je Modus
 * bleiben klar.
 */
interface Tone {
  fill: string;
  pig: string;
  ink: string;
}

interface Palette {
  /** Grund jeder Karte unter der Tönung. */
  card: string;
  /** Fläche einer ruhigen Karte (idle, none, unknown). */
  idleCard: string;
  /** Hex-Alpha der atmenden Ebene bei Rückfrage und denkt. */
  pulse: string;
  /** Hex-Alpha des einmaligen Aufblühens nach einem Statuswechsel. */
  bloom: string;
  /** Ring der ausgewählten Karte und Panelgrund für den Abstand davor. */
  ring: string;
  panel: string;
  /** Schatten einer gezogenen Karte. */
  lift: string;
  /** Modell, Effort und Modus. */
  meta: string;
  /** Modus Bypass. */
  bypass: string;
  /** Griff, wenn alles ruhig ist. */
  handleQuiet: string;
  /** Punkt der Leiste für einen unbekannten Wert. */
  dotUnknown: string;
  status: Record<"waiting" | "thinking" | "done" | "lost" | "idle", Tone>;
}

export const PALETTES: Record<Scheme, Palette> = {
  // Rosé Pine Dawn als Grund, Statusflächen „01 Klar“: Aprikose, Himmelblau, Mint, Rosé.
  light: {
    card: "#fffaf3",
    idleCard: "#efe7df",
    pulse: "24",
    bloom: "66",
    ring: "#575279",
    panel: "#faf4ed",
    lift: "rgba(87, 82, 121, 0.45)",
    meta: "#575279",
    bypass: "#8f3f58",
    handleQuiet: "#9893a5",
    dotUnknown: "#cecacd",
    status: {
      waiting: { fill: "#fce2cf", pig: "#e8813f", ink: "#a4441c" },
      thinking: { fill: "#dce7f6", pig: "#4f86c6", ink: "#2d5d95" },
      done: { fill: "#d8eedd", pig: "#4fa36a", ink: "#2c7247" },
      lost: { fill: "#f5e0e4", pig: "#c4536f", ink: "#a13d59" },
      idle: { fill: "#efe7df", pig: "#9893a5", ink: "#6e6a86" },
    },
  },
  // „Graphit“ (gewählt am 09.11.2026), dasselbe Dunkel wie das Terminal-Schema
  // papier-ebenen-nacht: warmes Graphit statt Lila, Schrift in Papierfarbe. Auf Dunkel
  // trägt das Pigment selbst den Text.
  dark: {
    card: "#242220",
    idleCard: "#2b2927",
    pulse: "1e",
    bloom: "55",
    ring: "#e6e0d6",
    panel: "#1d1c1b",
    lift: "rgba(0, 0, 0, 0.7)",
    meta: "#cfc9bf",
    bypass: "#f497a8",
    handleQuiet: "#6f6b66",
    dotUnknown: "#4a4744",
    status: {
      waiting: { fill: "#4b2b1f", pig: "#f5a27c", ink: "#f5a27c" },
      thinking: { fill: "#213349", pig: "#8db9ee", ink: "#8db9ee" },
      done: { fill: "#1c392d", pig: "#86d2a3", ink: "#86d2a3" },
      lost: { fill: "#3a2528", pig: "#f08a9e", ink: "#f08a9e" },
      idle: { fill: "#2b2927", pig: "#6f6b66", ink: "#a9a39b" },
    },
  },
};

/** Statusfarbe je Zustand; unbekannt und none sehen aus wie idle. */
export function toneOf(status: Status, scheme: Scheme): Tone {
  const s = PALETTES[scheme].status;
  switch (status) {
    case "waiting":
    case "thinking":
    case "done":
    case "lost":
      return s[status];
    default:
      return s.idle;
  }
}

/**
 * Ampelfarben der Nutzung: Rust schickt sie als feste Werte (deck-core
 * `claude/usage.rs`, GREEN/AMBER/RED/GRAY, für Schwarz gewählt); hier werden sie zu den
 * Tönen der Palette. Eine unbekannte Farbe bleibt, wie sie ist.
 */
export function usageDot(rustColor: string, scheme: Scheme): string {
  const s = PALETTES[scheme].status;
  switch (rustColor.toLowerCase()) {
    case "#6ee7a8":
      return s.done.pig;
    case "#ffc48a":
      return s.waiting.pig;
    case "#ff6b6b":
      return s.lost.pig;
    case "#8b8b99":
      return s.idle.pig;
    default:
      return rustColor;
  }
}
