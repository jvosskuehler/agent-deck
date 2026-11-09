// Der Vertrag zwischen Oberfläche und Rust, wie er in docs/IPC.md steht, als Typen.
//
// Wer hier etwas ändert, ändert zuerst IPC.md. Die Nutzlasten sind die aus Rust
// erzeugten Bindings; nichts davon wird hier nachgebaut.

import type { DeckView } from "../bindings/DeckView";
import type { DockView } from "../bindings/DockView";
import type { SettingsView } from "../bindings/SettingsView";
import type { TicketView } from "../bindings/TicketView";
import type { TooltipView } from "../bindings/TooltipView";

/** Ereignisse Rust → Oberfläche. */
export interface EventPayloads {
  "deck://view": DeckView;
  "deck://dock": DockView;
  "deck://tooltip": TooltipView;
  "deck://tooltip-hide": null;
  "deck://settings": SettingsView;
  "deck://ticket": TicketView;
}

export type EventName = keyof EventPayloads;

/** Rechteck einer Karte in Bildschirm-Pixeln (physisch, wie Win32 sie zählt). */
export interface ScreenRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** Was das freie Panel beim Ziehen tut: verschieben oder an der Ecke zoomen. */
export type PanelDragMode = "move" | "zoom";

export type SettingKey = keyof SettingsView;

/** Befehle Oberfläche → Rust (`invoke`), je mit Argumenten und Rückgabe. */
export interface Commands {
  get_view: { args: Record<string, never>; ret: DeckView };
  get_dock: { args: Record<string, never>; ret: DockView };
  get_settings: { args: Record<string, never>; ret: SettingsView };
  panel_content_size: { args: { width: number; height: number }; ret: void };
  focus_slot: { args: { slot: string }; ret: void };
  close_slot: { args: { slot: string }; ret: void };
  create_agent: { args: { letter: string }; ret: void };
  show_tile_menu: { args: { slot: string }; ret: void };
  reorder_tiles: { args: { letter: string; slots: string[] }; ret: void };
  reorder_blocks: { args: { letters: string[] }; ret: void };
  hover_tile: { args: { slot: string | null; rect: ScreenRect | null }; ret: void };
  open_usage_page: { args: Record<string, never>; ret: void };
  open_settings: { args: Record<string, never>; ret: void };
  set_setting: { args: { key: SettingKey; value: SettingsView[SettingKey] }; ret: SettingsView };
  /** „Panel neu starten“: frische Instanz, diese endet. */
  restart_panel: { args: Record<string, never>; ret: void };
  /** Rechtsklick auf den Kopf eines Blocks: natives Menü (vergessen, verbinden). */
  show_block_menu: { args: { letter: string }; ret: void };
  /** Karte oder Block wird gezogen: Panel offen halten, auch wenn der Zeiger es verlässt. */
  panel_hold: { args: { hold: boolean }; ret: void };
  /** Freies Panel: Rust folgt dem Zeiger, bis die Taste los ist (verschieben bzw. zoomen). */
  /** x, y: Stelle des Drucks im Panel (clientX/Y, CSS-Pixel), damit das Panel genau dort am Zeiger hängt. */
  panel_drag: { args: { mode: PanelDragMode; x: number; y: number }; ret: void };
  /** Klick auf den Repo-Namen holt das Fenster des Blocks nach vorn (Python: show_window). */
  focus_block: { args: { letter: string }; ret: void };
  /** Der Tooltip meldet seine Größe, damit Rust ihn auf dem Monitor halten kann. */
  tooltip_content_size: { args: { width: number; height: number }; ret: void };
  /** Inhalt des Ticket-Dialogs (null, solange keiner geöffnet wurde). */
  get_ticket: { args: Record<string, never>; ret: TicketView | null };
  /** Ticket zuweisen (Text) oder abbrechen (null); schließt den Dialog. */
  submit_ticket: { args: { ticket: string | null }; ret: void };
}

export type CommandName = keyof Commands;

export type Unlisten = () => void;

/** Was eine Ansicht vom Hintergrund braucht: Befehle rufen, Ereignisse hören. */
export interface Backend {
  /** true in Tauri, false im Browser mit Mock. */
  readonly native: boolean;
  invoke<K extends CommandName>(cmd: K, args: Commands[K]["args"]): Promise<Commands[K]["ret"]>;
  listen<E extends EventName>(event: E, handler: (payload: EventPayloads[E]) => void): Unlisten;
  /** Bildschirmrechteck eines Elements (für hover_tile). */
  screenRect(el: Element): Promise<ScreenRect>;
  /** Wirksamer Zoom der Webview (DockView.zoom), für screenRect. */
  setZoom(zoom: number): void;
}
