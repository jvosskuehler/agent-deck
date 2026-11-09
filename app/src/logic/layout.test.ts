import { describe, expect, it } from "vitest";
import { blockWidth, cardHeight, columnsFor } from "./layout";

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
