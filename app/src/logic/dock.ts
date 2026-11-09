// Andocken: wohin das Panel gleitet, wie lange, welche Ecken rund sind.
//
// Rust entscheidet, ob offen oder zu (Zeigerposition); die Oberfläche animiert nur.
// Ein halb ausgefahrenes Deck ist der eine unzulässige Zustand: das Gleiten ist eine
// CSS-Transition auf ein festes Ziel, sie endet also immer ganz offen oder ganz zu.

export type Edge = "top" | "left" | "right" | "bottom" | "off";
export type Phase = "open" | "closed";

const EDGES: readonly Edge[] = ["top", "left", "right", "bottom", "off"];

export function normalizeEdge(edge: string): Edge {
  return (EDGES as readonly string[]).includes(edge) ? (edge as Edge) : "top";
}

/** Frei schwebend gibt es kein Eingeklappt: dort gilt immer offen. */
export function normalizePhase(edge: Edge, phase: string): Phase {
  if (edge === "off") return "open";
  return phase === "closed" ? "closed" : "open";
}

/** Ausfahren ≈ 220 ms, Einfahren ≈ 180 ms (DESIGN.md). */
export const OPEN_MS = 220;
export const CLOSE_MS = 180;

/**
 * Raum für den weichen Schatten an der freien Seite gegenüber der Kante (px). Er gehört
 * zum Fenster (das Fenster schneidet ab), darum ist der Schatten so bemessen, dass er
 * innerhalb dieses Raums ausläuft: sonst stünde am Fensterrand eine harte Kante.
 */
export const SHADOW_GUTTER = 24;

interface SlideTiming {
  durationMs: number;
  easing: string;
}

/**
 * Raus bremst weich ab (das Panel „landet“), rein beschleunigt (es wird weggezogen).
 * Kein Überschwingen: ein am Rand verankertes Panel, das über sein Ziel hinausschießt,
 * löst sich sichtbar von der Kante.
 */
export function slideTiming(phase: Phase): SlideTiming {
  return phase === "open"
    ? { durationMs: OPEN_MS, easing: "cubic-bezier(0.22, 1, 0.36, 1)" }
    : { durationMs: CLOSE_MS, easing: "cubic-bezier(0.4, 0, 0.9, 0.5)" };
}

/**
 * Transform des Panels: offen an Ort und Stelle, zu ganz hinter der Kante. Der Weg ist
 * die eigene Größe plus der Schattenraum, sonst bliebe der Schatten als dunkler Streifen
 * im Fenster stehen.
 */
export function slideTransform(edge: Edge, phase: Phase): string {
  if (edge === "off" || phase === "open") return "translate(0, 0)";
  const away = `calc(-100% - ${SHADOW_GUTTER}px)`;
  const toward = `calc(100% + ${SHADOW_GUTTER}px)`;
  switch (edge) {
    case "top":
      return `translate(0, ${away})`;
    case "bottom":
      return `translate(0, ${toward})`;
    case "left":
      return `translate(${away}, 0)`;
    case "right":
      return `translate(${toward}, 0)`;
  }
}

/** Radien: nur die freien Ecken sind rund (18 px), die an der Bildschirmkante eckig. */
export function panelRadius(edge: Edge): string {
  const r = "18px";
  switch (edge) {
    case "top":
      return `0 0 ${r} ${r}`;
    case "bottom":
      return `${r} ${r} 0 0`;
    case "left":
      return `0 ${r} ${r} 0`;
    case "right":
      return `${r} 0 0 ${r}`;
    case "off":
      return r;
  }
}

/**
 * Weicher Schatten, der von der Kante weg fällt. Die Vorlage (0 30px 60px -30px) läuft
 * erst nach ≈ 60 px aus; im Fenster wäre das ein breiter, klickfangender Streifen. Halb so
 * groß sieht gleich aus und endet nach zwei Standardabweichungen (24 px) im Schattenraum.
 */
export function panelShadow(edge: Edge): string {
  // Farbe aus base.css: auf Papier ein violettes Grau, auf Dunkel Schwarz.
  const c = "var(--panel-shadow)";
  switch (edge) {
    case "top":
    case "off":
      return `0 12px 24px -12px ${c}`;
    case "bottom":
      return `0 -12px 24px -12px ${c}`;
    case "left":
      return `12px 0 24px -12px ${c}`;
    case "right":
      return `-12px 0 24px -12px ${c}`;
  }
}

type GutterPadding = Partial<Record<"paddingTop" | "paddingBottom" | "paddingLeft" | "paddingRight", number>>;

/** Schattenraum an der Seite gegenüber der Kante, als Innenabstand der Messbox. */
export function gutterStyle(edge: Edge): GutterPadding {
  const side = gutterSide(edge);
  const key = `padding${side[0].toUpperCase()}${side.slice(1)}` as keyof GutterPadding;
  return { [key]: SHADOW_GUTTER };
}

/** Seite, an der das Panel-Fenster Schattenraum braucht (gegenüber der Kante). */
export function gutterSide(edge: Edge): "bottom" | "top" | "right" | "left" {
  switch (edge) {
    case "top":
    case "off":
      return "bottom";
    case "bottom":
      return "top";
    case "left":
      return "right";
    case "right":
      return "left";
  }
}
