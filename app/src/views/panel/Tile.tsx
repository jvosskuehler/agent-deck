// Eine Agentenkarte. Aufteilung nach karte-aufteilung-deck05.html, Fläche nach dem
// Papier-Look „02 Farbfeld“ (docs/DESIGN.md).
//
// Terminal-Karte (hoch): Status | Modell · Effort, darunter der Sessiontitel, unten
// Ticket PR | Cache-Restzeit | Modus. VS-Code-Karte (flach): dieselben zwei Zeilen ohne
// Titel.

import { memo, useEffect, useRef, useState, type CSSProperties, type PointerEvent } from "react";
import type { Host } from "../../bindings/Host";
import type { TileView } from "../../bindings/TileView";
import { propsEqual } from "../../logic/equal";
import { t, type Lang } from "../../logic/i18n";
import type { GridPlace } from "../../logic/layout";
import type { Point } from "../../logic/reorder";
import {
  bloomFill,
  liftShadow,
  metaLine,
  modeColor,
  selectedRing,
  shouldBloom,
  syncedDelay,
  tileLook,
  type StatusStamp,
} from "../../logic/status";
import { useScheme } from "../useScheme";

export interface TileHandlers {
  onPointerDown(slot: string, e: PointerEvent<HTMLElement>): void;
  onPointerMove(e: PointerEvent<HTMLElement>): void;
  onPointerUp(e: PointerEvent<HTMLElement>): void;
  onPointerCancel(e: PointerEvent<HTMLElement>): void;
  onEnter(slot: string, el: HTMLElement): void;
  onLeave(slot: string): void;
  onMenu(slot: string): void;
  onClose(slot: string): void;
  /** Enter oder Leertaste auf der fokussierten Karte (wie ein Klick). */
  onActivate(slot: string): void;
}

interface Props {
  tile: TileView;
  host: Host;
  lang: Lang;
  /** Verschiebung beim Umsortieren (null = an seinem Platz). */
  offset: Point | null;
  /** Diese Karte wird gerade gezogen (folgt dem Zeiger ohne Verzögerung). */
  lifted: boolean;
  /** Die gezogene Karte gleitet nach dem Loslassen in ihre Zelle. */
  landing: boolean;
  /** Nach dem Ablegen einen Frame ohne Übergang, damit nichts zurückgleitet. */
  settling: boolean;
  handlers: TileHandlers;
  /** Fester Platz im Raster (Windows Terminal: wie das Fenster auf dem Bildschirm). */
  place?: GridPlace;
}

