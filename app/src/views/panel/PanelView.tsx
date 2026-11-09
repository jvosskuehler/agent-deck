// Ansicht `#/panel`: das Deck. Blöcke mit Karten, untere Leiste, Gleiten an der Kante.
//
// Aufbau des Fensters: `.panel-window` ist das Tauri-Fenster (im Browser eine Box auf
// der Ersatzfläche), `.panel-measure` der natürliche Inhalt samt Schattenraum (wird
// gemessen und per panel_content_size gemeldet), `.panel` das sichtbare Panel, das beim
// Ein- und Ausklappen gleitet. Transform ändert die gemessene Größe nicht, darum
// wackelt das Fenster beim Gleiten nicht.
//
// Passt das Panel nicht auf den Monitor oder hat der Nutzer an der Ecke gezogen, zoomt
// Rust die ganze Webview (DockView.zoom); gemessen und gemeldet wird weiter die
// natürliche Größe. Frei schwebend (Kante „off“) zieht ein Druck auf leere Fläche das
// Panel mit, die Ecke unten rechts zoomt; beides führt Rust aus (panel_drag).

import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore, type CSSProperties, type PointerEvent } from "react";
import type { DeckView } from "../../bindings/DeckView";
import { commands, getBackend, isNative } from "../../ipc";
import { useDeckView, useDock } from "../../ipc/hooks";
import { gutterStyle, normalizeEdge, normalizePhase, panelRadius, panelShadow, type Edge } from "../../logic/dock";
import { langFromView, t } from "../../logic/i18n";
import { blockWidth, columnsFor } from "../../logic/layout";
import { createPendingOrder } from "../../logic/pendingOrder";
import { applyOrder, pinFirst } from "../../logic/reorder";
import { Block } from "./Block";
import { BottomBar } from "./BottomBar";
import { DevTooltip } from "./DevTooltip";
import { useBlockDrag } from "./useBlockDrag";
import { usePanelSize } from "./usePanelSize";
import { useSlide } from "./useSlide";
import { useTileTip } from "./useTileTip";
import "../../styles/panel.css";
import "../../styles/block.css";
import "../../styles/bar.css";
import "../../styles/tile.css";

