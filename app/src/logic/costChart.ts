// Rechnung des Kostengraphen im Info-Fenster: Stapel je Ticket, Maßstab, Achse,
// Rangliste und Tooltip aus `CostView` (Rust, deck_core::dash::cost_ledger).
//
// Einheit: Kosten in Input-Tokens von Opus 5.5 (4 $ je Mio.). Die acht Farb-Tickets
// (`colored`) behalten ihre Farbe in jedem Zeitraum; der Rest steht grau als „weitere
// Tickets“, die Antworten ohne sichere Zuordnung als „ohne Ticket“.

import type { CostMix } from "../bindings/CostMix";
import type { CostRange } from "../bindings/CostRange";
import type { CostView } from "../bindings/CostView";
import type { Lang } from "./i18n";

export type RangeKey = "h1" | "d1" | "w1" | "m1";
export const RANGE_KEYS: RangeKey[] = ["h1", "d1", "w1", "m1"];

/** Fokus der Ansicht: ein Ticket, `*` = weitere Tickets, `""` = ohne Ticket, null = alle. */
export type Focus = string | null;
export const MORE = "*";

/** CSS-Farbe eines Stapels: `--c1` … `--c8`, grau für weitere und ohne. */
export function colorOf(view: CostView, key: string): string {
  if (key === "") return "var(--c-none)";
  const i = view.colored.indexOf(key);
  return i >= 0 ? `var(--c${i + 1})` : "var(--c-more)";
}

const t = (lang: Lang, de: string, en: string) => (lang === "en" ? en : de);

export function rangeLabel(key: RangeKey, lang: Lang): string {
  return { h1: t(lang, "1 Std.", "1 h"), d1: t(lang, "24 Std.", "24 h"), w1: t(lang, "7 Tage", "7 days"), m1: t(lang, "30 Tage", "30 days") }[key];
}

function rangeLong(key: RangeKey, lang: Lang): string {
  return {
    h1: t(lang, "in der letzten Stunde", "in the last hour"),
    d1: t(lang, "in den letzten 24 Std.", "in the last 24 h"),
    w1: t(lang, "in den letzten 7 Tagen", "in the last 7 days"),
    m1: t(lang, "in den letzten 30 Tagen", "in the last 30 days"),
  }[key];
}

/** 1 234 567 → „1,2 Mio.“; 12 345 → „12 Tsd.“ (en: „1.2M“, „12k“). */
export function fmtCost(v: number, lang: Lang): string {
  const one = (x: number) => (lang === "en" ? x.toFixed(1) : x.toFixed(1).replace(".", ","));
  if (v >= 1e9) return lang === "en" ? `${(v / 1e9).toFixed(2)}B` : `${(v / 1e9).toFixed(2).replace(".", ",")} Mrd.`;
  if (v >= 1e6) return lang === "en" ? `${one(v / 1e6)}M` : `${one(v / 1e6)} Mio.`;
  if (v >= 1e3) return lang === "en" ? `${Math.round(v / 1e3)}k` : `${Math.round(v / 1e3)} Tsd.`;
  return v > 0 ? t(lang, "unter 1 Tsd.", "under 1k") : "0";
}

function fmtAxis(v: number): string {
  const short = (x: number) => String(+x.toFixed(1)).replace(".", ",");
  if (v >= 1e9) return `${short(v / 1e9)} G`;
  if (v >= 1e6) return `${short(v / 1e6)} M`;
  if (v >= 1e3) return `${Math.round(v / 1e3)} k`;
  return "0";
}

/** Nächste runde Zahl ab `v` für den oberen Rand der Achse. */
export function niceMax(v: number): number {
  if (v <= 0) return 1;
  const p = Math.pow(10, Math.floor(Math.log10(v)));
  for (const m of [1, 1.2, 1.5, 2, 2.5, 3, 4, 5, 6, 8, 10]) if (m * p >= v) return m * p;
  return 10 * p;
}

