// Nutzung in der unteren Leiste: wie ein Limit als Zahl und als Ampelpunkt erscheint.

import type { LimitView } from "../bindings/LimitView";
import { PALETTES, usageDot, type Scheme } from "./palette";

/** „34 %“; ein Limit hinter seinem Reset ist keine Zahl mehr (CLAUDE.md, Fallen): „—“. */
export function percentText(limit: Pick<LimitView, "percent">): string {
  return limit.percent === null ? "—" : `${Math.round(limit.percent)} %`;
}

/** Ampelpunkt eines Limits; unbekannt = leiser Punkt. */
export function dotColor(limit: Pick<LimitView, "percent" | "color">, scheme: Scheme): string {
  return limit.percent === null ? PALETTES[scheme].dotUnknown : usageDot(limit.color, scheme);
}
