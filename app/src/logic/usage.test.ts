import { describe, expect, it } from "vitest";
import { PALETTES, usageDot } from "./palette";
import { dotColor, percentText } from "./usage";

describe("Nutzung in der Leiste", () => {
  it("rundet die Auslastung auf ganze Prozent", () => {
    expect(percentText({ percent: 34.4 })).toBe("34 %");
    expect(percentText({ percent: 60.5 })).toBe("61 %");
    expect(percentText({ percent: 0 })).toBe("0 %");
  });

  it("zeigt ein Limit hinter seinem Reset als Strich statt als Zahl", () => {
    expect(percentText({ percent: null })).toBe("—");
  });

  it("färbt den Punkt mit der Ampelfarbe aus Rust, übersetzt in die Palette", () => {
    const limit = { percent: 34, color: "#6ee7a8" };
    expect(dotColor(limit, "light")).toBe(usageDot("#6ee7a8", "light"));
    expect(dotColor(limit, "dark")).toBe(PALETTES.dark.status.done.pig);
  });

  it("macht den Punkt eines unbekannten Limits leise, egal welche Farbe Rust schickt", () => {
    expect(dotColor({ percent: null, color: "#ff6b6b" }, "light")).toBe(PALETTES.light.dotUnknown);
    expect(dotColor({ percent: null, color: "#ff6b6b" }, "dark")).toBe(PALETTES.dark.dotUnknown);
  });
});