function TileCard({ tile, host, lang, offset, lifted, landing, settling, handlers, place }: Props) {
  const scheme = useScheme();
  const look = tileLook(tile.status, scheme);
  const [bloomKey, endBloom] = useBloom({ status: tile.status, changedAt: tile.statusChangedAt });
  const [breatheDelay] = useState(() => syncedDelay(performance.now()));
  const tall = host === "terminal";
  const meta = metaLine(tile.model, tile.effort);

  const style = {
    ...place,
    "--tint": look.tint,
    boxShadow: [tile.selected ? selectedRing(scheme) : null, lifted && !landing ? liftShadow(scheme) : null]
      .filter(Boolean)
      .join(", ") || "none",
    opacity: look.matte ? 0.55 : 1,
    transform: offset ? `translate(${offset.x}px, ${offset.y}px)${lifted ? " scale(1.02)" : ""}` : undefined,
  } as CSSProperties;

  return (
    <div
      className={`tile ${tall ? "tile--tall" : "tile--flat"}`}
      data-slot={tile.slot}
      data-status={tile.status}
      data-lifted={lifted || undefined}
      data-landing={(lifted && landing) || undefined}
      data-settling={settling || undefined}
      style={style}
      role="button"
      tabIndex={0}
      aria-label={`${tile.slot} ${tile.statusLabel}${tile.title ? `: ${tile.title}` : ""}`}
      onPointerDown={(e) => handlers.onPointerDown(tile.slot, e)}
      onPointerMove={handlers.onPointerMove}
      onPointerUp={handlers.onPointerUp}
      onPointerCancel={handlers.onPointerCancel}
      onPointerEnter={(e) => handlers.onEnter(tile.slot, e.currentTarget)}
      onPointerLeave={() => handlers.onLeave(tile.slot)}
      onContextMenu={(e) => {
        e.preventDefault();
        handlers.onMenu(tile.slot);
      }}
      onKeyDown={(e) => {
        if (e.target !== e.currentTarget) return;
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          handlers.onActivate(tile.slot);
        }
      }}
    >
      <span
        aria-hidden="true"
        className={`tile-pulse${look.breathe ? " breathe-fill" : ""}`}
        style={{ background: look.pulse, animationDelay: look.breathe ? breatheDelay : undefined }}
      />
      {bloomKey !== null && (
        <span
          key={bloomKey}
          aria-hidden="true"
          className="tile-bloom"
          style={{ background: bloomFill(tile.status, scheme) }}
          onAnimationEnd={endBloom}
        />
      )}
      <span className="tile-hover" aria-hidden="true" />

      <span className="tile-row">
        <span className="tile-status" style={{ color: look.ink }}>
          {tile.statusLabel}
        </span>
        <span className="tile-spacer" />
        {meta && <span className="tile-meta">{meta}</span>}
      </span>

      {tall && <span className="tile-title">{tile.title ?? ""}</span>}

      {/* Fußzeile „A“: Ticket und PR | Cache-Restzeit | Modus. Alle drei Plätze stehen
          immer da (auch leer), damit die Zeit nicht springt, wenn links etwas fehlt. */}
      <span className="tile-row tile-row--foot">
        <span className="tile-ref">
          {tile.ticket && (
            <span className={`tile-ticket${tile.ticket.detected ? " tile-ref--detected" : ""}`}>{tile.ticket.text}</span>
          )}
          {tile.ticket && tile.pr && " "}
          {tile.pr && <span className={`tile-pr${tile.pr.detected ? " tile-ref--detected" : ""}`}>{tile.pr.text}</span>}
        </span>
        <span className={tile.cache ? `tile-cache tile-cache--${tile.cache.stage}` : "tile-cache"}>{tile.cache?.text}</span>
        <span className="tile-mode" style={tile.mode ? { color: modeColor(tile.mode, scheme) } : undefined}>
          {tile.mode}
        </span>
      </span>

      <button
        type="button"
        className="tile-close"
        aria-label={t(lang, "closeAgent")}
        title={t(lang, "closeAgent")}
        // Ein Druck auf das ✕ ist weder Klick auf die Karte noch Beginn eines Ziehens.
        onPointerDown={(e) => e.stopPropagation()}
        onClick={(e) => {
          e.stopPropagation();
          handlers.onClose(tile.slot);
        }}
      >
        <svg viewBox="0 0 10 10" width="8" height="8" aria-hidden="true">
          <path d="M1.5 1.5l7 7M8.5 1.5l-7 7" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
        </svg>
      </button>
    </div>
  );
}

/**
 * Eine Karte zeichnet nur neu, wenn sich etwas an ihr ändert: nicht bei jedem Poll, der
 * eine andere Karte betrifft, und beim Ziehen nur, wenn sie selbst rückt (propsEqual
 * vergleicht Karte, Verschiebung, Platz und Handler nach Inhalt).
 */
export const Tile = memo(TileCard, propsEqual);

/**
 * Merkt sich den vorigen Stand der Karte und liefert einen Schlüssel, solange sie
 * aufblüht (null sonst). Die Karte bleibt beim Neuzeichnen dieselbe React-Instanz, ihr
 * Gedächtnis überlebt also jeden Poll; eine frisch eingehängte Karte blüht nicht auf.
 * Ein neuer Schlüssel startet die Animation neu, ihr Ende räumt sie ab.
 */
function useBloom(stamp: StatusStamp): [number | null, () => void] {
  const prev = useRef<StatusStamp | undefined>(undefined);
  const [key, setKey] = useState<number | null>(null);
  useEffect(() => {
    const last = prev.current;
    prev.current = stamp;
    if (shouldBloom(last, stamp)) setKey(stamp.changedAt);
    // Nur ein neuer Zeitstempel oder Zustand zählt, nicht jedes neue Objekt.
  }, [stamp.changedAt, stamp.status]);
  return [key, () => setKey(null)];
}
