import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPendingOrder, PENDING_MS } from "./pendingOrder";

const view = (blocks: Record<string, string[]>) => ({
  blocks: Object.entries(blocks).map(([letter, slots]) => ({ letter, tiles: slots.map((slot) => ({ slot })) })),
});

describe("Gehaltene Reihenfolge nach dem Ablegen", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("gilt sofort und bis zum Ende der Frist, wenn Rust nichts meldet", () => {
    const p = createPendingOrder();
    p.holdTiles("A", ["A2", "A1"]);
    p.holdBlocks(["B", "A"]);
    expect(p.get()).toEqual({ tiles: { A: ["A2", "A1"] }, blocks: ["B", "A"] });
    vi.advanceTimersByTime(PENDING_MS - 1);
    expect(p.get().tiles.A).toEqual(["A2", "A1"]);
    expect(p.get().blocks).toEqual(["B", "A"]);
    vi.advanceTimersByTime(1);
    expect(p.get()).toEqual({ tiles: {}, blocks: null });
  });

  it("endet, sobald Rust dieselbe Reihenfolge meldet", () => {
    const p = createPendingOrder();
    p.holdTiles("A", ["A2", "A1"]);
    p.holdBlocks(["B", "A"]);
    p.confirm(view({ B: ["B1"], A: ["A2", "A1"] }));
    expect(p.get()).toEqual({ tiles: {}, blocks: null });
  });

  it("bleibt, solange Rust noch die alte Reihenfolge meldet", () => {
    const p = createPendingOrder();
    p.holdTiles("A", ["A2", "A1"]);
    p.holdBlocks(["B", "A"]);
    p.confirm(view({ A: ["A1", "A2"], B: ["B1"] }));
    expect(p.get()).toEqual({ tiles: { A: ["A2", "A1"] }, blocks: ["B", "A"] });
  });

  it("lässt eine neuere Reihenfolge desselben Blocks nicht mit der Frist der älteren ablaufen", () => {
    const p = createPendingOrder();
    p.holdTiles("A", ["A2", "A1", "A3"]);
    vi.advanceTimersByTime(1000);
    p.holdTiles("A", ["A3", "A2", "A1"]);
    vi.advanceTimersByTime(PENDING_MS - 1000);
    expect(p.get().tiles.A).toEqual(["A3", "A2", "A1"]);
    vi.advanceTimersByTime(1000);
    expect(p.get().tiles.A).toBeUndefined();
  });

  it("hält die Blöcke getrennt voneinander", () => {
    const p = createPendingOrder();
    p.holdTiles("A", ["A2", "A1"]);
    p.holdTiles("B", ["B2", "B1"]);
    p.confirm(view({ A: ["A2", "A1"], B: ["B1", "B2"] }));
    expect(p.get().tiles).toEqual({ B: ["B2", "B1"] });
  });

  it("meldet nur echte Änderungen", () => {
    const p = createPendingOrder();
    const listener = vi.fn();
    p.subscribe(listener);
    p.confirm(view({ A: ["A1"] }));
    expect(listener).not.toHaveBeenCalled();
    p.holdTiles("A", ["A1"]);
    expect(listener).toHaveBeenCalledTimes(1);
    const before = p.get();
    p.confirm(view({ A: ["A2", "A1"] }));
    expect(p.get()).toBe(before);
    expect(listener).toHaveBeenCalledTimes(1);
  });

  it("räumt beim Abbau alle Fristen ab", () => {
    const p = createPendingOrder();
    p.holdTiles("A", ["A2", "A1"]);
    p.dispose();
    vi.advanceTimersByTime(PENDING_MS);
    expect(p.get().tiles.A).toEqual(["A2", "A1"]);
  });
});
