import { describe, expect, it } from "vitest";
// @ts-expect-error ohne @types/node (wie in vite.config.ts); Vitest läuft in Node.
import { readFileSync } from "node:fs";
import { blockWidth, CARD_GAP, CARD_W, cardHeight, columnsFor, gridVars, placeAt } from "./layout";

// Roh von der Platte: Vitest liefert CSS-Importe (auch ?raw) leer aus.
const styleOf = (name: string): string => readFileSync(new URL(`../styles/${name}`, import.meta.url), "utf8");
const blockCss = styleOf("block.css");
const tileCss = styleOf("tile.css");

describe("Raster des Panels", () => {
  it("ist so breit wie der breiteste Block samt ＋-Karte", () => {
    expect(columnsFor([2, 3])).toBe(3);
    expect(columnsFor([1])).toBe(1);
  });

  it("hat höchstens vier Karten je Reihe", () => {
    expect(columnsFor([6, 4])).toBe(4);
    expect(columnsFor([5])).toBe(4);
  });

  it("hat ohne Blöcke eine Spalte", () => {
    expect(columnsFor([])).toBe(1);
  });

  it("macht Terminal-Karten hoch und VS-Code-Karten flach", () => {
    expect(cardHeight("terminal")).toBe(118);
    expect(cardHeight("vscode")).toBe(58);
  });
});

describe("Kartenmaße im CSS", () => {
  it("kommen als Variablen aus layout.ts", () => {
    expect(gridVars(4, "terminal")).toEqual({ "--cols": "4", "--card-w": "197px", "--card-h": "118px", "--card-gap": "10px" });
    expect(gridVars(2, "vscode")["--card-h"]).toBe("58px");
  });

  it("stehen im Raster nur als diese Variablen, nicht als eigene Zahlen", () => {
    const rule = /\.block-grid\s*\{([^}]*)\}/.exec(blockCss)?.[1] ?? "";
    expect(rule).toContain("gap: var(--card-gap)");
    expect(rule).toContain("grid-template-columns: repeat(var(--cols), var(--card-w))");
    expect(rule).toContain("grid-auto-rows: var(--card-h)");
    for (const css of [blockCss, tileCss]) {
      expect(css).not.toMatch(new RegExp(`\\b(${CARD_W}|${CARD_W + CARD_GAP}|118|58)px`));
    }
  });
});

describe("Fester Platz einer Karte im Raster", () => {
  it("rechnet die Zellnummer in Reihe und Spalte um, ab 1 gezählt", () => {
    const cells = [0, 1, 2, 4, 5];
    expect(placeAt(cells, 0, 4)).toEqual({ gridRow: 1, gridColumn: 1 });
    expect(placeAt(cells, 2, 4)).toEqual({ gridRow: 1, gridColumn: 3 });
    expect(placeAt(cells, 3, 4)).toEqual({ gridRow: 2, gridColumn: 1 });
    expect(placeAt(cells, 4, 4)).toEqual({ gridRow: 2, gridColumn: 2 });
  });

  it("gibt dicht umbrechenden Karten keinen Platz vor", () => {
    expect(placeAt(undefined, 3, 4)).toBeUndefined();
  });
});

describe("Breite eines Terminal-Blocks", () => {
  const tiles = (n: number) => Array.from({ length: n }) as never[];
  const wt = (rows: number[], screenCols: number, n = rows.reduce((a, b) => a + b, 0)) => ({
    rows,
    screenCols,
    tiles: tiles(n),
    connected: true,
  });

  it("nimmt die ＋-Karte mit in die Reihe, solange der Monitor Platz hat", () => {
    expect(blockWidth(wt([2], 4))).toBe(3);
    expect(blockWidth(wt([3], 4))).toBe(4);
    expect(blockWidth(wt([1], 3))).toBe(2);
  });

  it("schiebt die ＋-Karte in die nächste Reihe, wenn die Reihe voll ist", () => {
    expect(blockWidth(wt([4], 4))).toBe(4);
    expect(blockWidth(wt([3], 3))).toBe(3);
    expect(blockWidth(wt([2, 2], 2))).toBe(2);
  });

  it("bleibt ohne bekannte Monitorbreite bei der breitesten Reihe", () => {
    expect(blockWidth(wt([2], 0))).toBe(2);
  });

  it("zählt bei VS Code Karten plus ＋-Karte", () => {
    expect(blockWidth({ rows: [], screenCols: 0, tiles: tiles(2), connected: true })).toBe(3);
  });
});
