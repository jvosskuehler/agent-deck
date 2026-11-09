// Eine eben gezogene Reihenfolge gilt sofort, nicht erst, wenn Rust sie im nächsten
// Takt zurückschickt; sonst sprängen die Karten nach dem Ablegen einmal zurück. Sobald
// Rust dieselbe Reihenfolge meldet oder die Frist abläuft, gilt wieder allein Rust.
//
// Ohne React: ein kleiner Speicher mit unveränderlichem Stand, den die Oberfläche per
// useSyncExternalStore liest.

import { sameOrder } from "./reorder";

/** So lange hält die Oberfläche eine eben gezogene Reihenfolge, bis Rust sie bestätigt. */
export const PENDING_MS = 1500;

export interface Pending {
  /** Karten je Block (Buchstabe → Slots). */
  tiles: Readonly<Record<string, string[]>>;
  blocks: string[] | null;
}

/** Was Rust gerade meldet: Blöcke mit ihren Karten, je in Anzeigereihenfolge. */
export interface Reported {
  blocks: ReadonlyArray<{ letter: string; tiles: ReadonlyArray<{ slot: string }> }>;
}

export interface PendingOrder {
  get(): Pending;
  subscribe(listener: () => void): () => void;
  holdTiles(letter: string, slots: string[]): void;
  holdBlocks(letters: string[]): void;
  /** Rust hat gemeldet: was es bestätigt, braucht die Oberfläche nicht mehr zu halten. */
  confirm(view: Reported): void;
  /** Laufende Fristen beenden (Ansicht wird abgebaut). */
  dispose(): void;
}

export function createPendingOrder(ms = PENDING_MS): PendingOrder {
  let state: Pending = { tiles: {}, blocks: null };
  const listeners = new Set<() => void>();
  const timers = new Set<ReturnType<typeof setTimeout>>();

  const set = (next: Pending) => {
    if (next === state) return;
    state = next;
    listeners.forEach((l) => l());
  };

  const after = (fn: () => void) => {
    const id = setTimeout(() => {
      timers.delete(id);
      fn();
    }, ms);
    timers.add(id);
  };

  const dropTiles = (letters: readonly string[]): Pending => {
    if (letters.length === 0) return state;
    const tiles = { ...state.tiles };
    for (const l of letters) delete tiles[l];
    return { ...state, tiles };
  };

  return {
    get: () => state,
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    holdTiles(letter, slots) {
      set({ ...state, tiles: { ...state.tiles, [letter]: slots } });
      // Nur diese Reihenfolge läuft ab; eine neuere für denselben Block hat ihre eigene Frist.
      after(() => set(state.tiles[letter] === slots ? dropTiles([letter]) : state));
    },
    holdBlocks(letters) {
      set({ ...state, blocks: letters });
      after(() => set(state.blocks === letters ? { ...state, blocks: null } : state));
    },
    confirm(view) {
      const confirmed = view.blocks
        .filter((b) => {
          const want = state.tiles[b.letter];
          return want !== undefined && sameOrder(want, b.tiles.map((x) => x.slot));
        })
        .map((b) => b.letter);
      let next = dropTiles(confirmed);
      if (next.blocks && sameOrder(next.blocks, view.blocks.map((b) => b.letter))) next = { ...next, blocks: null };
      set(next);
    },
    dispose() {
      timers.forEach((id) => clearTimeout(id));
      timers.clear();
    },
  };
}
