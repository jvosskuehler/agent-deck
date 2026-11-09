import { describe, expect, it } from "vitest";
import { PALETTES, toneOf, usageDot } from "./palette";
import {
  bloomFill,
  bloomsOnEnter,
  handleLook,
  metaLine,
  modeColor,
  selectedRing,
  shouldBloom,
  syncedDelay,
  tileLook,
} from "./status";

describe("Statusfarben", () => {
  it("folgen der Tabelle in DESIGN.md („01 Klar“, hell und dunkel)", () => {
    expect(toneOf("waiting", "light")).toEqual({ fill: "#fce2cf", pig: "#e8813f", ink: "#a4441c" });
    expect(toneOf("thinking", "light")).toEqual({ fill: "#dce7f6", pig: "#4f86c6", ink: "#2d5d95" });
    expect(toneOf("done", "light")).toEqual({ fill: "#d8eedd", pig: "#4fa36a", ink: "#2c7247" });
    expect(toneOf("lost", "light")).toEqual({ fill: "#f5e0e4", pig: "#c4536f", ink: "#a13d59" });
    expect(toneOf("idle", "light")).toEqual({ fill: "#efe7df", pig: "#9893a5", ink: "#6e6a86" });
    expect(toneOf("waiting", "dark").fill).toBe("#4b2b1f");
    expect(toneOf("thinking", "dark").fill).toBe("#213349");
    expect(toneOf("done", "dark").fill).toBe("#1c392d");
    expect(toneOf("lost", "dark").fill).toBe("#3a2528");
  });

  it("zeigen none wie idle", () => {
    for (const scheme of ["light", "dark"] as const) {
      expect(toneOf("none", scheme)).toEqual(toneOf("idle", scheme));
    }
  });
});

describe("Kartenfläche", () => {
  it("füllt aktive Zustände mit ihrer festen Fläche statt einer durchscheinenden Tönung", () => {
    expect(tileLook("done", "light").tint).toBe("#d8eedd");
    expect(tileLook("done", "dark").tint).toBe("#1c392d");
  });

  it("legt ruhige Karten auf die Overlay-Fläche statt sie zu tönen", () => {
    for (const s of ["idle", "none"] as const) {
      expect(tileLook(s, "light").tint).toBe(PALETTES.light.idleCard);
      expect(tileLook(s, "dark").tint).toBe(PALETTES.dark.idleCard);
    }
  });

  it("schreibt den Status in der dunklen Ink-Farbe, nicht im Pigment", () => {
    expect(tileLook("waiting", "light").ink).toBe("#a4441c");
  });

  it("lässt nur Rückfrage und denkt in der Fläche atmen", () => {
    for (const s of ["waiting", "thinking"] as const) {
      const look = tileLook(s, "light");
      expect(look.pulse).toBe(`${toneOf(s, "light").pig}24`);
      expect(look.breathe).toBe(true);
    }
    for (const s of ["done", "lost", "idle", "none"] as const) {
      expect(tileLook(s, "light").pulse).toBe("transparent");
      expect(tileLook(s, "light").breathe).toBe(false);
    }
  });

  it("setzt den Ring der Auswahl mit Abstand in Panelfarbe ab", () => {
    expect(selectedRing("light")).toBe("0 0 0 2px #faf4ed, 0 0 0 3.5px #575279");
    expect(selectedRing("dark")).toBe("0 0 0 2px #1d1c1b, 0 0 0 3.5px #e6e0d6");
  });
});

describe("Aufblühen", () => {
  it("geschieht beim Wechsel nach Rückfrage, ungelesen oder getrennt", () => {
    expect(bloomsOnEnter("waiting")).toBe(true);
    expect(bloomsOnEnter("done")).toBe(true);
    expect(bloomsOnEnter("lost")).toBe(true);
  });

  it("geschieht nicht beim Wechsel nach idle oder denkt", () => {
    expect(shouldBloom({ status: "done", changedAt: 1 }, { status: "idle", changedAt: 2 })).toBe(false);
    expect(shouldBloom({ status: "idle", changedAt: 1 }, { status: "thinking", changedAt: 2 })).toBe(false);
  });

  it("braucht einen neuen Zeitstempel, ein gleicher Poll blüht nicht erneut", () => {
    expect(shouldBloom({ status: "waiting", changedAt: 5 }, { status: "waiting", changedAt: 5 })).toBe(false);
    expect(shouldBloom({ status: "thinking", changedAt: 5 }, { status: "waiting", changedAt: 6 })).toBe(true);
  });

  it("geschieht nie bei einer frisch gezeichneten Karte (kein Aufblitzen beim Aufklappen)", () => {
    expect(shouldBloom(undefined, { status: "waiting", changedAt: 6 })).toBe(false);
  });

  it("färbt die Fläche kurz kräftiger als die Tönung", () => {
    expect(bloomFill("lost", "light")).toBe("#c4536f66");
    expect(bloomFill("lost", "dark")).toBe("#f08a9e55");
  });
});

describe("Griff", () => {
  it("ist ruhig grau, wenn nichts dringlich ist", () => {
    expect(handleLook(null, "light")).toEqual({ color: "#9893a5", glow: "none", breathe: false });
    expect(handleLook("idle", "dark").color).toBe("#6f6b66");
  });

  it("trägt das Pigment des dringlichsten Zustands und atmet bei Rückfrage und denkt", () => {
    expect(handleLook("waiting", "light")).toEqual({ color: "#e8813f", glow: "0 0 10px #e8813f99", breathe: true });
    expect(handleLook("thinking", "light").breathe).toBe(true);
    expect(handleLook("done", "light").breathe).toBe(false);
    expect(handleLook("lost", "dark").color).toBe("#f08a9e");
  });
});

describe("Kartentexte", () => {
  it("färben nur Bypass rot", () => {
    expect(modeColor("bypass", "light")).toBe("#8f3f58");
    expect(modeColor("bypassPermissions", "dark")).toBe("#f497a8");
    expect(modeColor("auto", "light")).toBe("#575279");
  });

  it("lassen leere Teile von Modell und Effort ohne Trennpunkt weg", () => {
    expect(metaLine("Opus 5.5", "xhigh")).toBe("Opus 5.5 · xhigh");
    expect(metaLine("Opus 5.5", "")).toBe("Opus 5.5");
    expect(metaLine("", "")).toBe("");
  });
});

describe("Atemtakt", () => {
  it("hängt an der Dokument-Uhr, damit alle Karten im selben Takt atmen", () => {
    expect(syncedDelay(0)).toBe("-0ms");
    expect(syncedDelay(4500)).toBe("-1500ms");
    expect(syncedDelay(7500)).toBe(syncedDelay(1500));
  });
});

describe("Ampel der Leiste", () => {
  it("übersetzt die festen Farben aus usage.rs in die Pigmente der Palette", () => {
    expect(usageDot("#6ee7a8", "light")).toBe("#4fa36a");
    expect(usageDot("#ffc48a", "light")).toBe("#e8813f");
    expect(usageDot("#FF6B6B", "dark")).toBe("#f08a9e");
    expect(usageDot("#8b8b99", "dark")).toBe("#6f6b66");
  });

  it("lässt eine unbekannte Farbe stehen", () => {
    expect(usageDot("#123456", "light")).toBe("#123456");
  });
});
