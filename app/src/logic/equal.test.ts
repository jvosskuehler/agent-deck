import { describe, expect, it } from "vitest";
import { propsEqual } from "./equal";

describe("Props-Vergleich für memo", () => {
  const onClick = () => undefined;

  it("hält eine Karte für gleich, wenn der Poll ein neues, inhaltsgleiches Objekt schickt", () => {
    expect(propsEqual({ tile: { slot: "A1", status: "idle" }, lifted: false }, { tile: { slot: "A1", status: "idle" }, lifted: false })).toBe(true);
  });

  it("sieht eine geänderte Karte", () => {
    expect(propsEqual({ tile: { slot: "A1", status: "idle" } }, { tile: { slot: "A1", status: "thinking" } })).toBe(false);
  });

  it("vergleicht Verschiebungen nach Wert, null nur mit null", () => {
    expect(propsEqual({ offset: { x: 1, y: 2 } }, { offset: { x: 1, y: 2 } })).toBe(true);
    expect(propsEqual({ offset: { x: 1, y: 2 } }, { offset: { x: 1, y: 3 } })).toBe(false);
    expect(propsEqual({ offset: null }, { offset: { x: 0, y: 0 } })).toBe(false);
    expect(propsEqual({ offset: null }, { offset: null })).toBe(true);
  });

  it("vergleicht Handler nach Identität", () => {
    expect(propsEqual({ handlers: { onClick } }, { handlers: { onClick } })).toBe(true);
    expect(propsEqual({ handlers: { onClick } }, { handlers: { onClick: () => undefined } })).toBe(false);
  });

  it("schaut nur eine Ebene tief", () => {
    expect(propsEqual({ a: { b: { c: 1 } } }, { a: { b: { c: 1 } } })).toBe(false);
  });

  it("sieht fehlende und zusätzliche Props", () => {
    expect(propsEqual<Record<string, unknown>>({ place: undefined }, {})).toBe(false);
    expect(propsEqual<Record<string, unknown>>({ a: { x: 1 } }, { a: { x: 1, y: 2 } })).toBe(false);
  });
});
