// Umsortieren: die Rechnung hinter dem Ziehen, ohne Bildschirm.
//
// Portiert aus deck/domain/ordering.py (Blöcke, variable Höhen) und deck/ui/reorder.py
// (Kachelreihe). Neu gegenüber Python: die Karten stehen in einem Raster mit vier
// Spalten statt in einer Reihe, das Ziel wird darum in zwei Richtungen gesucht.

/** Zeigerweg (px), ab dem aus einem Druck ein Ziehen wird. Darunter bleibt es ein Klick. */
export const DRAG_THRESHOLD = 8;

/** Hat sich der Zeiger weit genug bewegt, um zu ziehen? (Manhattan-Abstand wie in Python.) */
export function passedThreshold(dx: number, dy: number, threshold = DRAG_THRESHOLD): boolean {
  return Math.abs(dx) + Math.abs(dy) >= threshold;
}

/**
 * <item> herausnehmen und an <target> wieder einsetzen. Der Zielindex kommt aus einer
 * Zeigerposition und kennt keine Listengrenzen, darum wird geklemmt.
 */
export function moveTo<T>(order: readonly T[], item: T, target: number): T[] {
  const rest = order.filter((x) => x !== item);
  const t = Math.max(0, Math.min(target, rest.length));
  return [...rest.slice(0, t), item, ...rest.slice(t)];
}

/**
 * Gemerkte Reihenfolge <saved> über die gemeldete Liste <live> legen: was in beiden
 * steht, in der Reihenfolge von <saved>; nur in <live> hinten an; nur in <saved> weg.
 * Dient hier dazu, eine eben gezogene Reihenfolge zu halten, bis Rust sie bestätigt.
 */
export function applyOrder(saved: readonly string[], live: readonly string[]): string[] {
  const liveSet = new Set(live);
  const seen = new Set<string>();
  const out: string[] = [];
  for (const x of saved) {
    if (liveSet.has(x) && !seen.has(x)) {
      out.push(x);
      seen.add(x);
    }
  }
  return [...out, ...live.filter((x) => !seen.has(x))];
}

export function sameOrder(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((x, i) => x === b[i]);
}

// ── Raster (Karten eines Blocks) ─────────────────────────────────────────────

export interface Grid {
  /** Spalten je Reihe. */
  cols: number;
  /** Schritt von Karte zu Karte (Breite + Abstand). */
  stepX: number;
  /** Schritt von Reihe zu Reihe (Höhe + Abstand). */
  stepY: number;
  /**
   * Feste Zelle je Position (Zellnummer = Reihe · cols + Spalte), wenn die Karten nicht
   * dicht umbrechen, sondern wie die Fenster auf dem Bildschirm liegen (siehe
   * `screenCells`). Fehlt sie, gilt der dichte Umbruch.
   */
  cells?: readonly number[];
}

export interface Point {
  x: number;
  y: number;
}

/** Linke obere Ecke der Zelle <index>, relativ zum Rasterursprung. */
export function cellOf(index: number, grid: Grid): Point {
  const cell = grid.cells?.[index] ?? index;
  return {
    x: (cell % grid.cols) * grid.stepX,
    y: Math.floor(cell / grid.cols) * grid.stepY,
  };
}

/**
 * Zellen für Karten, die wie die Fenster auf dem Bildschirm liegen: <rows> sind die
 * Reihenlängen (`[3, 3]` = zwei Reihen zu drei). Jede Reihe beginnt links in einer
 * neuen Rasterreihe; eine Reihe breiter als <cols> bricht um. Was über die Summe
 * hinausgeht (<count> größer), und danach die ＋-Karte, folgen in der nächsten freien
 * Zelle, aber nur bis Spalte <limit> (so viele Fenster passen auf dem Monitor
 * nebeneinander): ist die Reihe dort voll, geht es vorn in der nächsten weiter.
 * Rückgabe: eine Zelle je Karte plus eine für die ＋-Karte.
 */
export function screenCells(rows: readonly number[], count: number, cols: number, limit = cols): number[] {
  const out: number[] = [];
  let line = 0;
  for (const len of rows) {
    for (let i = 0; i < len && out.length < count; i++) out.push(line * cols + i);
    if (len > 0) line += Math.ceil(len / cols);
    if (out.length >= count) break;
  }
  const wrap = Math.max(1, Math.min(cols, limit));
  let next = out.length ? out[out.length - 1] + 1 : 0;
  while (out.length < count + 1) {
    if (next % cols >= wrap) next = (Math.floor(next / cols) + 1) * cols;
    out.push(next++);
  }
  return out;
}

