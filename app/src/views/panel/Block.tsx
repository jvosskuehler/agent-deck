// Ein Repo-Block: Kopf (Repo-Name als Griff, daneben der Host), links die Schiene, die
// Kopf und Karten zu einer Gruppe bindet, darunter das Kartenraster mit der ＋-Karte.
//
// Der Kopf meint das ganze Fenster (connect.py): Rechtsklick öffnet das native Menü
// (Bindung vergessen, mit einem VS-Code-Fenster verbinden), das ✕ beim Hover schließt
// das Fenster nach einer Rückfrage, ebenfalls nativ.

import { forwardRef, type CSSProperties, type PointerEvent } from "react";
import type { BlockView } from "../../bindings/BlockView";
import { t, type Lang } from "../../logic/i18n";
import { CARD_GAP, CARD_W, cardHeight, wrapLimit } from "../../logic/layout";
import { screenCells } from "../../logic/reorder";
import { AddTile } from "./AddTile";
import { Tile, type TileHandlers } from "./Tile";
import { useTileDrag } from "./useTileDrag";

export interface BlockHeadHandlers {
  onPointerDown(letter: string, e: PointerEvent<HTMLElement>): void;
  onPointerMove(e: PointerEvent<HTMLElement>): void;
  onPointerUp(e: PointerEvent<HTMLElement>): void;
  onPointerCancel(e: PointerEvent<HTMLElement>): void;
}

interface Props {
  block: BlockView;
  /** Karten in Anzeigereihenfolge (mit einer eben gezogenen, noch unbestätigten). */
  order: string[];
  cols: number;
  lang: Lang;
  /** Senkrechte Verschiebung beim Umsortieren der Blöcke. */
  offsetY: number;
  lifted: boolean;
  landing: boolean;
  settling: boolean;
  head: BlockHeadHandlers;
  tiles: Omit<TileHandlers, "onPointerDown" | "onPointerMove" | "onPointerUp" | "onPointerCancel" | "onActivate">;
  onFocusSlot(slot: string): void;
  /** Repo-Name per Tastatur ausgelöst (die Maus geht über die Zieh-Logik). */
  onFocusBlock(letter: string): void;
  onTileDragStart(): void;
  onReorder(letter: string, slots: string[]): void;
  onCreate(letter: string): void;
  /** Rechtsklick auf den Kopf. */
  onBlockMenu(letter: string): void;
  /** ✕ im Kopf: Rust fragt nach und schließt dann das ganze Fenster. */
  onCloseBlock(letter: string): void;
  onTileDragEnd(): void;
}

export const Block = forwardRef<HTMLElement, Props>(function Block(props, ref) {
  const { block, order, cols, lang, offsetY, lifted, landing, settling, head } = props;
  const height = cardHeight(block.host);
  // Windows Terminal meldet, wie die Fenster liegen: dann hat jede Karte ihre feste
  // Zelle (3 + 3 Fenster = 3 + 3 Karten), sonst brechen sie dicht um.
  // Die ＋-Karte (und minimierte) laufen dahinter nur so weit, wie auf dem Monitor
  // Fenster nebeneinander passen; danach vorn in der nächsten Reihe.
  const cells = block.rows.length ? screenCells(block.rows, order.length, cols, wrapLimit(block, cols)) : undefined;
  const placeAt = (i: number): CSSProperties | undefined =>
    cells ? { gridRow: Math.floor(cells[i] / cols) + 1, gridColumn: (cells[i] % cols) + 1 } : undefined;
  const drag = useTileDrag({
    order,
    grid: { cols, stepX: CARD_W + CARD_GAP, stepY: height + CARD_GAP, cells },
    onClick: props.onFocusSlot,
    onDragStart: props.onTileDragStart,
    onDragEnd: props.onTileDragEnd,
    onCommit: (slots) => props.onReorder(block.letter, slots),
  });

  const bySlot = new Map(block.tiles.map((x) => [x.slot, x]));
  const host = block.host === "terminal" ? t(lang, "hostTerminal") : t(lang, "hostVscode");
  const note = !block.connected ? t(lang, "notConnected") : block.remote ? t(lang, "remote") : null;

  const style: CSSProperties = {
    transform: offsetY ? `translateY(${offsetY}px)` : undefined,
  };

  return (
    <section
      ref={ref}
      className="block"
      data-letter={block.letter}
      data-host={block.host}
      data-connected={block.connected}
      data-lifted={lifted || undefined}
      data-landing={(lifted && landing) || undefined}
      data-settling={settling || undefined}
      data-tile-drag={drag.dragging || undefined}
      style={style}
    >
      <span className="block-rail" aria-hidden="true" />
      <div
        className="block-head"
        onContextMenu={(e) => {
          e.preventDefault();
          props.onBlockMenu(block.letter);
        }}
      >
        <button
          type="button"
          className="block-name"
          onPointerDown={(e) => head.onPointerDown(block.letter, e)}
          onPointerMove={head.onPointerMove}
          onPointerUp={head.onPointerUp}
          onPointerCancel={head.onPointerCancel}
          // detail 0 = per Tastatur ausgelöst; ein Mausklick ist schon über pointerup gelaufen.
          onClick={(e) => e.detail === 0 && props.onFocusBlock(block.letter)}
        >
          {block.name}
        </button>
        {block.binding ? (
          <span className="block-binding" role="status">
            <span className="block-binding-dot" aria-hidden="true" />
            {t(lang, "binding")}
          </span>
        ) : (
          <span className="block-host">
            {host}
            {note && <span className="block-note"> · {note}</span>}
          </span>
        )}
        {/* Kein ✕ für das ganze Fenster: die Python-Fassung hatte es nie verdrahtet, und ein
            Knopf, der ein ganzes VS-Code-Fenster schließt, gehört nicht ungefragt dazu. */}
      </div>
      <div
        className="block-grid"
        style={{ gridTemplateColumns: `repeat(${cols}, ${CARD_W}px)`, gridAutoRows: `${height}px` }}
      >
        {order.map((slot, i) => {
          const tile = bySlot.get(slot);
          if (!tile) return null;
          return (
            <Tile
              key={slot}
              tile={tile}
              host={block.host}
              lang={lang}
              offset={drag.offsetOf(slot)}
              lifted={drag.liftedSlot === slot}
              landing={drag.landing}
              settling={drag.settling}
              place={placeAt(i)}
              handlers={{
                ...props.tiles,
                onActivate: props.onFocusSlot,
                onPointerDown: drag.onPointerDown,
                onPointerMove: drag.onPointerMove,
                onPointerUp: drag.onPointerUp,
                onPointerCancel: drag.onPointerCancel,
              }}
            />
          );
        })}
        {block.connected && (
          <AddTile
            label={t(lang, "newAgentIn", { name: block.name })}
            onClick={() => props.onCreate(block.letter)}
            place={placeAt(order.length)}
          />
        )}
      </div>
    </section>
  );
});
