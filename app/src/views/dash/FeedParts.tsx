// Bausteine, die der PR-Feed (FeedPane) und das Kommentar-Fenster (CommentView) teilen:
// Avatar, Knopf mit Rückfrage, Knöpfe einer Karte „Braucht dich“ und der Antwort-Entwurf.
// Alles, was nach außen wirkt (Agent starten, Antwort senden), fragt mit einem zweiten
// Klick nach; Rust prüft danach selbst noch einmal.

import { useEffect, useState } from "react";
import type { DraftView } from "../../bindings/DraftView";
import type { NeedCard } from "../../bindings/NeedCard";
import { initialsOf } from "../../logic/feedTime";
import type { Lang } from "../../logic/i18n";
import "../../styles/feedParts.css";

export const tr = (lang: Lang, de: string, en: string) => (lang === "en" ? en : de);

export interface FeedActions {
  onOpen(url: string): void;
  onAction(command: string): void;
}

/** Kreis mit Kürzel. */
export function Avatar({ card, size }: { card: NeedCard; size: number }) {
  return (
    <span className="favatar" style={{ width: size, height: size, fontSize: size <= 24 ? 11 : 12 }} aria-hidden="true">
      {initialsOf(card.initials, card.login)}
    </span>
  );
}

/**
 * Knopf mit Rückfrage: der erste Klick zeigt `confirm`, der zweite führt aus; nach 4 s
 * ohne zweiten Klick gilt wieder der erste. `kind` wählt das Aussehen.
 */
export function ArmButton({
  label,
  confirm,
  kind,
  disabled,
  title,
  onConfirm,
}: {
  label: string;
  confirm: string;
  kind: "primary" | "plain";
  disabled?: boolean;
  title?: string;
  onConfirm(): void;
}) {
  const [armed, setArmed] = useState(false);
  useEffect(() => {
    if (!armed) return;
    const id = window.setTimeout(() => setArmed(false), 4000);
    return () => window.clearTimeout(id);
  }, [armed]);
  return (
    <button
      type="button"
      className="fbtn"
      data-kind={kind}
      data-armed={armed ? "" : undefined}
      disabled={disabled}
      title={title}
      onClick={(e) => {
        e.stopPropagation();
        if (armed) {
          setArmed(false);
          onConfirm();
        } else {
          setArmed(true);
        }
      }}
    >
      {armed ? confirm : label}
    </button>
  );
}

/** Warum „Claude soll umsetzen“ gerade nicht geht. */
function delegateBlocked(card: NeedCard, lang: Lang): string {
  if (card.agent) return card.agent;
  if (card.draft) return tr(lang, "Entwurf liegt bereit", "Draft is ready");
  return tr(lang, "Gerade nicht möglich", "Not possible right now");
}

/** Die drei Knöpfe einer Karte: umsetzen lassen, öffnen, später. */
export function CardButtons({ card, lang, onOpen, onAction, afterOpen }: { card: NeedCard; lang: Lang; afterOpen?(): void } & FeedActions) {
  return (
    <>
      <ArmButton
        label={tr(lang, "Claude soll umsetzen", "Let Claude do it")}
        confirm={tr(lang, "Wirklich starten?", "Start it?")}
        kind="primary"
        disabled={!card.canDelegate}
        title={card.canDelegate ? tr(lang, "Hintergrund-Agent setzt um und schreibt einen Antwort-Entwurf", "A background agent implements it and drafts a reply") : delegateBlocked(card, lang)}
        onConfirm={() => onAction(`delegate:${card.key}`)}
      />
      <button
        type="button"
        className="fbtn"
        data-kind="plain"
        onClick={() => {
          onOpen(card.url);
          afterOpen?.();
        }}
      >
        {tr(lang, "Öffnen", "Open")}
      </button>
      <button type="button" className="fbtn" data-kind="quiet" title={tr(lang, "Ausblenden bis zum nächsten Kommentar", "Hide until the next comment")} onClick={() => onAction(`later:${card.key}`)}>
        {tr(lang, "Später", "Later")}
      </button>
    </>
  );
}

/** Der Antwort-Entwurf des Hintergrund-Agenten mit „Absenden“ und „Ändern“. */
export function DraftBlock({ cardKey, draft, lang, onAction, compact }: { cardKey: string; draft: DraftView; lang: Lang; compact?: boolean; onAction(command: string): void }) {
  const send = draft.thread ? tr(lang, "Absenden und auflösen", "Send and resolve") : tr(lang, "Absenden", "Send");
  return (
    <div className="fdraft" data-compact={compact ? "" : undefined}>
      {!compact && <div className="fdraft-head">{tr(lang, "Entwurf von Claude", "Draft by Claude")}</div>}
      <div className="fdraft-text">{draft.text}</div>
      {draft.error && <div className="fdraft-error">{draft.error}</div>}
      <div className="fdraft-acts">
        {draft.sending ? (
          <span className="fbtn" data-kind="plain" data-busy="">
            {tr(lang, "wird gesendet…", "sending…")}
          </span>
        ) : (
          <ArmButton label={send} confirm={tr(lang, "Wirklich senden?", "Really send?")} kind="primary" onConfirm={() => onAction(`draftSend:${cardKey}`)} />
        )}
        <button type="button" className="fbtn" data-kind="plain" title={tr(lang, "Öffnet den Entwurf im Editor", "Opens the draft in the editor")} onClick={() => onAction(`draftEdit:${cardKey}`)}>
          {tr(lang, "Ändern", "Edit")}
        </button>
      </div>
    </div>
  );
}