export function PanelView() {
  const view = useDeckView();
  const dock = useDock();
  const native = isNative();
  const edge = normalizeEdge(dock?.edge ?? "top");
  const phase = normalizePhase(edge, dock?.phase ?? "open");
  const lang = langFromView(view?.lang);

  const measureRef = useRef<HTMLDivElement>(null);
  const size = usePanelSize(measureRef);
  const slide = useSlide(edge, phase, dock !== null);

  // Nur im Browser: wo der Mock-Tooltip erscheinen soll.
  const [devTipAnchor, setDevTipAnchor] = useState<DOMRect | null>(null);
  const onTipShown = useCallback((_: string, el: HTMLElement) => setDevTipAnchor(el.getBoundingClientRect()), []);
  const tip = useTileTip(phase === "open", native ? undefined : onTipShown);

  const zoom = dock?.zoom ?? 1;
  useEffect(() => getBackend().setZoom(zoom), [zoom]);

  // Solange eine Karte oder ein Block gezogen wird, klappt das Panel nicht ein, auch
  // wenn der Zeiger es dabei verlässt (wie app._dragging() im Python-Dock).
  const holdStart = useCallback(() => {
    tip.cancel();
    commands.panelHold(true);
  }, [tip.cancel]);
  const holdEnd = useCallback(() => commands.panelHold(false), []);

  const pending = usePendingOrder(view);
  const { tiles: pendingTiles, blocks: pendingBlocks } = pending.state;

  const blocks = view?.blocks ?? [];
  const home = blocks.find((b) => b.home)?.letter ?? null;
  const letters = pinFirst(
    applyOrder(pendingBlocks ?? [], blocks.map((b) => b.letter)),
    home,
  );
  const byLetter = new Map(blocks.map((b) => [b.letter, b]));
  // Zellen je Block: Karten plus ＋-Karte (nur verbundene Blöcke haben eine), die bei
  // Windows Terminal neben der letzten Karte steht, solange der Monitor Platz hätte.
  const cols = columnsFor(blocks.map(blockWidth));

  const blockEls = useRef(new Map<string, HTMLElement>());
  const blockDrag = useBlockDrag({
    letters,
    home,
    measure: (l) => {
      const el = blockEls.current.get(l);
      return el ? { top: el.offsetTop, height: el.offsetHeight } : null;
    },
    onClick: (l) => commands.focusBlock(l),
    onDragStart: holdStart,
    onDragEnd: holdEnd,
    onCommit: (next) => {
      pending.holdBlocks(next);
      commands.reorderBlocks(next);
    },
  });

  const tileHandlers = useMemo(
    () => ({
      onEnter: tip.enter,
      onLeave: tip.leave,
      onMenu: (slot: string) => {
        tip.cancel();
        commands.showTileMenu(slot);
      },
      onClose: (slot: string) => {
        tip.cancel();
        commands.closeSlot(slot);
      },
    }),
    [tip.enter, tip.leave, tip.cancel],
  );

  const panelStyle: CSSProperties = {
    ...slide,
    borderRadius: panelRadius(edge),
    boxShadow: panelShadow(edge),
  };

  const panelWindow = (
    <div
      className="panel-window"
      data-edge={edge}
      data-native={native || undefined}
      style={native ? undefined : mockWindowStyle(edge, size)}
    >
      <div ref={measureRef} className="panel-measure" data-edge={edge} style={gutterStyle(edge)}>
        <main
          className="panel"
          data-edge={edge}
          data-phase={phase}
          data-block-drag={blockDrag.dragging || undefined}
          aria-label="Agent Deck"
          style={panelStyle}
          onPointerDown={edge === "off" ? startMove : undefined}
        >
          <div className="blocks">
            {letters.map((letter) => {
              const block = byLetter.get(letter);
              if (!block) return null;
              const order = applyOrder(pendingTiles[letter] ?? [], block.tiles.map((x) => x.slot));
              return (
                <Block
                  key={letter}
                  ref={(el) => {
                    if (el) blockEls.current.set(letter, el);
                    else blockEls.current.delete(letter);
                  }}
                  block={block}
                  order={order}
                  cols={cols}
                  lang={lang}
                  shift={blockDrag.shift}
                  lifted={blockDrag.liftedLetter === letter}
                  landing={blockDrag.landing}
                  settling={blockDrag.settling}
                  head={blockDrag.handlers}
                  tiles={tileHandlers}
                  onFocusSlot={commands.focusSlot}
                  onFocusBlock={commands.focusBlock}
                  onTileDragStart={holdStart}
                  onTileDragEnd={holdEnd}
                  onBlockMenu={(l) => {
                    tip.cancel();
                    commands.showBlockMenu(l);
                  }}
                  onReorder={(l, slots) => {
                    pending.holdTiles(l, slots);
                    commands.reorderTiles(l, slots);
                  }}
                  onCreate={commands.createAgent}
                />
              );
            })}
            {view && blocks.length === 0 && <p className="panel-empty">{t(lang, "waitingForAgents")}</p>}
          </div>
          <BottomBar
            usage={view?.usage ?? null}
            lang={lang}
            onOpenUsage={commands.openUsagePage}
            onOpenSettings={commands.openSettings}
            orphans={view?.orphans ?? []}
            onKillOrphan={commands.killOrphan}
            onKillAllOrphans={commands.killAllOrphans}
          />
          {edge === "off" && (
            <span
              className="panel-grip"
              title={t(lang, "zoomGrip")}
              aria-hidden="true"
              onPointerDown={(e) => {
                if (e.button !== 0) return;
                e.preventDefault();
                e.stopPropagation();
                tip.cancel();
                commands.panelDrag("zoom", e.clientX, e.clientY);
              }}
            >
              <svg viewBox="0 0 10 10" width="10" height="10">
                <path d="M9 3L3 9M9 6.5L6.5 9" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
              </svg>
            </span>
          )}
        </main>
      </div>
    </div>
  );

  if (native) return panelWindow;
  return (
    <div className="mock-stage">
      {panelWindow}
      <DevTooltip anchor={devTipAnchor} />
    </div>
  );
}

/**
 * Frei schwebend: ein Druck auf leere Fläche verschiebt das Panel (Rust folgt dem
 * Zeiger). Karten, Knöpfe und der Repo-Name behalten ihre eigene Bedeutung.
 */
function startMove(e: PointerEvent<HTMLElement>) {
  if (e.button !== 0) return;
  const target = e.target as Element;
  if (target.closest("button, input, [role='button'], .tile, .usage-tip, .orphans-pop, .panel-grip")) return;
  e.preventDefault();
  commands.panelDrag("move", e.clientX, e.clientY);
}

/**
 * Im Browser vertritt eine Box das Tauri-Fenster: so groß, wie das Panel meldet, an
 * der Kante der Ersatzfläche. Sie schneidet ab wie das echte Fenster, damit das Gleiten
 * aussieht wie später auf dem Bildschirm.
 */
function mockWindowStyle(edge: Edge, size: { width: number; height: number } | null): CSSProperties {
  const base: CSSProperties = {
    position: "absolute",
    width: size?.width ?? "auto",
    height: size?.height ?? "auto",
  };
  switch (edge) {
    case "top":
      return { ...base, left: 90, top: 0 };
    case "bottom":
      return { ...base, left: 90, bottom: 0 };
    case "left":
      return { ...base, left: 0, top: 40 };
    case "right":
      return { ...base, right: 0, top: 40 };
    case "off":
      return { ...base, left: 90, top: 40 };
  }
}

/**
 * Die eben gezogene Reihenfolge, bis Rust sie bestätigt oder die Frist abläuft
 * (logic/pendingOrder.ts).
 */
function usePendingOrder(view: DeckView | null) {
  const [order] = useState(createPendingOrder);
  const state = useSyncExternalStore(order.subscribe, order.get);
  useEffect(() => {
    if (view) order.confirm(view);
  }, [order, view]);
  useEffect(() => () => order.dispose(), [order]);
  return { state, holdTiles: order.holdTiles, holdBlocks: order.holdBlocks };
}
