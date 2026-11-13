// Rechnen für das Fenster „Glow einstellen“: Farbräume, Vorlagen, die ungefähre Vorschau.
// Was der Rahmen wirklich zeichnet, rechnet der Shader (frame_eigen.hlsl); die Grenzen
// der Werte setzt Rust (deck_core::glow).

import type { GlowAnim } from "../bindings/GlowAnim";
import type { GlowKind } from "../bindings/GlowKind";
import type { GlowStyle } from "../bindings/GlowStyle";
import type { SavedGlow } from "../bindings/SavedGlow";

export interface Hsv {
  h: number; // 0 bis 360
  s: number; // 0 bis 1
  v: number; // 0 bis 1
}

/** `#RRGGBB` → [r, g, b] 0 bis 255; Unlesbares → null. */
export function parseHex(hex: string): [number, number, number] | null {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) return null;
  const n = parseInt(m[1], 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

export function toHex([r, g, b]: [number, number, number]): string {
  const h = (x: number) => Math.round(Math.min(255, Math.max(0, x))).toString(16).padStart(2, "0");
  return `#${h(r)}${h(g)}${h(b)}`.toUpperCase();
}

export function hexToHsv(hex: string): Hsv {
  const rgb = parseHex(hex) ?? [0, 0, 0];
  const [r, g, b] = rgb.map((x) => x / 255);
  const max = Math.max(r, g, b);
  const d = max - Math.min(r, g, b);
  let h = 0;
  if (d > 0) {
    if (max === r) h = ((g - b) / d) % 6;
    else if (max === g) h = (b - r) / d + 2;
    else h = (r - g) / d + 4;
    h *= 60;
    if (h < 0) h += 360;
  }
  return { h, s: max === 0 ? 0 : d / max, v: max };
}

export function hsvToHex({ h, s, v }: Hsv): string {
  const c = v * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = v - c;
  const [r, g, b] =
    h < 60 ? [c, x, 0] : h < 120 ? [x, c, 0] : h < 180 ? [0, c, x] : h < 240 ? [0, x, c] : h < 300 ? [x, 0, c] : [c, 0, x];
  return toHex([(r + m) * 255, (g + m) * 255, (b + m) * 255]);
}

/** Eine Farbvorlage: sie setzt nur die Farben, Glow-Art und Regler bleiben. */
export interface Preset {
  name: string;
  color: string;
  outer: string;
}

/** Die Glow-Arten und Animationen in der Reihenfolge von `deck_core::glow`. */
export const KINDS: GlowKind[] = ["weichlicht", "glutsaum", "zwielicht", "bodenlicht", "ecklicht"];
export const ANIMS: GlowAnim[] = ["still", "atmen", "umlauf", "stroemung", "farbwandern", "funken"];

/** Vorgaben wie `deck_core::glow`. */
export const DARK_DEFAULT: GlowStyle = {
  kind: "weichlicht",
  animation: "still",
  color: "#F5B847",
  outer: "#F08A3C",
  intensity: 1,
  width: 1,
  speed: 1,
};
export const LIGHT_DEFAULT: GlowStyle = { ...DARK_DEFAULT, color: "#D9775A", outer: "", intensity: 1.2 };

/** Hell: die Farben der Light-Mode-Probe (Canvas „Fokus-Rahmen Light Mode“). */
export const LIGHT_PRESETS: Preset[] = [
  { name: "Terrakotta", color: "#D9775A", outer: "" },
  { name: "Gold", color: "#EA9D34", outer: "" },
  { name: "Rosé", color: "#D7827E", outer: "#B4637A" },
  { name: "Beerenrot", color: "#B4637A", outer: "" },
  { name: "Pfirsich", color: "#F0956B", outer: "#D7827E" },
  { name: "Violett", color: "#907AA9", outer: "" },
  { name: "Türkis", color: "#56949F", outer: "#286983" },
  { name: "Tinte", color: "#575279", outer: "" },
];

/** Dunkel: Honiggold (die Vorgabe) und ein paar warme und kühle Gegenstücke. */
export const DARK_PRESETS: Preset[] = [
  { name: "Honiggold", color: "#F5B847", outer: "#F08A3C" },
  { name: "Abendrot", color: "#F5B847", outer: "#F0567A" },
  { name: "Clay", color: "#D97757", outer: "#C4513A" },
  { name: "Kupfer", color: "#DB6E40", outer: "#B4432A" },
  { name: "Rosé", color: "#EBBCBA", outer: "#E0708A" },
  { name: "Iris", color: "#C4A7E7", outer: "#8A7BE0" },
  { name: "Foam", color: "#9CCFD8", outer: "#4FA3C4" },
];

/** So viele gemerkte Glows je Seite, wie `deck_core::glow::SAVED_MAX`. */
export const SAVED_MAX = 24;

/** Gleicher Glow? (Zum Hervorheben des gemerkten, der gerade eingestellt ist.) */
export function sameStyle(a: GlowStyle, b: GlowStyle): boolean {
  return (
    a.kind === b.kind &&
    a.animation === b.animation &&
    a.color.toUpperCase() === b.color.toUpperCase() &&
    a.outer.toUpperCase() === b.outer.toUpperCase() &&
    a.intensity === b.intensity &&
    a.width === b.width &&
    a.speed === b.speed
  );
}

/** Den Glow vorn in die Liste merken; ältere über der Grenze fallen hinten weg. */
export function remember(list: SavedGlow[], style: GlowStyle, name: string): SavedGlow[] {
  return [{ name, style: { ...style } }, ...list].slice(0, SAVED_MAX);
}

/** Datum und Uhrzeit für den Namen eines gemerkten Glows, z. B. „12.11. 14:05“. */
export function stamp(d: Date): string {
  const two = (n: number) => String(n).padStart(2, "0");
  return `${two(d.getDate())}.${two(d.getMonth() + 1)}. ${two(d.getHours())}:${two(d.getMinutes())}`;
}

function alpha(hex: string, a: number): string {
  const [r, g, b] = parseHex(hex) ?? [0, 0, 0];
  return `rgba(${r}, ${g}, ${b}, ${Math.min(1, Math.max(0, a)).toFixed(3)})`;
}

/** Ungefähr der Shader als CSS-Schatten: eng, mittel, weit. Glow-Art und Animation zeigt
 * nur der echte Rahmen. */
export function previewShadow(style: GlowStyle): string {
  const outer = parseHex(style.outer) ? style.outer : style.color;
  const r = style.width;
  const i = 1 - Math.exp(-style.intensity);
  return [
    `0 0 ${Math.round(8 * r)}px ${alpha(style.color, i * 0.95)}`,
    `0 0 ${Math.round(20 * r)}px ${Math.round(2 * r)}px ${alpha(style.color, i * 0.6)}`,
    `0 0 ${Math.round(44 * r)}px ${Math.round(4 * r)}px ${alpha(outer, i * 0.45)}`,
  ].join(", ");
}
