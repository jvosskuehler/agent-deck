// Die gemeinsame Mechanik hinter dem Umsortieren von Karten und Blöcken, mit
// Pointer-Events.
//
// Ein Druck ohne Bewegung bleibt ein Klick. Ab DRAG_THRESHOLD Zeigerweg wird daraus ein
// Zug: das gezogene Element folgt dem Zeiger, die anderen rücken weich zur Seite (CSS-
// Übergang auf transform). Beim Loslassen gleitet es in LAND_MS an sein Ziel, erst dann
// wird die neue Reihenfolge festgeschrieben; so springt nichts. Pointer-Capture hält die
// Ereignisse beim Element, auch wenn der Zeiger es beim Ziehen verlässt (die Falle, die
// im Python-Deck die Bindung an den Canvas erzwungen hat).
//
// Was ein Zug ist und wohin er führt, sagt die Spezifikation (Karten: Raster, Blöcke:
// Stapel; Geometrie in logic/reorder.ts). Der Zugstand liegt in einem kleinen Speicher
// statt in React-State: wer ihn braucht, abonniert genau den Teil, der ihn betrifft
// (useDragSelect). So zeichnet eine Zeigerbewegung nur neu, was sich wirklich bewegt.

import { useLayoutEffect, useRef, useState, useSyncExternalStore, type PointerEvent } from "react";
import { passedThreshold } from "../../logic/reorder";

/** Wie lange das losgelassene Element an sein Ziel gleitet (ms). */
const LAND_MS = 140;

interface Landable {
  /** Das Element gleitet nach dem Loslassen an sein Ziel. */
  landing: boolean;
}

interface DragSpec<S extends Landable> {
  /** Aus dem Druck auf <key> wird ein Zug; null = es gibt nichts umzusortieren. */
  start(key: string): S | null;
  /**
   * Was aus einem Druck wird, dessen Zug start abgelehnt hat: weiter ein Klick beim
   * Loslassen („click“) oder gar nichts („ignore“).
   */
  refused: "click" | "ignore";
  /** Neuer Zugstand bei <dx>, <dy> Zeigerweg seit dem Druck. */
  move(drag: S, dx: number, dy: number): S;
  /**
   * Zugstand beim Hinübergleiten (ans Ziel oder, abgebrochen, zurück) und was danach
   * festzuschreiben ist. Beides wird beim Loslassen bestimmt, nicht erst danach.
   */
  land(drag: S, commit: boolean): { state: S; done(): void };
  onClick(key: string): void;
  onDragStart(): void;
  /** Der Zug ist vorbei (abgelegt oder abgebrochen). */
  onDragEnd(): void;
}

export interface DragHandlers {
  onPointerDown(key: string, e: PointerEvent<HTMLElement>): void;
  onPointerMove(e: PointerEvent<HTMLElement>): void;
  onPointerUp(e: PointerEvent<HTMLElement>): void;
  onPointerCancel(e: PointerEvent<HTMLElement>): void;
}

/** Der laufende Zug zum Abonnieren; bleibt über alle Renders dasselbe Objekt. */
export interface DragStore<S> {
  get(): S | null;
  subscribe(listener: () => void): () => void;
}

interface DragSession<S> {
  store: DragStore<S>;
  /** Nach dem Ablegen einen Frame ohne Übergang, damit nichts zurückgleitet. */
  settling: boolean;
  /** Bleiben über alle Renders dieselben (für memo-Kinder). */
  handlers: DragHandlers;
}

interface Press {
  key: string;
  pointerId: number;
  sx: number;
  sy: number;
  moved: boolean;
}

function createStore<S>(): DragStore<S> & { set(next: S | null): void } {
  let value: S | null = null;
  const listeners = new Set<() => void>();
  return {
    get: () => value,
    set(next) {
      value = next;
      listeners.forEach((l) => l());
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
}

export function useDragSession<S extends Landable>(spec: DragSpec<S>): DragSession<S> {
  // Die Handler bleiben stabil und lesen die Spezifikation des letzten Renders (aktuelle
  // Reihenfolge, Raster, Rückrufe), wie es frische Closures je Render täten.
  const specRef = useRef(spec);
  useLayoutEffect(() => {
    specRef.current = spec;
  });
  const [store] = useState(() => createStore<S>());
  const [settling, setSettling] = useState(false);

  const [handlers] = useState<DragHandlers>(() => {
    let press: Press | null = null;

    const finish = (commit: boolean) => {
      const d = store.get();
      if (!d) return;
      const { state, done } = specRef.current.land(d, commit);
      store.set(state);
      window.setTimeout(() => {
        done();
        store.set(null);
        specRef.current.onDragEnd();
        // Einen Frame ohne Übergang: die Elemente stehen jetzt an ihren neuen Plätzen,
        // und das Zurücksetzen der Verschiebung darf nicht als Bewegung sichtbar werden.
        setSettling(true);
        requestAnimationFrame(() => requestAnimationFrame(() => setSettling(false)));
      }, LAND_MS);
    };

    /** Der Druck zu diesem Ereignis, sonst null (anderer Zeiger oder kein Druck). */
    const pressOf = (e: PointerEvent<HTMLElement>) => (press && press.pointerId === e.pointerId ? press : null);

    return {
      onPointerDown(key, e) {
        if (e.button !== 0 || store.get()?.landing) return;
        press = { key, pointerId: e.pointerId, sx: e.clientX, sy: e.clientY, moved: false };
        e.currentTarget.setPointerCapture(e.pointerId);
      },
      onPointerMove(e) {
        const p = pressOf(e);
        if (!p) return;
        const dx = e.clientX - p.sx;
        const dy = e.clientY - p.sy;
        const s = specRef.current;
        let d = store.get();
        if (!p.moved) {
          if (!passedThreshold(dx, dy)) return;
          const first = s.start(p.key);
          if (!first) {
            if (s.refused === "ignore") press = null;
            return;
          }
          p.moved = true;
          s.onDragStart();
          d = first;
        }
        if (!d) return;
        store.set(s.move(d, dx, dy));
      },
      onPointerUp(e) {
        const p = pressOf(e);
        if (!p) return;
        press = null;
        if (!p.moved) {
          specRef.current.onClick(p.key);
          return;
        }
        finish(true);
      },
      onPointerCancel(e) {
        const p = pressOf(e);
        if (!p) return;
        press = null;
        if (p.moved) finish(false);
      },
    };
  });

  return { store, settling, handlers };
}

/**
 * Den Teil des Zugs, den eine Komponente braucht. Sie zeichnet nur neu, wenn sich
 * dieser Teil ändert (Vergleich mit Object.is, also Zahlen, Strings oder ein stabiles
 * Objekt zurückgeben).
 */
export function useDragSelect<S, R>(store: DragStore<S>, select: (drag: S | null) => R): R {
  return useSyncExternalStore(store.subscribe, () => select(store.get()));
}
