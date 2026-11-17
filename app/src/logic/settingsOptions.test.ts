import { describe, expect, it } from "vitest";
import { t } from "./i18n";
import { appearanceOptions, effortOptions, matchOption, modelOptions, modeOptions, onOffOptions } from "./settingsOptions";

describe("Segment zum gespeicherten Wert", () => {
  it("trifft den exakten Wert", () => {
    expect(matchOption(modelOptions(), "fable")).toBe("fable");
    expect(matchOption(effortOptions(), "Ultracode")).toBe("Ultracode");
  });

  it("trifft ein Modell auch ohne Kontext-Zusatz", () => {
    expect(matchOption(modelOptions(), "opus")).toBe("opus[1m]");
  });

  it("zeigt ohne Deck-Modell Standard, denn dann startet der Agent ohne --model", () => {
    expect(matchOption(modelOptions(), "")).toBe("");
    expect(modelOptions("en")[0]).toEqual({ value: "", label: "Default" });
  });

  it("trifft Effort unabhängig von Groß- und Kleinschreibung", () => {
    expect(matchOption(effortOptions(), "ultracode")).toBe("Ultracode");
  });

  it("zeigt einen Schalter als An und Aus", () => {
    expect(onOffOptions("de").map((o) => o.label)).toEqual(["An", "Aus"]);
    expect(onOffOptions("en").map((o) => o.value)).toEqual(["on", "off"]);
  });

  it("bietet Hell/Dunkel in vier Stufen an, Sonnenstand zuletzt", () => {
    expect(appearanceOptions("de").map((o) => o.label)).toEqual(["System", "Hell", "Dunkel", "Sonnenstand"]);
    expect(appearanceOptions("en").map((o) => o.label)).toEqual(["System", "Light", "Dark", "Sunrise/sunset"]);
    expect(matchOption(appearanceOptions("de"), "sun")).toBe("sun");
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
    const keys = [
      "waitingForAgents",
      "usageOpen",
      "newAgentsHint",
      "liveNote",
      "autoCompact",
      "reposUnset",
      "reposMissing",
      "reposNoRepos",
      "jiraPrefix",
      "titanHint",
      "storeUpdateHint",
      "speechmikeVoiceHint",
      "prAutomationHint",
    ] as const;
    for (const k of keys) {
      for (const lang of ["de", "en"] as const) expect(t(lang, k)).not.toMatch(/[—–]/);
    }
  });
});
