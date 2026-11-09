import { describe, expect, it } from "vitest";
import {
  applyOrder,
  cellOf,
  dropIndex,
  gridDropIndex,
  gridReflow,
  moveTo,
  passedThreshold,
  pinFirst,
  reflowTops,
  screenCells,
  slotTops,
} from "./reorder";

const GRID = { cols: 4, stepX: 207, stepY: 128 };

describe("Klick oder Ziehen", () => {
  it("bleibt unter 8 px Zeigerweg ein Klick", () => {
    expect(passedThreshold(3, 4)).toBe(false);
    expect(passedThreshold(5, 3)).toBe(true);
    expect(passedThreshold(-8, 0)).toBe(true);
  });
});

describe("moveTo", () => {
  it("setzt das Element an die Zielposition", () => {
    expect(moveTo(["a", "b", "c", "d"], "a", 2)).toEqual(["b", "c", "a", "d"]);
    expect(moveTo(["a", "b", "c", "d"], "d", 0)).toEqual(["d", "a", "b", "c"]);
  });

  it("klemmt einen Zielindex jenseits der Liste", () => {
    expect(moveTo(["a", "b", "c"], "a", 99)).toEqual(["b", "c", "a"]);
    expect(moveTo(["a", "b", "c"], "c", -3)).toEqual(["c", "a", "b"]);
  });
});

describe("applyOrder", () => {
  it("hält die gezogene Reihenfolge, hängt Neues hinten an und lässt Verschwundenes weg", () => {
    expect(applyOrder(["c", "a", "x"], ["a", "b", "c"])).toEqual(["c", "a", "b"]);
  });

  it("zählt doppelte Einträge nur einmal", () => {
    expect(applyOrder(["a", "a", "b"], ["a", "b"])).toEqual(["a", "b"]);
  });
});

describe("Raster der Karten", () => {
  it("legt vier Karten je Reihe", () => {
    expect(cellOf(0, GRID)).toEqual({ x: 0, y: 0 });
    expect(cellOf(3, GRID)).toEqual({ x: 621, y: 0 });
    expect(cellOf(4, GRID)).toEqual({ x: 0, y: 128 });
  });

  it("findet die Zelle, deren Ecke der gezogenen Karte am nächsten liegt", () => {
    expect(gridDropIndex(5, GRID, { x: 190, y: 10 })).toBe(1);
    expect(gridDropIndex(5, GRID, { x: 0, y: 120 })).toBe(4);
  });

  it("klemmt das Ziel auf die vorhandenen Karten, auch unterhalb der letzten Reihe", () => {
    expect(gridDropIndex(5, GRID, { x: 600, y: 140 })).toBe(4);
    expect(gridDropIndex(5, GRID, { x: -400, y: -300 })).toBe(0);
    expect(gridDropIndex(3, GRID, { x: 900, y: 0 })).toBe(2);
  });

  it("lässt an der Zielzelle eine Lücke und rückt die anderen zusammen", () => {
    // Karte 0 nach Position 2 ziehen: 1 und 2 rücken nach links, 3 bleibt.
    const want = gridReflow(4, 0, 2, GRID);
    expect(want.get(1)).toEqual(cellOf(0, GRID));
    expect(want.get(2)).toEqual(cellOf(1, GRID));
    expect(want.get(3)).toEqual(cellOf(3, GRID));
    expect(want.has(0)).toBe(false);
  });

  it("ergibt ohne Bewegung genau die Ausgangslage", () => {
    const want = gridReflow(6, 4, 4, GRID);
    for (const [i, p] of want) expect(p).toEqual(cellOf(i, GRID));
  });

  it("rückt über den Reihenumbruch hinweg", () => {
    // Karte 4 (zweite Reihe) an Position 0: 0..3 rücken je eine Zelle weiter, 3 in Reihe 2.
    const want = gridReflow(5, 4, 0, GRID);
    expect(want.get(0)).toEqual(cellOf(1, GRID));
    expect(want.get(3)).toEqual(cellOf(4, GRID));
  });
});

