// Repo-Blöcke per Ziehen am Repo-Namen umsortieren: senkrecht, was die Karten im
// Raster tun.
//
// Der Repo-Name ist zugleich ein Knopf: ein Druck ohne Bewegung holt das Fenster des
// Blocks nach vorn (wie im Python-Deck). Blöcke sind unterschiedlich hoch, darum rechnet
// das Ziel mit den gemessenen Höhen (dropIndex/reflowTops). Der Heimat-Block steht
// immer oben; er zieht nicht mit und lässt sich nicht überholen.

import { useRef, useState, type PointerEvent } from "react";
import { dropIndex, moveTo, passedThreshold, pinFirst, reflowTops, sameOrder, slotTops } from "../../logic/reorder";

const LAND_MS = 140;

interface Press {
  letter: string;
  pointerId: number;
  sx: number;
  sy: number;
  moved: boolean;
}

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

export interface BlockDragOptions {
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

export function useBlockDrag({ letters, home, measure, onClick, onDragStart, onDragEnd, onCommit }: BlockDragOptions) {
  const press = useRef<Press | null>(null);
  const dragRef = useRef<DragState | null>(null);
  const [drag, setDrag] = useState<DragState | null>(null);
  const [settling, setSettling] = useState(false);

  const update = (d: DragState | null) => {
    dragRef.current = d;
    setDrag(d);
  };

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

  const onPointerDown = (letter: string, e: PointerEvent<HTMLElement>) => {
    if (e.button !== 0 || dragRef.current?.landing) return;
    press.current = { letter, pointerId: e.pointerId, sx: e.clientX, sy: e.clientY, moved: false };
    e.currentTarget.setPointerCapture(e.pointerId);
  };

  const onPointerMove = (e: PointerEvent<HTMLElement>) => {
    const p = press.current;
    if (!p || p.pointerId !== e.pointerId) return;
    const dy = e.clientY - p.sy;
    let d = dragRef.current;
    if (!p.moved) {
      if (!passedThreshold(e.clientX - p.sx, dy)) return;
      const geo = begin(p.letter);
      if (!geo) {
        press.current = null;
        return;
      }
      p.moved = true;
      onDragStart();
      const from = geo.order.indexOf(p.letter);
      d = { letter: p.letter, from, target: from, dy: 0, geo, landing: false };
    }
    if (!d) return;
    const { geo, from } = d;
    const target = dropIndex(geo.heights, geo.gap, geo.tops[0], from, geo.tops[from] + dy);
    update({ ...d, dy, target });
  };

  const finish = (commit: boolean) => {
    const d = dragRef.current;
    if (!d) return;
    const { geo } = d;
    const landTop = commit ? slotTops(geo.heights, geo.gap, geo.tops[0], d.from)[d.target] : geo.tops[d.from];
    update({ ...d, dy: landTop - geo.tops[d.from], landing: true });
    window.setTimeout(() => {
      const next = pinFirst([...(home ? [home] : []), ...moveTo(geo.order, d.letter, d.target)], home);
      if (commit && !sameOrder(next, letters)) onCommit(next);
      update(null);
      onDragEnd();
      setSettling(true);
      requestAnimationFrame(() => requestAnimationFrame(() => setSettling(false)));
    }, LAND_MS);
  };

  const onPointerUp = (e: PointerEvent<HTMLElement>) => {
    const p = press.current;
    if (!p || p.pointerId !== e.pointerId) return;
    press.current = null;
    if (!p.moved) {
      onClick(p.letter);
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

  const want = drag ? reflowTops(drag.geo.heights, drag.geo.gap, drag.geo.tops[0], drag.from, drag.target) : null;

  /** Senkrechte Verschiebung eines Blocks (0 = an seinem Platz). */
  const offsetOf = (letter: string): number => {
    if (!drag) return 0;
    if (letter === drag.letter) return drag.dy;
    const i = drag.geo.order.indexOf(letter);
    const to = want?.get(i);
    return to === undefined ? 0 : to - drag.geo.tops[i];
  };

  return {
    offsetOf,
    liftedLetter: drag?.letter ?? null,
    landing: drag?.landing ?? false,
    settling,
    dragging: drag !== null,
    onPointerDown,
    onPointerMove,
    onPointerUp,
    onPointerCancel,
  };
}

export type BlockDrag = ReturnType<typeof useBlockDrag>;
