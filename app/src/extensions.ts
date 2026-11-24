// Anbaustellen für Erweiterungen der Oberfläche (Gegenstück zu `extension.rs` in Rust).
//
// Die Team-Ausgabe registriert hier nichts. Eine andere Ausgabe hat ein eigenes
// Vite-Frontend, das diese Quellen einbindet, vor `startDeck()` ihre Ansichten
// registriert und dann startet:
//
//   registerView("info", InfoView);              // Fenster `#/info`
//   registerSettingsSection("info", InfoSection); // Abschnitt, wenn Rust ihn meldet
//   registerMockCommands({ get_info: () => … });  // nur für `pnpm dev` im Browser
//   startDeck();
//
// Welche Abschnitte es gibt, sagt Rust (`SettingsView.extras`); eine registrierte
// Ansicht ohne Abschnitt von Rust bleibt unsichtbar, ein Abschnitt ohne Ansicht ebenso.

import type { ComponentType, ReactNode } from "react";
import type { SettingsView } from "./bindings/SettingsView";
import type { Lang } from "./logic/i18n";

/** Was ein Abschnitt der Einstellungen zum Zeichnen bekommt. */
export interface SettingsSectionProps {
  lang: Lang;
  /** Die Werte aus Rust (`SettingsSection.values`). */
  values: Record<string, unknown>;
  /** Zweispaltig aufgeklappt („Erweitert“)? */
  expanded: boolean;
  /** Eine Einstellung speichern; Rust antwortet mit dem ganzen neuen Stand. */
  save(key: string, value: unknown): void;
  /** Ein Stand, den die Erweiterung selbst geholt hat (etwa nach eigenem Befehl). */
  onSaved(next: SettingsView): void;
  onError(message: string): void;
}

export interface SettingsSectionEntry {
  /** Nur sichtbar, wenn „Erweitert“ aufgeklappt ist. */
  advancedOnly?: boolean;
  render(props: SettingsSectionProps): ReactNode;
}

const views = new Map<string, ComponentType>();
const sections = new Map<string, SettingsSectionEntry>();
const mockCommands = new Map<string, (args: Record<string, unknown>) => unknown>();

/** Ein eigenes Fenster: Rust legt es mit dem Label `route` an, die Seite zeigt `view`. */
export function registerView(route: string, view: ComponentType): void {
  views.set(route, view);
}

export function viewFor(route: string): ComponentType | undefined {
  return views.get(route);
}

export function registerSettingsSection(id: string, entry: SettingsSectionEntry): void {
  sections.set(id, entry);
}

export function settingsSection(id: string): SettingsSectionEntry | undefined {
  return sections.get(id);
}

/**
 * Befehle der Erweiterung für den Browser-Mock (`pnpm dev`); in Tauri ohne Wirkung.
 * Besondere Namen: `mock_settings_extras` liefert die Startwerte der Abschnitte
 * (`SettingsView.extras`), `set_setting:<key>` bedient eine Einstellung der Erweiterung
 * (bekommt `{ value, settings }` und ändert `settings.extras` selbst).
 */
export function registerMockCommands(map: Record<string, (args: Record<string, unknown>) => unknown>): void {
  for (const [k, v] of Object.entries(map)) mockCommands.set(k, v);
}

export function mockCommand(cmd: string): ((args: Record<string, unknown>) => unknown) | undefined {
  return mockCommands.get(cmd);
}
