// Zeiten und kleine Texte des PR-Feeds. Rust schickt Unix-Sekunden; die Uhrzeit rechnet
// die Oberfläche, weil nur sie die Ortszeit des Browsers kennt. Keine Gedankenstriche.

import type { Lang } from "./i18n";

/** „15:27“ in Ortszeit. */
export function clock(at: number): string {
  const d = new Date(at * 1000);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

/** „gerade eben“, „vor 4 Min.“, „vor 3 Std.“, „vor 2 Tagen“ (bzw. englisch). */
export function ago(at: number, now: number, lang: Lang): string {
  const s = Math.max(0, now - at);
  const en = lang === "en";
  if (s < 60) return en ? "just now" : "gerade eben";
  const min = Math.floor(s / 60);
  if (min < 60) return en ? `${min} min ago` : `vor ${min} Min.`;
  const h = Math.floor(min / 60);
  if (h < 24) return en ? `${h} h ago` : `vor ${h} Std.`;
  const d = Math.floor(h / 24);
  if (en) return d === 1 ? "1 day ago" : `${d} days ago`;
  return d === 1 ? "vor 1 Tag" : `vor ${d} Tagen`;
}

/** Kürzel im Avatar: aus Rust, sonst die ersten zwei Buchstaben des Logins. */
export function initialsOf(initials: string, login: string): string {
  const given = initials.trim();
  if (given) return given.slice(0, 2).toUpperCase();
  const letters = login.replace(/[^A-Za-z0-9]/g, "");
  return (letters.slice(0, 2) || "?").toUpperCase();
}

/** „und 2 weitere“ hinter dem Auszug; leer bei keinem. */
export function moreText(more: number, lang: Lang): string {
  if (more <= 0) return "";
  return lang === "en" ? `and ${more} more` : `und ${more} weitere`;
}
