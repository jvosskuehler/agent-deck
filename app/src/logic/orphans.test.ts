import { describe, expect, it } from "vitest";
import { orphanHint, popoverMaxHeight } from "./orphans";

describe("Läuft noch ohne Fenster", () => {
  it("der Hinweis nennt die Zahl in der Sprache des Decks, ohne Gedankenstrich", () => {
    expect(orphanHint(2, "de")).toBe("2 ohne Fenster");
    expect(orphanHint(1, "en")).toBe("1 without window");
    expect(orphanHint(3, "de")).not.toMatch(/[—–]/);
  });

  it("das Popover bleibt im Panel über der Leiste und wird nie winzig", () => {
    expect(popoverMaxHeight(0, 300)).toBe(290);
    expect(popoverMaxHeight(100, 180)).toBe(120);
  });
});
