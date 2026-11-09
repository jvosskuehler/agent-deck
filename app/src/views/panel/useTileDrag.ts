// Karten innerhalb eines Blocks per Ziehen umsortieren, mit Pointer-Events.
//
// Ein Druck ohne Bewegung bleibt ein Klick (fokussieren). Ab 8 px Zeigerweg zieht die
// Karte: sie folgt dem Zeiger, die anderen rücken weich zur Seite (CSS-Übergang auf
// transform). Beim Loslassen gleitet sie in ihre Zelle, erst dann wird die neue
// Reihenfolge festgeschrieben; so springt nichts. Pointer-Capture hält die Ereignisse
// bei der Karte, auch wenn der Zeiger sie beim Ziehen verlässt (die Falle, die im
// Python-Deck die Bindung an den Canvas erzwungen hat).

import { useCallback, useRef, useState, type PointerEvent } from "react";
import {
  cellOf,
  gridDropIndex,
  gridReflow,
  moveTo,
  passedThreshold,
  sameOrder,
  type Grid,
  type Point,
} from "../../logic/reorder";

/** Wie lange die losgelassene Karte in ihre Zelle gleitet (ms). */
const LAND_MS = 140;

interface Press {
  slot: string;
  pointerId: number;
  sx: number;
  sy: number;
  moved: boolean;
}

interface DragState {
  slot: string;
  from: number;
  target: number;
  /** Verschiebung der gezogenen Karte gegenüber ihrer Zelle. */
  dx: number;
  dy: number;
  landing: boolean;
}

export interface TileDragOptions {
  order: readonly string[];
  grid: Grid;
  onClick(slot: string): void;
  onDragStart(): void;
  /** Der Zug ist vorbei (abgelegt oder abgebrochen). */
  onDragEnd(): void;
  onCommit(order: string[]): void;
}

export interface TileDrag {
  /** Verschiebung einer Karte (null = an ihrem Platz). */
  offsetOf(slot: string): Point | null;
  /** Die Karte, die gerade gezogen wird. */
  liftedSlot: string | null;
  landing: boolean;
  settling: boolean;
  dragging: boolean;
  onPointerDown(slot: string, e: PointerEvent<HTMLElement>): void;
  onPointerMove(e: PointerEvent<HTMLElement>): void;
  onPointerUp(e: PointerEvent<HTMLElement>): void;
  onPointerCancel(e: PointerEvent<HTMLElement>): void;
}

export function useTileDrag({ order, grid, onClick, onDragStart, onDragEnd, onCommit }: TileDragOptions): TileDrag {
  const press = useRef<Press | null>(null);
  const [drag, setDrag] = useState<DragState | null>(null);
  const dragRef = useRef<DragState | null>(null);
  const [settling, setSettling] = useState(false);

  const update = (d: DragState | null) => {
    dragRef.current = d;
    setDrag(d);
  };

  const onPointerDown = useCallback((slot: string, e: PointerEvent<HTMLElement>) => {
    if (e.button !== 0 || dragRef.current?.landing) return;
    press.current = { slot, pointerId: e.pointerId, sx: e.clientX, sy: e.clientY, moved: false };
    e.currentTarget.setPointerCapture(e.pointerId);
  }, []);

  const onPointerMove = (e: PointerEvent<HTMLElement>) => {
    const p = press.current;
    if (!p || p.pointerId !== e.pointerId) return;
    const dx = e.clientX - p.sx;
    const dy = e.clientY - p.sy;
    if (!p.moved) {
      if (!passedThreshold(dx, dy)) return;
      // Eine einzelne Karte hat keine Reihenfolge: dann bleibt es beim Klick.
      if (order.length < 2 || !order.includes(p.slot)) return;
      p.moved = true;
      onDragStart();
    }
    const from = order.indexOf(p.slot);
    const home = cellOf(from, grid);
    const target = gridDropIndex(order.length, grid, { x: home.x + dx, y: home.y + dy });
    update({ slot: p.slot, from, target, dx, dy, landing: false });
  };

  const finish = (commit: boolean) => {
    const d = dragRef.current;
    if (!d) return;
    const next = moveTo(order, d.slot, d.target);
    const home = cellOf(d.from, grid);
    const cell = commit ? cellOf(d.target, grid) : home;
    // Erst in die Zelle gleiten lassen, dann festschreiben.
    update({ ...d, dx: cell.x - home.x, dy: cell.y - home.y, landing: true });
    window.setTimeout(() => {
      if (commit && !sameOrder(next, order)) onCommit(next);
      update(null);
      onDragEnd();
      // Einen Frame ohne Übergang: die Karten stehen jetzt an ihren neuen Plätzen, und
      // das Zurücksetzen der Verschiebung darf nicht als Bewegung sichtbar werden.
      setSettling(true);
      requestAnimationFrame(() => requestAnimationFrame(() => setSettling(false)));
    }, LAND_MS);
  };

  const onPointerUp = (e: PointerEvent<HTMLElement>) => {
    const p = press.current;
    if (!p || p.pointerId !== e.pointerId) return;
    press.current = null;
    if (!p.moved) {
      onClick(p.slot);
      return;
    }
    finish(true);
  };

  const onPointerCancel = (e: PointerEvent<HTMLElement>) => {
    const p = press.current;
    if (!p || p.pointerId !== e.pointerId) return;
    press.current = null;
    if (p.moved) finish(false);
  };

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
    onPointerDown,
    onPointerMove,
    onPointerUp,
    onPointerCancel,
  };
}
