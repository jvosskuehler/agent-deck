import { describe, expect, it } from "vitest";
import {
  CLOSE_MS,
  gutterSide,
  normalizeEdge,
  normalizePhase,
  OPEN_MS,
  panelRadius,
  panelShadow,
  SHADOW_GUTTER,
  slideTiming,
  slideTransform,
} from "./dock";

describe("Andocken", () => {
  it("fährt in 220 ms aus und in 180 ms ein", () => {
    expect(OPEN_MS).toBe(220);
    expect(CLOSE_MS).toBe(180);
    expect(slideTiming("open").durationMs).toBe(220);
    expect(slideTiming("closed").durationMs).toBe(180);
  });

  it("schiebt das Panel samt Schatten ganz hinter die Kante", () => {
    expect(slideTransform("top", "closed")).toBe("translate(0, calc(-100% - 24px))");
    expect(slideTransform("bottom", "closed")).toBe("translate(0, calc(100% + 24px))");
    expect(slideTransform("left", "closed")).toBe("translate(calc(-100% - 24px), 0)");
    expect(slideTransform("right", "closed")).toBe("translate(calc(100% + 24px), 0)");
  });

  it("steht offen an Ort und Stelle", () => {
    for (const e of ["top", "left", "right", "bottom"] as const) {
      expect(slideTransform(e, "open")).toBe("translate(0, 0)");
    }
  });

  it("klappt frei schwebend nie ein", () => {
    expect(normalizePhase("off", "closed")).toBe("open");
    expect(slideTransform("off", "closed")).toBe("translate(0, 0)");
  });

  it("liest unbekannte Werte als oben bzw. offen", () => {
    expect(normalizeEdge("diagonal")).toBe("top");
    expect(normalizePhase("top", "halb")).toBe("open");
  });

  it("rundet nur die freien Ecken", () => {
    expect(panelRadius("top")).toBe("0 0 18px 18px");
    expect(panelRadius("left")).toBe("0 18px 18px 0");
    expect(panelRadius("right")).toBe("18px 0 0 18px");
    expect(panelRadius("bottom")).toBe("18px 18px 0 0");
    expect(panelRadius("off")).toBe("18px");
  });

  it("lässt den Schatten innerhalb des Schattenraums auslaufen (keine harte Kante am Fenster)", () => {
    // Versatz und negative Ausdehnung heben sich auf, die Schattenkante liegt an der
    // Panelkante; die Unschärfe (zwei Standardabweichungen) muss in den Raum passen.
    const [, offset, blur, spread] = /0 (\d+)px (\d+)px (-?\d+)px/.exec(panelShadow("top"))!.map(Number);
    expect(offset + spread).toBe(0);
    expect(blur).toBeLessThanOrEqual(SHADOW_GUTTER);
  });

  it("lässt den Schattenraum gegenüber der Kante", () => {
    expect(gutterSide("top")).toBe("bottom");
    expect(gutterSide("left")).toBe("right");
    expect(gutterSide("right")).toBe("left");
    expect(gutterSide("bottom")).toBe("top");
  });
});
