// „Läuft noch ohne Fenster“: die kleinen Rechnungen der Oberfläche. Zeilen, Ports und
// Herkunft kommen fertig aus Rust (OrphanView); hier nur Hinweistext und Platz.

import type { Lang } from "./i18n";

/** Hinweis in der Leiste: „2 ohne Fenster“ / „2 without window“. */
export function orphanHint(count: number, lang: Lang): string {
  return lang === "en" ? `${count} without window` : `${count} ohne Fenster`;
}

/**
 * Höchsthöhe des Popovers über der Leiste: was zwischen Oberkante des Panels und der
 * Leiste frei ist, abzüglich Rand. Das Fenster ist so groß wie das Panel; was darüber
 * hinausragte, wäre abgeschnitten. Nie unter 120 px (dann rollt die Liste).
 */
export function popoverMaxHeight(panelTop: number, barTop: number, margin = 10): number {
  return Math.max(120, Math.floor(barTop - panelTop - margin));
}
