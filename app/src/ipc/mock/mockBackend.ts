// Ein Hintergrund ohne Rust: bedient dieselben Befehle und Ereignisse wie IPC.md, mit
// Beispieldaten. Damit lässt sich die Oberfläche allein im Browser bauen und anschauen
// (`pnpm dev`). Er wechselt ab und zu Zustände, damit Atmen und Aufblühen zu sehen sind.
//
// Startwerte über die Adresse: ?lang=en, ?edge=left, ?phase=closed, ?mode=bypassPermissions,
// ?still=1 (keine Zustandswechsel, für Screenshots), ?blocks=3 (dritter Block), ?zoom=0.8,
// ?orphans=0 (keine Prozesse ohne Fenster), ?orphanError=1 (eine Zeile mit Fehler vom
// letzten Beenden).
// Playwright steuert über window.__deckMock.

import type { DeckView } from "../../bindings/DeckView";
import type { DockView } from "../../bindings/DockView";
import type { SettingsView } from "../../bindings/SettingsView";
import type { Status } from "../../bindings/Status";
import type { TileView } from "../../bindings/TileView";
import { langFromSetting, type Lang } from "../../logic/i18n";
import type { Backend, CommandName, Commands, EventName, EventPayloads, ScreenRect, Unlisten } from "../contract";
import type { OrphanView } from "../../bindings/OrphanView";
import { initialBlocks, initialSettings, orphans as mockOrphans, statusText, SUMMARIES, urgentOf, usage } from "./mockData";

type Handler = (payload: unknown) => void;

interface MockControl {
  setDock(dock: Partial<DockView>): void;
  setStatus(slot: string, status: Status): void;
  emit<E extends EventName>(event: E, payload: EventPayloads[E]): void;
  /** Letzte gemeldete Panelgröße (panel_content_size). */
  contentSize: { width: number; height: number } | null;
  /** Mitschnitt der Befehle, zum Nachprüfen in Playwright. */
  calls: Array<{ cmd: string; args: unknown }>;
}

declare global {
  interface Window {
    __deckMock?: MockControl;
  }
}

/** Übergänge, die im Alltag vorkommen: so wechselt der Mock, nicht wahllos. */
const NEXT: Partial<Record<Status, Status[]>> = {
  idle: ["thinking"],
  thinking: ["waiting", "done", "thinking"],
  waiting: ["thinking"],
  done: ["idle", "thinking"],
};

