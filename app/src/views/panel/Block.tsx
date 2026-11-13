// Ein Repo-Block: Kopf (Repo-Name als Griff), darunter das Kartenraster mit der ＋-Karte.
//
// Der Kopf meint den ganzen Block (connect.py): Klick holt seine Fenster nach vorn,
// Rechtsklick öffnet das native Menü (Block vergessen). Ein ✕ für den ganzen Block gibt
// es bewusst nicht (docs/DESIGN.md).

import { forwardRef, useMemo, type CSSProperties } from "react";
import type { BlockView } from "../../bindings/BlockView";
import { t, type Lang } from "../../logic/i18n";
import { CARD_GAP, CARD_H, CARD_W, gridVars, placeAt, wrapLimit } from "../../logic/layout";
import { screenCells } from "../../logic/reorder";
import { AddTile } from "./AddTile";
import { Tile, type TileHandlers } from "./Tile";
import { useBlockOffset, type BlockShift } from "./useBlockDrag";
import type { DragHandlers } from "./useDragSession";
import { useTileDrag } from "./useTileDrag";

interface Props {
  block: BlockView;
  /** Karten in Anzeigereihenfolge (mit einer eben gezogenen, noch unbestätigten). */
  order: string[];
  cols: number;
  lang: Lang;
  /** Senkrechte Verschiebung beim Umsortieren der Blöcke; jeder Block liest seine selbst. */
  shift: BlockShift;
  lifted: boolean;
  landing: boolean;
  settling: boolean;
  head: DragHandlers;
  tiles: Omit<TileHandlers, keyof DragHandlers | "onActivate">;
  onFocusSlot(slot: string): void;
  /** Repo-Name per Tastatur ausgelöst (die Maus geht über die Zieh-Logik). */
  onFocusBlock(letter: string): void;
  onTileDragStart(): void;
  onReorder(letter: string, slots: string[]): void;
  onCreate(letter: string): void;
  /** Rechtsklick auf den Kopf. */
  onBlockMenu(letter: string): void;
  onTileDragEnd(): void;
}

export const Block = forwardRef<HTMLElement, Props>(function Block(props, ref) {
  const { block, order, cols, lang, shift, lifted, landing, settling, head } = props;
  const offsetY = useBlockOffset(shift, block.letter);
  // Rust meldet, wie die Fenster liegen: dann hat jede Karte ihre feste
  // Zelle (3 + 3 Fenster = 3 + 3 Karten), sonst brechen sie dicht um.
  // Die ＋-Karte (und minimierte) laufen dahinter nur so weit, wie auf dem Monitor
  // Fenster nebeneinander passen; danach vorn in der nächsten Reihe.
  const cells = block.rows.length ? screenCells(block.rows, order.length, cols, wrapLimit(block, cols)) : undefined;
  const drag = useTileDrag({
    order,
    grid: { cols, stepX: CARD_W + CARD_GAP, stepY: CARD_H + CARD_GAP, cells },
    onClick: props.onFocusSlot,
    onDragStart: props.onTileDragStart,
    onDragEnd: props.onTileDragEnd,
    onCommit: (slots) => props.onReorder(block.letter, slots),
  });

  // Ein Handler-Objekt für alle Karten des Blocks, stabil über die Renders: sonst
  // zeichnete jede Zeigerbewegung jede Karte neu (Tile ist memo).
  const handlers = useMemo<TileHandlers>(
    () => ({ ...props.tiles, onActivate: props.onFocusSlot, ...drag.handlers }),
    [props.tiles, props.onFocusSlot, drag.handlers],
  );

  const bySlot = new Map(block.tiles.map((x) => [x.slot, x]));

  const style: CSSProperties = {
    transform: offsetY ? `translateY(${offsetY}px)` : undefined,
  };

  return (
    <section
      ref={ref}
      className="block"
      data-letter={block.letter}
      data-connected={block.connected}
      data-lifted={lifted || undefined}
      data-landing={(lifted && landing) || undefined}
      data-settling={settling || undefined}
      data-tile-drag={drag.dragging || undefined}
      style={style}
    >
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
        {!block.connected && <span className="block-note">{t(lang, "notConnected")}</span>}
        {/* Kein ✕ für den ganzen Block: die Python-Fassung hatte es nie verdrahtet, und ein
            Knopf, der alle Agenten eines Repos schließt, gehört nicht ungefragt dazu. */}
      </div>
      <div className="block-grid" style={gridVars(cols) as CSSProperties}>
        {order.map((slot, i) => {
          const tile = bySlot.get(slot);
          if (!tile) return null;
          return (
            <Tile
              key={slot}
              tile={tile}
              lang={lang}
              offset={drag.offsetOf(slot)}
              lifted={drag.liftedSlot === slot}
              landing={drag.landing}
              settling={drag.settling}
              place={placeAt(cells, i, cols)}
              handlers={handlers}
            />
          );
        })}
        {block.connected && (
          <AddTile
            label={t(lang, "newAgentIn", { name: block.name })}
            onClick={() => props.onCreate(block.letter)}
            place={placeAt(cells, order.length, cols)}
          />
        )}
      </div>
    </section>
  );
});
