// Einstieg in die Schnittstelle: in Tauri die echte Anbindung, im Browser der Mock.
//
// Die Ansichten rufen nur die Funktionen hier, nie `invoke` direkt. So steht an einer
// Stelle, welche Befehle es gibt, und der Mock kann nicht vom Vertrag abweichen.

import { isTauri } from "@tauri-apps/api/core";
import type { SettingsView } from "../bindings/SettingsView";
import type { Backend, PanelDragMode, SettingKey } from "./contract";
import { createMockBackend } from "./mock/mockBackend";
import { createTauriBackend } from "./tauriBackend";

let backend: Backend | null = null;

export function getBackend(): Backend {
  backend ??= isTauri() ? createTauriBackend() : createMockBackend();
  return backend;
}

/** Ein Befehl, dessen Ausgang die Oberfläche nicht braucht: Fehler nur ins Log. */
function fire(promise: Promise<unknown>, what: string): void {
  promise.catch((err) => console.warn(`[deck] ${what} fehlgeschlagen:`, err));
}

export const commands = {
  getView: () => getBackend().invoke("get_view", {}),
  getDock: () => getBackend().invoke("get_dock", {}),
  getSettings: () => getBackend().invoke("get_settings", {}),
  panelContentSize: (width: number, height: number) =>
    fire(getBackend().invoke("panel_content_size", { width, height }), "panel_content_size"),
  focusSlot: (slot: string) => fire(getBackend().invoke("focus_slot", { slot }), "focus_slot"),
  closeSlot: (slot: string) => fire(getBackend().invoke("close_slot", { slot }), "close_slot"),
  createAgent: (letter: string) => fire(getBackend().invoke("create_agent", { letter }), "create_agent"),
  showTileMenu: (slot: string) => fire(getBackend().invoke("show_tile_menu", { slot }), "show_tile_menu"),
  reorderTiles: (letter: string, slots: string[]) =>
    fire(getBackend().invoke("reorder_tiles", { letter, slots }), "reorder_tiles"),
  reorderBlocks: (letters: string[]) => fire(getBackend().invoke("reorder_blocks", { letters }), "reorder_blocks"),
  hoverTile: async (slot: string | null, el: Element | null) => {
    const b = getBackend();
    const rect = slot && el ? await b.screenRect(el) : null;
    fire(b.invoke("hover_tile", { slot, rect }), "hover_tile");
  },
  openUsagePage: () => fire(getBackend().invoke("open_usage_page", {}), "open_usage_page"),
  openSettings: () => fire(getBackend().invoke("open_settings", {}), "open_settings"),
  setSetting: <K extends SettingKey>(key: K, value: SettingsView[K]) => getBackend().invoke("set_setting", { key, value }),
  restartPanel: () => getBackend().invoke("restart_panel", {}),
  focusBlock: (letter: string) => fire(getBackend().invoke("focus_block", { letter }), "focus_block"),
  tooltipContentSize: (width: number, height: number) =>
    fire(getBackend().invoke("tooltip_content_size", { width, height }), "tooltip_content_size"),
  showBlockMenu: (letter: string) => fire(getBackend().invoke("show_block_menu", { letter }), "show_block_menu"),
  panelHold: (hold: boolean) => fire(getBackend().invoke("panel_hold", { hold }), "panel_hold"),
  panelDrag: (mode: PanelDragMode, x: number, y: number) =>
    fire(getBackend().invoke("panel_drag", { mode, x, y }), "panel_drag"),
  getTicket: () => getBackend().invoke("get_ticket", {}),
  submitTicket: (ticket: string | null) => fire(getBackend().invoke("submit_ticket", { ticket }), "submit_ticket"),
};

export function isNative(): boolean {
  return getBackend().native;
}
