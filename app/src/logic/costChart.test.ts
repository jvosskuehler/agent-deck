import { describe, expect, it } from "vitest";
import type { CostView } from "../bindings/CostView";
import { bandPath, chartModel, colorOf, fmtCost, MORE, niceMax, smooth, stack } from "./costChart";

const mix = (v: number) => ({ input: 0, output: v * 0.2, write: v * 0.3, read: v * 0.5 });

function view(): CostView {
  const starts = [0, 3600, 7200].map((s) => 1_794_783_600 + s);
  const series = [
    { ticket: "ORPH-1", values: [10, 20, 30], mix: mix(60) },
    { ticket: "ORPH-2", values: [5, 5, 5], mix: mix(15) },
    { ticket: "", values: [1, 1, 1], mix: mix(3) },
  ];
  return {
    ranges: [{ key: "d1", starts, stepS: 3600, series, prev: 39, models: [{ model: "claude-opus-5-5", value: 78 }] }],
    colored: ["ORPH-1"],
    firstAt: starts[0],
    stand: starts[2],
  };
}

describe("Kostengraph", () => {
  it("stapelt Farb-Tickets, dann weitere, dann ohne Ticket", () => {
    const m = chartModel(view(), "d1", null, "de", false)!;
    expect(m.bands.map((b) => b.key)).toEqual(["ORPH-1", MORE, ""]);
    expect(m.bands[1].values).toEqual([5, 5, 5]);
    expect(m.total).toBe(78);
    expect(m.max).toBe(40);
    expect(m.delta).toBe("+100 % ggü. den 24 Std. davor");
  });

  it("zeigt im Fokus nur das eine Ticket im eigenen Maßstab", () => {
    const m = chartModel(view(), "d1", "ORPH-2", "de", false)!;
    expect(m.bands).toHaveLength(1);
    expect(m.max).toBe(5);
    expect(m.shown).toBe(15);
    expect(m.delta).toBe("");
    expect(m.headline).toContain("ORPH-2, 19 %");
  });

  it("behält die Farbe eines Tickets unabhängig vom Rang", () => {
    expect(colorOf(view(), "ORPH-1")).toBe("var(--c1)");
    expect(colorOf(view(), "ORPH-2")).toBe("var(--c-more)");
    expect(colorOf(view(), "")).toBe("var(--c-none)");
  });

  it("schreibt Zahlen kurz und ohne Gedankenstriche", () => {
    expect(fmtCost(642_900_000, "de")).toBe("642,9 Mio.");
    expect(fmtCost(1_230_000_000, "de")).toBe("1,23 Mrd.");
    expect(fmtCost(12_400, "en")).toBe("12k");
    expect(niceMax(37)).toBe(40);
    expect(chartModel(view(), "d1", null, "de")!.headline).not.toMatch(/[–—]/);
  });

  it("schließt jede Fläche von Rand zu Rand", () => {
    const layers = stack(chartModel(view(), "d1", null, "de")!.bands);
    const d = bandPath(layers[0].top, layers[0].bottom, 40, 300);
    expect(d.startsWith("M0,")).toBe(true);
    expect(d).toContain("L1000,");
    expect(d.endsWith("Z")).toBe(true);
  });

  it("glättet nur die Zeichnung, Tooltip und Summen bleiben echt", () => {
    const m = chartModel(view(), "d1", "ORPH-1", "de")!;
    expect(m.bands[0].values).toEqual([10, 20, 30]);
    expect(m.shown).toBe(60);
    const d = m.bands[0].drawn;
    expect(d[0]).toBeGreaterThan(10);
    expect(d[2]).toBeLessThan(30);
  });

  it("verschmiert eine Spitze, ohne Werte zu erfinden", () => {
    const s = smooth([...new Array(8).fill(0), 90, ...new Array(8).fill(0)], 1.5);
    expect(s[8]).toBeLessThan(90);
    expect(s[7]).toBeGreaterThan(0);
    expect(s.reduce((a, b) => a + b, 0)).toBeCloseTo(90, 0);
    expect(smooth([1, 2, 3], 0)).toEqual([1, 2, 3]);
    expect(smooth([5, 5, 5, 5], 1.5).every((v) => Math.abs(v - 5) < 1e-9)).toBe(true);
  });
});