/**
 * Zielzelle (0 .. n-1) der gezogenen Karte: die Zelle, deren Ecke der Ecke der Karte am
 * nächsten liegt. Bezug ist die Karte, nicht der Zeiger, damit der Griff-Versatz (wo
 * man sie angefasst hat) keine Rolle spielt.
 */
export function gridDropIndex(count: number, grid: Grid, dragTopLeft: Point): number {
  if (count <= 1) return 0;
  if (grid.cells) {
    // Feste Zellen: die Position, deren Zelle der Karte am nächsten liegt.
    let best = 0;
    let bestD = Infinity;
    for (let p = 0; p < count; p++) {
      const c = cellOf(p, grid);
      const d = Math.hypot((c.x - dragTopLeft.x) / grid.stepX, (c.y - dragTopLeft.y) / grid.stepY);
      if (d < bestD) {
        best = p;
        bestD = d;
      }
    }
    return best;
  }
  const rows = Math.ceil(count / grid.cols);
  const col = clamp(Math.round(dragTopLeft.x / grid.stepX), 0, grid.cols - 1);
  const row = clamp(Math.round(dragTopLeft.y / grid.stepY), 0, rows - 1);
  return clamp(row * grid.cols + col, 0, count - 1);
}

/**
 * Wohin die NICHT gezogenen Karten gehören: sie rücken zusammen und lassen an <target>
 * genau eine Zelle frei. Rückgabe je Index der Ausgangsreihenfolge die Zielzelle.
 * Mit target == from ergibt das exakt die Ausgangslage.
 */
export function gridReflow(count: number, from: number, target: number, grid: Grid): Map<number, Point> {
  const want = new Map<number, Point>();
  let pos = 0;
  for (let i = 0; i < count; i++) {
    if (i === from) continue;
    if (pos === target) pos++;
    want.set(i, cellOf(pos, grid));
    pos++;
  }
  return want;
}

// ── Stapel mit variablen Höhen (Blöcke untereinander) ─────────────────────────

/**
 * Die Oberkanten, die das gezogene Element an Position 0, 1, … hätte, gerechnet gegen
 * den zusammengerückten Stapel (ohne das gezogene). Diese Kanten stehen fest, während
 * die anderen noch gleiten; darum zittert die Entscheidung nicht.
 */
export function slotTops(heights: readonly number[], gap: number, top: number, from: number): number[] {
  const out: number[] = [];
  let y = top;
  heights.forEach((h, i) => {
    if (i === from) return;
    out.push(y);
    y += h + gap;
  });
  out.push(y);
  return out;
}

/**
 * Zielposition des gezogenen Elements: die Position, deren Oberkante seiner aktuellen
 * am nächsten liegt. Kante statt Mitte, weil ein hoher Block mit der Mitten-Regel
 * Position 0 nie erreichen könnte (Begründung in ordering.py).
 */
export function dropIndex(heights: readonly number[], gap: number, top: number, from: number, dragTop: number): number {
  const tops = slotTops(heights, gap, top, from);
  let best = 0;
  for (let p = 1; p < tops.length; p++) {
    if (Math.abs(tops[p] - dragTop) < Math.abs(tops[best] - dragTop)) best = p;
  }
  return best;
}

/** Ziel-Oberkanten der nicht gezogenen Elemente; an <target> bleibt eine Lücke. */
export function reflowTops(heights: readonly number[], gap: number, top: number, from: number, target: number): Map<number, number> {
  const want = new Map<number, number>();
  let y = top;
  let pos = 0;
  heights.forEach((h, i) => {
    if (i === from) return;
    if (pos === target) {
      y += heights[from] + gap;
      pos++;
    }
    want.set(i, y);
    y += h + gap;
    pos++;
  });
  return want;
}

/**
 * Neue Blockreihenfolge mit dem Heimat-Block vorn. Er steht immer oben, egal wohin
 * gezogen wurde; gezogen wird nur unter ihm.
 */
export function pinFirst(order: readonly string[], item: string | null): string[] {
  if (item === null || !order.includes(item)) return [...order];
  return [item, ...order.filter((x) => x !== item)];
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, v));
}