const pad2 = (n: number) => String(n).padStart(2, "0");
const DOW = { de: ["So", "Mo", "Di", "Mi", "Do", "Fr", "Sa"], en: ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"] };

/** Beschriftung eines Eimers im Tooltip („Do 12.11. 18 bis 24 Uhr“). */
export function bucketLabel(key: RangeKey, start: number, stepS: number, lang: Lang): string {
  const d = new Date(start * 1000);
  const e = new Date((start + stepS) * 1000);
  const dow = DOW[lang][d.getDay()];
  const until = t(lang, "bis", "to");
  const date = `${d.getDate()}.${d.getMonth() + 1}.`;
  switch (key) {
    case "h1":
      return `${pad2(d.getHours())}:${pad2(d.getMinutes())} ${until} ${pad2(e.getHours())}:${pad2(e.getMinutes())}`;
    case "d1":
      return `${dow} ${pad2(d.getHours())}:${pad2(d.getMinutes())} ${until} ${pad2(e.getHours() || (e.getMinutes() ? 0 : 24))}:${pad2(e.getMinutes())}`;
    case "w1":
    case "m1":
      return `${dow} ${date} ${pad2(d.getHours())} ${until} ${pad2(e.getHours() || 24)}${t(lang, " Uhr", ":00")}`;
  }
}

/** Achsenbeschriftung (nur jeder n-te Eimer), `null` = keine. */
function tickLabel(key: RangeKey, start: number, lang: Lang): string | null {
  const d = new Date(start * 1000);
  switch (key) {
    case "h1":
      return d.getMinutes() % 10 === 0 ? `${pad2(d.getHours())}:${pad2(d.getMinutes())}` : null;
    case "d1":
      return d.getHours() % 4 === 0 && d.getMinutes() === 0 ? `${pad2(d.getHours())}${t(lang, " Uhr", ":00")}` : null;
    case "w1":
      return d.getHours() === 0 ? `${DOW[lang][d.getDay()]} ${d.getDate()}.` : null;
    case "m1":
      return d.getDay() === 1 && d.getHours() === 0 ? `${d.getDate()}.${d.getMonth() + 1}.` : null;
  }
}

export interface Band {
  key: string;
  name: string;
  color: string;
  /** Echte Werte je Eimer (Tooltip, Summen). */
  values: number[];
  /** Gezeichnete Werte, geglättet. */
  drawn: number[];
}

/** Breite der Glättung je Zeitraum in Eimern (Standardabweichung des Gauß-Kerns). */
const SIGMA: Record<RangeKey, number> = { h1: 1.5, d1: 1.5, w1: 1.5, m1: 1 };

/**
 * Gauß-Glättung über die Eimer. An den Rändern wird der Kern abgeschnitten und neu
 * gewichtet, damit der jüngste Eimer nicht zur Null hin abfällt. `sigma` 0 = unverändert.
 */
export function smooth(values: number[], sigma: number): number[] {
  if (sigma <= 0) return values.slice();
  const r = Math.ceil(sigma * 3);
  const k = Array.from({ length: 2 * r + 1 }, (_, j) => Math.exp(-((j - r) ** 2) / (2 * sigma * sigma)));
  return values.map((_, i) => {
    let acc = 0;
    let w = 0;
    for (let j = -r; j <= r; j++) {
      const v = values[i + j];
      if (v === undefined) continue;
      acc += v * k[j + r];
      w += k[j + r];
    }
    return acc / w;
  });
}

export interface RankRow {
  key: string;
  name: string;
  color: string;
  value: number;
  /** Breite des Balkens relativ zum größten (0–1). */
  share: number;
  mix: CostMix;
}

export interface ChartModel {
  range: CostRange;
  /** Stapel von unten nach oben; im Fokus genau einer. */
  bands: Band[];
  max: number;
  axisTop: string;
  axisMid: string;
  ticks: { label: string; at: number }[];
  rank: RankRow[];
  total: number;
  /** Fokus-Summe bzw. Gesamtsumme. */
  shown: number;
  headline: string;
  delta: string;
  mix: { name: string; pct: string }[];
  models: string;
}

const ZERO: CostMix = { input: 0, output: 0, write: 0, read: 0 };
const addMix = (a: CostMix, b: CostMix): CostMix => ({ input: a.input + b.input, output: a.output + b.output, write: a.write + b.write, read: a.read + b.read });
const sum = (a: number[]) => a.reduce((x, y) => x + y, 0);
const MODEL_NAMES: Record<string, string> = {
  "claude-opus-5-5": "Opus 5.5",
  "claude-fable-5-2": "Fable 5.2",
  "claude-sonnet-5-5": "Sonnet 5.5",
  "claude-haiku-5-5": "Haiku 5.5",
  "claude-opus-5": "Opus 5",
};

/** Das ganze Modell für Zeitraum `key` und Fokus `focus`; `null`, wenn der Zeitraum fehlt. */
export function chartModel(view: CostView, key: RangeKey, focus: Focus, lang: Lang, glatt = true): ChartModel | null {
  const range = view.ranges.find((r) => r.key === key);
  if (!range) return null;
  const n = range.starts.length;
  const zeros = () => new Array<number>(n).fill(0);
  const series = range.series;
  const total = sum(series.map((s) => sum(s.values)));
  const restKeys = series.filter((s) => s.ticket !== "" && !view.colored.includes(s.ticket));
  const rest = zeros();
  for (const s of restKeys) s.values.forEach((v, i) => (rest[i] += v));
  const byKey = (k: string) => series.find((s) => s.ticket === k);

  let bands: Band[];
  if (focus === null) {
    bands = view.colored.map((k) => ({ key: k, name: k, color: colorOf(view, k), values: byKey(k)?.values ?? zeros(), drawn: [] }));
    bands.push({ key: MORE, name: t(lang, "weitere Tickets", "other tickets"), color: "var(--c-more)", values: rest, drawn: [] });
    bands.push({ key: "", name: t(lang, "ohne Ticket", "no ticket"), color: "var(--c-none)", values: byKey("")?.values ?? zeros(), drawn: [] });
  } else {
    const values = focus === MORE ? rest : (byKey(focus)?.values ?? zeros());
    bands = [{ key: focus, name: nameOf(focus, lang), color: focus === MORE ? "var(--c-more)" : colorOf(view, focus), values, drawn: [] }];
  }
  for (const b of bands) b.drawn = smooth(b.values, glatt ? SIGMA[key] : 0);
  const stackTop = zeros();
  for (const b of bands) b.drawn.forEach((v, i) => (stackTop[i] += v));
  const max = niceMax(Math.max(...stackTop, 0));

  // Rangliste: sieben größte Tickets, Rest gesammelt, „ohne Ticket“ zuletzt.
  const named = series.filter((s) => s.ticket !== "").map((s) => ({ s, v: sum(s.values) })).filter((x) => x.v > 0);
  const shownNamed = named.slice(0, 7);
  const others = named.slice(7);
  const rank: RankRow[] = shownNamed.map(({ s, v }) => ({ key: s.ticket, name: s.ticket, color: colorOf(view, s.ticket), value: v, share: 0, mix: s.mix }));
  if (others.length) {
    rank.push({
      key: MORE,
      name: t(lang, `+${others.length} weitere`, `+${others.length} more`),
      color: "var(--c-more)",
      value: sum(others.map((o) => o.v)),
      share: 0,
      mix: others.reduce((m, o) => addMix(m, o.s.mix), ZERO),
    });
  }
  const none = byKey("");
  if (none && sum(none.values) > 0) rank.push({ key: "", name: nameOf("", lang), color: "var(--c-none)", value: sum(none.values), share: 0, mix: none.mix });
  const top = Math.max(...rank.map((r) => r.value), 1);
  rank.forEach((r) => (r.share = r.value / top));

  const shown = focus === null ? total : sum(bands[0].values);
  const mixAll = focus === null ? series.reduce((m, s) => addMix(m, s.mix), ZERO) : (rank.find((r) => r.key === focus)?.mix ?? ZERO);
  const mixSum = mixAll.input + mixAll.output + mixAll.write + mixAll.read || 1;
  const pct = (v: number) => `${Math.round((v / mixSum) * 100)} %`;
  const long = rangeLong(key, lang);
  const usd = Math.round((shown * 4) / 1e6).toLocaleString(lang === "en" ? "en-US" : "de-DE");
  const headline =
    focus === null
      ? t(lang, `Kosten in Input-Tokens ${long}, etwa ${usd} $ zu API-Preisen`, `cost in input tokens ${long}, about $${usd} at API prices`)
      : `${nameOf(focus, lang)}, ${Math.round((shown / (total || 1)) * 100)} % ${t(lang, "von allem", "of all")} ${long}`;
  const prevName = { h1: t(lang, "der Stunde davor", "the hour before"), d1: t(lang, "den 24 Std. davor", "the 24 h before"), w1: t(lang, "den 7 Tagen davor", "the 7 days before"), m1: t(lang, "den 30 Tagen davor", "the 30 days before") }[key];
  const delta =
    focus === null && range.prev > 0
      ? `${total >= range.prev ? "+" : "−"}${Math.abs(Math.round((total / range.prev - 1) * 100))} % ${t(lang, "ggü.", "vs.")} ${prevName}`
      : "";
  const modelTotal = sum(range.models.map((m) => m.value)) || 1;
  const models = range.models
    .filter((m) => m.value / modelTotal >= 0.005)
    .map((m) => `${MODEL_NAMES[m.model] ?? m.model} ${Math.round((m.value / modelTotal) * 100)} %`)
    .join(" · ");
  return {
    range,
    bands,
    max,
    axisTop: fmtAxis(max),
    axisMid: fmtAxis(max / 2),
    ticks: range.starts.flatMap((s, i) => {
      const label = tickLabel(key, s, lang);
      return label ? [{ label, at: (i + 0.5) / n }] : [];
    }),
    rank,
    total,
    shown,
    headline,
    delta,
    mix: [
      { name: t(lang, "Cache lesen", "cache read"), pct: pct(mixAll.read) },
      { name: t(lang, "Cache schreiben", "cache write"), pct: pct(mixAll.write) },
      { name: "Output", pct: pct(mixAll.output) },
      { name: "Input", pct: pct(mixAll.input) },
    ],
    models,
  };
}

export function nameOf(key: string, lang: Lang): string {
  if (key === "") return t(lang, "ohne Ticket", "no ticket");
  if (key === MORE) return t(lang, "weitere Tickets", "other tickets");
  return key;
}

/**
 * Fläche eines Bandes im viewBox 0 0 1000 `h`, Punkte in Eimermitte, bis an beide Ränder.
 * Ober- und Unterkante als monotone Kurve (Fritsch-Carlson): weich, aber ohne Überschwinger
 * unter null oder über den Nachbarpunkt hinaus.
 */
export function bandPath(top: number[], bottom: number[], max: number, h: number): string {
  const n = top.length;
  if (n === 0) return "";
  const xs = top.map((_, i) => ((i + 0.5) / n) * 1000);
  const y = (v: number) => h - (v / max) * h;
  const f = (v: number) => v.toFixed(1);
  const upper = curve(xs, top.map(y));
  const lower = curve(xs.slice().reverse(), bottom.map(y).reverse());
  return `M0,${f(y(top[0]))} L${f(xs[0])},${f(y(top[0]))} ${upper} L1000,${f(y(top[n - 1]))} L1000,${f(y(bottom[n - 1]))} L${f(xs[n - 1])},${f(y(bottom[n - 1]))} ${lower} L0,${f(y(bottom[0]))} Z`;
}

/** Kubische Segmente durch alle Punkte; der erste Punkt ist schon gesetzt. */
function curve(xs: number[], ys: number[]): string {
  const n = xs.length;
  if (n < 2) return "";
  const d = xs.slice(1).map((x, i) => (ys[i + 1] - ys[i]) / (x - xs[i]));
  const m = xs.map((_, i) => {
    if (i === 0) return d[0];
    if (i === n - 1) return d[n - 2];
    return d[i - 1] * d[i] <= 0 ? 0 : (2 * d[i - 1] * d[i]) / (d[i - 1] + d[i]);
  });
  const f = (v: number) => v.toFixed(1);
  const out: string[] = [];
  for (let i = 0; i < n - 1; i++) {
    const dx = (xs[i + 1] - xs[i]) / 3;
    out.push(`C${f(xs[i] + dx)},${f(ys[i] + m[i] * dx)} ${f(xs[i + 1] - dx)},${f(ys[i + 1] - m[i + 1] * dx)} ${f(xs[i + 1])},${f(ys[i + 1])}`);
  }
  return out.join(" ");
}

/** Stapelgrenzen: für jedes Band seine Ober- und Unterkante. */
export function stack(bands: Band[]): { band: Band; top: number[]; bottom: number[] }[] {
  const n = bands[0]?.drawn.length ?? 0;
  let acc = new Array<number>(n).fill(0);
  return bands.map((band) => {
    const bottom = acc;
    acc = acc.map((a, i) => a + band.drawn[i]);
    return { band, top: acc, bottom };
  });
}
