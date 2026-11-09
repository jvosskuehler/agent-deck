// Karten innerhalb eines Blocks per Ziehen umsortieren: das Raster zur gemeinsamen
// Zieh-Mechanik (useDragSession).
//
// Ein Druck ohne Bewegung bleibt ein Klick (fokussieren). Eine einzelne Karte hat keine
// Reihenfolge, dann bleibt es auch mit Bewegung beim Klick.

import {
  cellOf,
  gridDropIndex,
  gridReflow,
  moveTo,
  sameOrder,
  type Grid,
  type Point,
} from "../../logic/reorder";
import { useDragSelect, useDragSession, type DragHandlers } from "./useDragSession";

interface DragState {
  slot: string;
  from: number;
  target: number;
  /** Verschiebung der gezogenen Karte gegenüber ihrer Zelle. */
  dx: number;
  dy: number;
  landing: boolean;
}

interface TileDragOptions {
  order: readonly string[];
  grid: Grid;
  onClick(slot: string): void;
  onDragStart(): void;
  /** Der Zug ist vorbei (abgelegt oder abgebrochen). */
  onDragEnd(): void;
  onCommit(order: string[]): void;
}

interface TileDrag {
  /** Verschiebung einer Karte (null = an ihrem Platz). */
  offsetOf(slot: string): Point | null;
  /** Die Karte, die gerade gezogen wird. */
  liftedSlot: string | null;
  landing: boolean;
  settling: boolean;
  dragging: boolean;
  /** Stabil über alle Renders. */
  handlers: DragHandlers;
}

export function useTileDrag({ order, grid, onClick, onDragStart, onDragEnd, onCommit }: TileDragOptions): TileDrag {
  const { store, settling, handlers } = useDragSession<DragState>({
    refused: "click",
    start(slot) {
      if (order.length < 2 || !order.includes(slot)) return null;
      const from = order.indexOf(slot);
      return { slot, from, target: from, dx: 0, dy: 0, landing: false };
    },
    move(d, dx, dy) {
      const from = order.indexOf(d.slot);
      const home = cellOf(from, grid);
      const target = gridDropIndex(order.length, grid, { x: home.x + dx, y: home.y + dy });
      return { slot: d.slot, from, target, dx, dy, landing: false };
    },
    land(d, commit) {
      const next = moveTo(order, d.slot, d.target);
      const home = cellOf(d.from, grid);
      const cell = commit ? cellOf(d.target, grid) : home;
      return {
        // Erst in die Zelle gleiten lassen, dann festschreiben.
        state: { ...d, dx: cell.x - home.x, dy: cell.y - home.y, landing: true },
        done: () => {
          if (commit && !sameOrder(next, order)) onCommit(next);
        },
      };
    },
    onClick,
    onDragStart,
    onDragEnd,
  });
  // Der Block zeichnet bei jeder Bewegung neu; die Karten selbst nur, wenn sich ihre
  // Verschiebung ändert (Tile ist memo).
  const drag = useDragSelect(store, (d) => d);

  const want = drag ? gridReflow(order.length, drag.from, drag.target, grid) : null;

  const offsetOf = (slot: string): Point | null => {
    if (!drag) return null;
    if (slot === drag.slot) return { x: drag.dx, y: drag.dy };
    const i = order.indexOf(slot);
    const to = want?.get(i);
    if (!to) return null;
    const home = cellOf(i, grid);
    return to.x === home.x && to.y === home.y ? null : { x: to.x - home.x, y: to.y - home.y };
  };

  return {
    offsetOf,
    liftedSlot: drag?.slot ?? null,
    landing: drag?.landing ?? false,
    settling,
    dragging: drag !== null,
    handlers,
  };
}