export function createMockBackend(): Backend {
  const params = new URLSearchParams(window.location.search);
  const handlers = new Map<EventName, Set<Handler>>();
  const now = () => Date.now() / 1000;

  const settings: SettingsView = { ...initialSettings() };
  if (params.get("lang") === "en") settings.language = "english";
  if (params.get("mode")) settings.mode = params.get("mode")!;
  if (params.get("edge")) settings.dockEdge = params.get("edge")!;

  let lang: Lang = langFromSetting(settings.language);
  const blocks = initialBlocks(lang, now() - 600, params.get("blocks") === "3");
  let dock: DockView = { edge: settings.dockEdge, phase: params.get("phase") ?? "open", zoom: Number(params.get("zoom") ?? 1) };

  let orphans: OrphanView[] = params.get("orphans") === "0" ? [] : mockOrphans(lang, params.get("orphanError") === "1");
  const view = (): DeckView => ({ blocks, usage: usage(lang), urgent: urgentOf(blocks), lang, orphans });
  // Beenden wie im echten Deck: erst „beendet…“, dann verschwindet die Zeile.
  const killOrphans = (ids: string[]) => {
    orphans = orphans.map((o) => (ids.includes(o.id) ? { ...o, busy: true, error: null } : o));
    pushView();
    window.setTimeout(() => {
      orphans = orphans.filter((o) => !ids.includes(o.id));
      pushView();
    }, 700);
  };

  const emit = <E extends EventName>(event: E, payload: EventPayloads[E]) => {
    handlers.get(event)?.forEach((h) => h(structuredClone(payload)));
  };
  const pushView = () => emit("deck://view", view());

  const findTile = (slot: string): TileView | undefined => {
    for (const b of blocks) {
      const t = b.tiles.find((x) => x.slot === slot);
      if (t) return t;
    }
    return undefined;
  };

  const setStatus = (slot: string, status: Status) => {
    const t = findTile(slot);
    if (!t || t.status === status) return;
    t.status = status;
    t.statusLabel = statusText(status, lang);
    t.statusChangedAt = now();
    pushView();
  };

  const relabel = () => {
    for (const b of blocks) for (const t of b.tiles) t.statusLabel = statusText(t.status, lang);
  };

  let nextSlotNo = 6;
  let tipSlot: string | null = null;
  let summaryTimer = 0;
  const commands: { [K in CommandName]: (args: Commands[K]["args"]) => Commands[K]["ret"] } = {
    get_view: () => view(),
    get_dock: () => ({ ...dock }),
    get_settings: () => ({ ...settings }),
    panel_content_size: (a) => {
      control.contentSize = { width: a.width, height: a.height };
    },
    focus_slot: ({ slot }) => {
      for (const b of blocks) for (const t of b.tiles) t.selected = t.slot === slot;
      const t = findTile(slot);
      // Wie im echten Deck: wer eine ungelesene Antwort anschaut, hat sie gelesen.
      if (t?.status === "done") setStatus(slot, "idle");
      pushView();
    },
    close_slot: ({ slot }) => {
      for (const b of blocks) b.tiles = b.tiles.filter((t) => t.slot !== slot);
      pushView();
    },
    create_agent: ({ letter }) => {
      const b = blocks.find((x) => x.letter === letter);
      if (!b) return;
      const slot = `${letter}${nextSlotNo++}`;
      b.tiles.push({
        slot, status: "thinking", statusLabel: statusText("thinking", lang), model: "Opus 5.5", effort: "xhigh",
        mode: "auto", title: lang === "en" ? "New session" : "Neue Sitzung",
        ticket: null, pr: null, cache: null, selected: false, statusChangedAt: now(),
      });
      pushView();
    },
    show_tile_menu: ({ slot }) => console.info("[mock] Kontextmenü für", slot),
    reorder_tiles: ({ letter, slots }) => {
      const b = blocks.find((x) => x.letter === letter);
      if (!b) return;
      b.tiles = slots.map((s) => b.tiles.find((t) => t.slot === s)).filter((t): t is TileView => !!t);
      pushView();
    },
    reorder_blocks: ({ letters }) => {
      const sorted = letters.map((l) => blocks.find((b) => b.letter === l)).filter((b): b is (typeof blocks)[number] => !!b);
      blocks.splice(0, blocks.length, ...sorted);
      pushView();
    },
    hover_tile: ({ slot }) => {
      window.clearTimeout(summaryTimer);
      tipSlot = slot;
      if (!slot) {
        emit("deck://tooltip-hide", null);
        return;
      }
      const t = findTile(slot);
      const s = SUMMARIES[slot];
      const origin = originOf(slot);
      const refs = { ticket: t?.ticket ?? null, pr: t?.pr ?? null };
      emit("deck://tooltip", { slot, ...refs, summary: null, lastPrompt: s?.lastPrompt ?? null, origin });
      // Die Zusammenfassung rechnet erst noch, dann kommt sie nach; aber nur, wenn der
      // Tooltip dann noch derselben Karte gilt.
      summaryTimer = window.setTimeout(() => {
        if (tipSlot !== slot) return;
        emit("deck://tooltip", { slot, ...refs, summary: s?.summary ?? null, lastPrompt: s?.lastPrompt ?? null, origin });
      }, 900);
    },
    open_usage_page: () => console.info("[mock] claude.ai/settings/usage"),
    open_settings: () => console.info("[mock] Einstellungen öffnen"),
    set_setting: ({ key, value }) => {
      (settings as Record<string, unknown>)[key] = value;
      if (key === "language") {
        lang = langFromSetting(String(value));
        relabel();
        const keep = new Set(orphans.map((o) => o.id));
        orphans = mockOrphans(lang, params.get("orphanError") === "1").filter((o) => keep.has(o.id));
        pushView();
      }
      if (key === "dockEdge") {
        dock = { ...dock, edge: String(value) };
        emit("deck://dock", { ...dock });
      }
      emit("deck://settings", { ...settings });
      return { ...settings };
    },
    restart_panel: () => console.info("[mock] Panel neu starten"),
    show_block_menu: ({ letter }) => console.info("[mock] Kontextmenü des Kopfes", letter),
    panel_hold: () => undefined,
    panel_drag: ({ mode }) => console.info("[mock] Panel ziehen", mode),
    focus_block: ({ letter }) => console.info("[mock] Fenster nach vorn", letter),
    tooltip_content_size: () => undefined,
    get_ticket: () => ({ slot: "B3", current: "", lang }),
    submit_ticket: ({ ticket }) => console.info("[mock] Ticket", ticket),
    kill_orphan: ({ id }) => killOrphans([id]),
    kill_all_orphans: () => killOrphans(orphans.map((o) => o.id)),
  };

  function originOf(slot: string): string[] {
    const b = blocks.find((x) => x.tiles.some((t) => t.slot === slot));
    const win = slot.slice(0, 1);
    const head = [b?.name ?? "", `${lang === "en" ? "Window" : "Fenster"} ${win}`, slot].filter(Boolean).join(" · ");
    const wt = findTile(slot)?.ticket;
    return wt && !wt.detected ? [head, `↳ wt/${wt.text.toLowerCase()}`] : [head];
  }

  const control: MockControl = {
    setDock: (d) => {
      dock = { ...dock, ...d };
      emit("deck://dock", { ...dock });
    },
    setStatus,
    emit,
    contentSize: null,
    calls: [],
  };
  window.__deckMock = control;

  if (params.get("still") !== "1") {
    // Ab und zu einen Zustand weiterdrehen. Getrennte Karten bleiben getrennt.
    window.setInterval(() => {
      const all = blocks.flatMap((b) => b.tiles).filter((t) => NEXT[t.status]);
      if (all.length === 0) return;
      const t = all[Math.floor(Math.random() * all.length)];
      const options = NEXT[t.status]!;
      setStatus(t.slot, options[Math.floor(Math.random() * options.length)]);
    }, 3500);
  }

  // Im Browser entscheidet kein Rust über Ein- und Ausklappen: die Taste d schaltet
  // um, damit sich die Animation ausprobieren lässt.
  window.addEventListener("keydown", (e) => {
    if (e.key === "d") control.setDock({ phase: dock.phase === "open" ? "closed" : "open" });
  });

  return {
    native: false,
    invoke<K extends CommandName>(cmd: K, args: Commands[K]["args"]): Promise<Commands[K]["ret"]> {
      control.calls.push({ cmd, args });
      try {
        // Kopie wie über die echte Schnittstelle (JSON): sonst hielte die Oberfläche die
        // Objekte des Mocks, die er später verändert, und memo sähe keinen Unterschied.
        return Promise.resolve(structuredClone(commands[cmd](args)));
      } catch (err) {
        return Promise.reject(err);
      }
    },
    listen<E extends EventName>(event: E, handler: (payload: EventPayloads[E]) => void): Unlisten {
      const set = handlers.get(event) ?? new Set<Handler>();
      handlers.set(event, set);
      const h = handler as Handler;
      set.add(h);
      return () => set.delete(h);
    },
    async screenRect(el: Element): Promise<ScreenRect> {
      // Im Browser zählt die Seite als Bildschirm: der Tooltip erscheint als Überlagerung.
      const r = el.getBoundingClientRect();
      return { x: r.left, y: r.top, width: r.width, height: r.height };
    },
    setZoom: () => undefined,
  };
}
