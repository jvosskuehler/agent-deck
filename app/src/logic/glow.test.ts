import { describe, expect, it } from "vitest";
import { ANIMS, DARK_PRESETS, KINDS, LIGHT_DEFAULT, SAVED_MAX, hexToHsv, hsvToHex, parseHex, previewShadow, remember, sameStyle, stamp } from "./glow";

describe("Glow-Farben", () => {
  it("hex und hsv gehen ohne Verlust hin und zurück", () => {
    for (const hex of ["#D9775A", "#F5B847", "#286983", "#000000", "#FFFFFF", "#575279"]) {
      expect(hsvToHex(hexToHsv(hex))).toBe(hex);
    }
  });

  it("unlesbare Farben werden nicht geraten", () => {
    expect(parseHex("#F00")).toBeNull();
    expect(parseHex("lila")).toBeNull();
    expect(parseHex("00ff00")).toEqual([0, 255, 0]);
  });

  it("die Vorschau zeichnet drei Schichten in der Farbe der Vorlage", () => {
    const s = previewShadow(LIGHT_DEFAULT);
    expect(s.split("rgba").length - 1).toBe(3);
    expect(s).toContain("rgba(217, 119, 90");
  });

  it("bietet fünf Glow-Arten und sechs Animationen samt Still an", () => {
    expect(KINDS).toHaveLength(5);
    expect(ANIMS).toHaveLength(6);
    expect(ANIMS[0]).toBe("still");
  });

  it("ein gemerkter Glow kommt vorn in die Liste, über der Grenze fällt der älteste weg", () => {
    let list = remember([], LIGHT_DEFAULT, "erster");
    for (let i = 0; i < SAVED_MAX; i++) list = remember(list, { ...LIGHT_DEFAULT, width: 0.5 }, `n${i}`);
    expect(list).toHaveLength(SAVED_MAX);
    expect(list[0].name).toBe(`n${SAVED_MAX - 1}`);
    expect(list.some((s) => s.name === "erster")).toBe(false);
  });

  it("der gemerkte Glow ist eine Kopie, späteres Verstellen ändert ihn nicht", () => {
    const style = { ...LIGHT_DEFAULT };
    const list = remember([], style, "a");
    style.width = 1.5;
    expect(list[0].style.width).toBe(LIGHT_DEFAULT.width);
    expect(sameStyle(list[0].style, LIGHT_DEFAULT)).toBe(true);
    expect(sameStyle(list[0].style, style)).toBe(false);
  });

  it("der Name trägt Tag und Uhrzeit", () => {
    expect(stamp(new Date(2026, 10, 12, 9, 5))).toBe("12.11. 09:05");
  });

  it("jede Vorlage hat lesbare Farben", () => {
    for (const p of DARK_PRESETS) {
      expect(parseHex(p.color)).not.toBeNull();
      expect(p.outer === "" || parseHex(p.outer) !== null).toBe(true);
    }
  });
});
