// Repo-Blöcke per Ziehen am Repo-Namen umsortieren: senkrecht, was die Karten im
// Raster tun (gemeinsame Mechanik in useDragSession).
//
// Der Repo-Name ist zugleich ein Knopf: ein Druck ohne Bewegung holt das Fenster des
// Blocks nach vorn (wie im Python-Deck). Blöcke sind unterschiedlich hoch, darum rechnet
// das Ziel mit den gemessenen Höhen (dropIndex/reflowTops). Der Heimat-Block steht
// immer oben; er zieht nicht mit und lässt sich nicht überholen.
//
// Das Panel zeichnet nur neu, wenn ein Zug beginnt, landet oder endet; die Verschiebung
// je Zeigerbewegung holt sich jeder Block selbst (useBlockOffset).

import { dropIndex, moveTo, pinFirst, reflowTops, sameOrder, slotTops } from "../../logic/reorder";
import { useDragSelect, useDragSession, type DragHandlers, type DragStore } from "./useDragSession";

interface Geometry {
  /** Ziehbare Blöcke (ohne Heimat) in Anzeigereihenfolge. */
  order: string[];
  tops: number[];
  heights: number[];
  gap: number;
}

interface DragState {
  letter: string;
  from: number;
  target: number;
  dy: number;
  geo: Geometry;
  landing: boolean;
}

export type BlockShift = DragStore<DragState>;

interface BlockDragOptions {
  /** Alle Blöcke in Anzeigereihenfolge. */
  letters: readonly string[];
  home: string | null;
  /** Gezeichnete Lage eines Blocks (relativ zum gemeinsamen Behälter). */
  measure(letter: string): { top: number; height: number } | null;
  onClick(letter: string): void;
  onDragStart(): void;
  /** Der Zug ist vorbei (abgelegt oder abgebrochen). */
  onDragEnd(): void;
  onCommit(letters: string[]): void;
}

interface BlockDrag {
  /** Für useBlockOffset; bleibt über alle Renders dasselbe Objekt. */
  shift: BlockShift;
  liftedLetter: string | null;
  landing: boolean;
  settling: boolean;
  dragging: boolean;
  /** Stabil über alle Renders. */
  handlers: DragHandlers;
}

export function useBlockDrag({ letters, home, measure, onClick, onDragStart, onDragEnd, onCommit }: BlockDragOptions): BlockDrag {
  const begin = (letter: string): Geometry | null => {
    const order = letters.filter((l) => l !== home);
    // Ein Block allein hat keine Reihenfolge; ein Kopf, der sich löst und nirgends hin
    // kann, sähe nach einem Fehler aus.
    if (!order.includes(letter) || order.length < 2) return null;
    const boxes = order.map(measure);
    if (boxes.some((b) => b === null)) return null;
    const tops = boxes.map((b) => b!.top);
    const heights = boxes.map((b) => b!.height);
    const gap = Math.max(0, tops[1] - (tops[0] + heights[0]));
    return { order, tops, heights, gap };
  };

  const { store, settling, handlers } = useDragSession<DragState>({
    // Ein Kopf, der nicht ziehen kann, wird nach der Bewegung auch kein Klick mehr.
    refused: "ignore",
    start(letter) {
      const geo = begin(letter);
      if (!geo) return null;
      const from = geo.order.indexOf(letter);
      return { letter, from, target: from, dy: 0, geo, landing: false };
    },
    move(d, _dx, dy) {
      const { geo, from } = d;
      const target = dropIndex(geo.heights, geo.gap, geo.tops[0], from, geo.tops[from] + dy);
      return { ...d, dy, target };
    },
    land(d, commit) {
      const { geo } = d;
      const landTop = commit ? slotTops(geo.heights, geo.gap, geo.tops[0], d.from)[d.target] : geo.tops[d.from];
      return {
        state: { ...d, dy: landTop - geo.tops[d.from], landing: true },
        done: () => {
          const next = pinFirst([...(home ? [home] : []), ...moveTo(geo.order, d.letter, d.target)], home);
          if (commit && !sameOrder(next, letters)) onCommit(next);
        },
      };
    },
    onClick,
    onDragStart,
    onDragEnd,
  });

  const liftedLetter = useDragSelect(store, (d) => d?.letter ?? null);
  const landing = useDragSelect(store, (d) => d?.landing ?? false);

  return { shift: store, liftedLetter, landing, settling, dragging: liftedLetter !== null, handlers };
}

/** Senkrechte Verschiebung eines Blocks (0 = an seinem Platz). */
function offsetOf(drag: DragState | null, letter: string): number {
  if (!drag) return 0;
  if (letter === drag.letter) return drag.dy;
  const i = drag.geo.order.indexOf(letter);
  const to = reflowTops(drag.geo.heights, drag.geo.gap, drag.geo.tops[0], drag.from, drag.target).get(i);
  return to === undefined ? 0 : to - drag.geo.tops[i];
}

/** Die Verschiebung des Blocks <letter>; zeichnet nur neu, wenn sie sich ändert. */
export function useBlockOffset(shift: BlockShift, letter: string): number {
  return useDragSelect(shift, (d) => offsetOf(d, letter));
}
