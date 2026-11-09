// Auswahl-Listen des Einstellungsfensters und die Zuordnung Wert → Segment.
//
// Die Werte sind die, die Rust in SettingsView erwartet und schreibt; die Labels sind
// die Segment-Beschriftungen. Reihenfolgen wie in einstellungen-glas.html.

import type { Lang } from "./i18n";

export interface Option {
  value: string;
  label: string;
}

export function modelOptions(): Option[] {
  return [
    { value: "opus[1m]", label: "Opus" },
    { value: "fable", label: "Fable" },
    { value: "sonnet", label: "Sonnet" },
    { value: "haiku", label: "Haiku" },
  ];
}

export function modeOptions(lang: Lang): Option[] {
  return [
    { value: "default", label: lang === "en" ? "Default" : "Standard" },
    { value: "acceptEdits", label: "Edits" },
    { value: "plan", label: "Plan" },
    { value: "auto", label: "Auto" },
    { value: "bypassPermissions", label: "Bypass" },
  ];
}

/** Effort als Label wie in SettingsView (`Ultracode`, `max` …), aufsteigend sortiert. */
export function effortOptions(): Option[] {
  return [
    { value: "low", label: "low" },
    { value: "medium", label: "medium" },
    { value: "high", label: "high" },
    { value: "xhigh", label: "xhigh" },
    { value: "max", label: "max" },
    { value: "Ultracode", label: "ultra" },
  ];
}

export function languageOptions(): Option[] {
  // Jede Sprache in sich selbst benannt: wer die andere sucht, soll sie lesen können.
  return [
    { value: "german", label: "Deutsch" },
    { value: "english", label: "English" },
  ];
}

export function edgeOptions(lang: Lang): Option[] {
  const de = lang !== "en";
  return [
    { value: "top", label: de ? "Oben" : "Top" },
    { value: "left", label: de ? "Links" : "Left" },
    { value: "right", label: de ? "Rechts" : "Right" },
    { value: "bottom", label: de ? "Unten" : "Bottom" },
    { value: "off", label: de ? "Frei" : "Free" },
  ];
}

/** Ein Schalter als zwei Segmente (Schalter selbst gibt es im Fenster nicht mehr). */
export function onOffOptions(lang: Lang): Option[] {
  const de = lang !== "en";
  return [
    { value: "on", label: de ? "An" : "On" },
    { value: "off", label: de ? "Aus" : "Off" },
  ];
}

/**
 * Welches Segment zeigt den Wert <value>? Erst exakt, dann ohne Groß/klein, dann über
 * den Kern (`opus` trifft `opus[1m]`): settings.json kann ältere oder gekürzte Werte
 * enthalten, und ein Fenster ohne markiertes Segment sähe kaputt aus. Kein Treffer → null.
 */
export function matchOption(options: readonly Option[], value: string): string | null {
  const v = value.trim();
  if (v === "") return null;
  const exact = options.find((o) => o.value === v);
  if (exact) return exact.value;
  const lower = v.toLowerCase();
  const folded = options.find((o) => o.value.toLowerCase() === lower);
  if (folded) return folded.value;
  const core = (s: string) => s.toLowerCase().replace(/\[.*\]$/, "");
  const byCore = options.find((o) => core(o.value) === core(v));
  return byCore ? byCore.value : null;
}
