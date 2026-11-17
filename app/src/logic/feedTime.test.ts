import { describe, expect, it } from "vitest";
import { ago, clock, initialsOf, moreText } from "./feedTime";

describe("Zeiten im Feed", () => {
  it("die Uhrzeit hat immer zwei Stellen je Teil", () => {
    const at = new Date(2026, 10, 17, 9, 5).getTime() / 1000;
    expect(clock(at)).toBe("09:05");
  });

  it("relative Zeit wird mit dem Abstand gröber", () => {
    const now = 1_000_000;
    expect(ago(now - 20, now, "de")).toBe("gerade eben");
    expect(ago(now - 4 * 60, now, "de")).toBe("vor 4 Min.");
    expect(ago(now - 3 * 3600, now, "de")).toBe("vor 3 Std.");
    expect(ago(now - 86400, now, "de")).toBe("vor 1 Tag");
    expect(ago(now - 3 * 86400, now, "en")).toBe("3 days ago");
    expect(ago(now + 30, now, "en")).toBe("just now");
  });

  it("ohne Kürzel aus Rust nimmt der Avatar die ersten Buchstaben des Logins", () => {
    expect(initialsOf("lh", "x")).toBe("LH");
    expect(initialsOf("", "reviewer1")).toBe("RE");
    expect(initialsOf("", "--")).toBe("?");
  });

  it("weitere Kommentare stehen nur da, wenn es welche gibt", () => {
    expect(moreText(0, "de")).toBe("");
    expect(moreText(2, "de")).toBe("und 2 weitere");
    expect(moreText(1, "en")).toBe("and 1 more");
  });

  it("kein Text enthält Gedankenstriche", () => {
    const texts = [ago(0, 99999, "de"), ago(0, 99999, "en"), moreText(3, "de")];
    for (const s of texts) expect(s).not.toMatch(/[—–]/);
  });
});
