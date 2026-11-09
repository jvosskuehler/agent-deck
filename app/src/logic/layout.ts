// Maße des Panels (DESIGN.md, Abschnitt „Karte“) und die Spaltenzahl.

import type { BlockView } from "../bindings/BlockView";
import { screenCells } from "./reorder";

// Die einzige Quelle der Kartenmaße: das CSS bekommt sie als Variablen (gridVars), es
// trägt keine eigenen Zahlen dafür.
export const CARD_W = 197;
export const CARD_GAP = 10;
export const CARD_H = 118;
const MAX_COLS = 4;

/**
 * CSS-Variablen für das Kartenraster eines Blocks (`.block-grid` in block.css): Spalten,
 * Kartenbreite, Kartenhöhe und Abstand.
 */
export function gridVars(cols: number): Record<"--cols" | "--card-w" | "--card-h" | "--card-gap", string> {
  return {
    "--cols": String(cols),
    "--card-w": `${CARD_W}px`,
    "--card-h": `${CARD_H}px`,
    "--card-gap": `${CARD_GAP}px`,
  };
}

/** Fester Platz im Raster (CSS-Grid, ab 1 gezählt) für Zellnummer = Reihe · cols + Spalte. */
export interface GridPlace {
  gridRow: number;
  gridColumn: number;
}

/**
 * Platz der Karte an Position <index>, wenn die Karten feste Zellen haben (siehe
 * `screenCells`); ohne Zellen brechen sie dicht um und brauchen keinen.
 */
export function placeAt(cells: readonly number[] | undefined, index: number, cols: number): GridPlace | undefined {
  if (!cells) return undefined;
  return { gridRow: Math.floor(cells[index] / cols) + 1, gridColumn: (cells[index] % cols) + 1 };
}

/**
 * Spalten des Rasters, für alle Blöcke gleich, damit die Karten untereinander fluchten.
 * <cells> zählt je Block die Zellen, die er belegt (Karten plus ＋-Karte, sofern er eine
 * hat). Das Panel ist so breit wie der breiteste Block und nie breiter als vier Karten:
 * danach bricht die Reihe um.
 */
export function columnsFor(cells: readonly number[]): number {
  const widest = cells.reduce((m, n) => Math.max(m, n), 1);
  return Math.min(MAX_COLS, Math.max(1, widest));
}

/**
 * Wie viele Karten nebeneinander dieser Block braucht, samt ＋-Karte.
 *
 * Liegen die Karten wie die Fenster (Fensterlage bekannt), zählt die breiteste Fensterreihe
 * und dazu die ＋-Karte im nächsten freien Feld: sie steht neben der letzten Karte, wenn
 * auf dem Monitor dort noch ein Fenster Platz hätte (<screenCols>), sonst vorn in der
 * nächsten Reihe. Sonst: alle Karten plus ＋-Karte (die Spaltengrenze setzt
 * [columnsFor]).
 */
export function blockWidth(block: Pick<BlockView, "rows" | "screenCols" | "tiles" | "connected">): number {
  const add = block.connected ? 1 : 0;
  if (!block.rows.length) return block.tiles.length + add;
  const widest = Math.max(...block.rows);
  const w = Math.max(widest, block.screenCols || widest);
  const cells = screenCells(block.rows, block.tiles.length, w, w);
  const used = add ? cells : cells.slice(0, -1);
  return used.reduce((m, c) => Math.max(m, (c % w) + 1), 1);
}

/**
 * Bis zu welcher Spalte die Karten eines Blocks hinter seinen Fensterreihen weiterlaufen
 * (siehe `screenCells`): so viele, wie auf seinem Monitor nebeneinander passen, nie
 * weniger als seine breiteste Reihe. Unbekannt: das ganze Raster.
 */
export function wrapLimit(block: Pick<BlockView, "rows" | "screenCols">, cols: number): number {
  if (!block.screenCols) return cols;
  return Math.max(block.screenCols, ...block.rows);
}