describe("Blöcke mit unterschiedlichen Höhen", () => {
  const heights = [100, 40, 100];
  const gap = 18;

  it("rechnet die Positionen gegen den zusammengerückten Stapel", () => {
    expect(slotTops(heights, gap, 0, 0)).toEqual([0, 58, 176]);
  });

  it("erreicht auch für einen hohen Block die oberste Position", () => {
    // Block 2 (hoch) knapp über die Oberkante von Block 0 gezogen.
    expect(dropIndex(heights, gap, 0, 2, 5)).toBe(0);
  });

  it("bleibt am Platz, solange die Kante näher am eigenen Platz liegt", () => {
    expect(dropIndex(heights, gap, 0, 1, 118)).toBe(1);
  });

  it("öffnet die Lücke so hoch wie der gezogene Block", () => {
    const want = reflowTops(heights, gap, 0, 2, 0);
    expect(want.get(0)).toBe(118);
    expect(want.get(1)).toBe(236);
  });

  it("ergibt ohne Bewegung genau den Ausgangsstapel", () => {
    const want = reflowTops(heights, gap, 0, 1, 1);
    expect(want.get(0)).toBe(0);
    expect(want.get(2)).toBe(176);
  });

  it("hält den Heimat-Block immer vorn", () => {
    expect(pinFirst(["A", "C", "B"], "B")).toEqual(["B", "A", "C"]);
    expect(pinFirst(["A", "C"], null)).toEqual(["A", "C"]);
    expect(pinFirst(["A", "C"], "Z")).toEqual(["A", "C"]);
  });
});

describe("Karten wie die Fenster auf dem Bildschirm", () => {
  it("legt 3 + 3 Fenster als zwei Reihen zu drei, auch im Vier-Spalten-Raster", () => {
    // Zellen: Reihe 0 = 0..2, Reihe 1 = 4..6, die ＋-Karte dahinter.
    expect(screenCells([3, 3], 6, 4)).toEqual([0, 1, 2, 4, 5, 6, 7]);
    expect(screenCells([3, 3], 6, 3)).toEqual([0, 1, 2, 3, 4, 5, 6]);
  });

  it("hängt minimierte Fenster und die ＋-Karte dicht an", () => {
    expect(screenCells([4, 3], 8, 4)).toEqual([0, 1, 2, 3, 4, 5, 6, 7, 8]);
    expect(screenCells([2], 3, 4)).toEqual([0, 1, 2, 3]);
  });

  it("stellt die ＋-Karte neben die letzte Karte, solange der Monitor Platz hat", () => {
    // Zwei Fenster, drei passen nebeneinander: ＋ rechts daneben.
    expect(screenCells([2], 2, 3, 3)).toEqual([0, 1, 2]);
    // Drei von drei: ＋ vorn in der nächsten Reihe, auch wenn das Raster breiter ist.
    expect(screenCells([3], 3, 4, 3)).toEqual([0, 1, 2, 4]);
    // 2 + 2 auf einem Drei-Spalten-Monitor: ＋ neben der zweiten Reihe.
    expect(screenCells([2, 2], 4, 3, 3)).toEqual([0, 1, 3, 4, 5]);
  });

  it("bricht eine zu breite Reihe um und beginnt die nächste in neuer Zeile", () => {
    expect(screenCells([5, 1], 6, 4)).toEqual([0, 1, 2, 3, 4, 8, 9]);
  });

  it("rechnet Ziehen und Ausweichen mit den festen Zellen", () => {
    const grid = { ...GRID, cells: screenCells([3, 3], 6, 4) };
    expect(cellOf(3, grid)).toEqual({ x: 0, y: 128 });
    // Über der leeren vierten Spalte, Reihe 2: nächste Karte ist Position 5 (Zelle 6).
    expect(gridDropIndex(6, grid, { x: 621, y: 128 })).toBe(5);
    const want = gridReflow(6, 0, 5, grid);
    expect(want.get(3)).toEqual(cellOf(2, grid));
  });
});
