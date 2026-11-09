import { describe, expect, it } from "vitest";
import { t } from "./i18n";
import { effortOptions, matchOption, modelOptions, modeOptions, onOffOptions } from "./settingsOptions";

describe("Segment zum gespeicherten Wert", () => {
  it("trifft den exakten Wert", () => {
    expect(matchOption(modelOptions(), "fable")).toBe("fable");
    expect(matchOption(effortOptions(), "Ultracode")).toBe("Ultracode");
  });

  it("trifft ein Modell auch ohne Kontext-Zusatz", () => {
    expect(matchOption(modelOptions(), "opus")).toBe("opus[1m]");
  });

  it("trifft Effort unabhängig von Groß- und Kleinschreibung", () => {
    expect(matchOption(effortOptions(), "ultracode")).toBe("Ultracode");
  });

  it("zeigt einen Schalter als An und Aus", () => {
    expect(onOffOptions("de").map((o) => o.label)).toEqual(["An", "Aus"]);
    expect(onOffOptions("en").map((o) => o.value)).toEqual(["on", "off"]);
  });

  it("markiert nichts bei einem fremden Wert", () => {
    expect(matchOption(modeOptions("de"), "yolo")).toBeNull();
    expect(matchOption(modeOptions("de"), "")).toBeNull();
  });
});

describe("Texte", () => {
  it("setzen Platzhalter ein", () => {
    expect(t("de", "newAgentIn", { name: "Repos" })).toBe("Neuer Agent in Repos");
    expect(t("en", "newAgentIn", { name: "Repos" })).toBe("New agent in Repos");
  });

  it("enthalten keine Gedankenstriche", () => {
    const keys = ["waitingForAgents", "usageOpen", "newAgentsHint", "liveNote", "autoCompact"] as const;
    for (const k of keys) {
      for (const lang of ["de", "en"] as const) expect(t(lang, k)).not.toMatch(/[—–]/);
    }
  });
});
