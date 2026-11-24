import { describe, expect, it } from "vitest";
import { mockCommand, registerMockCommands, registerSettingsSection, registerView, settingsSection, viewFor } from "./extensions";

describe("Erweiterungen", () => {
  it("sind ohne Anmeldung nicht da", () => {
    expect(viewFor("info")).toBeUndefined();
    expect(settingsSection("info")).toBeUndefined();
    expect(mockCommand("get_info")).toBeUndefined();
  });

  it("finden angemeldete Ansichten, Abschnitte und Mock-Befehle unter ihrem Namen", () => {
    const View = () => null;
    registerView("info", View);
    registerSettingsSection("info", { advancedOnly: true, render: () => null });
    registerMockCommands({ get_info: () => 42 });
    expect(viewFor("info")).toBe(View);
    expect(settingsSection("info")?.advancedOnly).toBe(true);
    expect(mockCommand("get_info")?.({})).toBe(42);
    expect(viewFor("panel")).toBeUndefined();
  });
});
