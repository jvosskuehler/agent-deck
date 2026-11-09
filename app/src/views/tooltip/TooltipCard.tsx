// Der Inhalt des Hover-Tooltips: ganz oben klein die Herkunft (Repo · Fenster · Slot,
// bei einem worktree „↳ wt/…“, wie hover.py), dann Ticket/PR (violett), darunter die
// KI-Zusammenfassung; solange sie rechnet, gedämpft die letzte Frage.

import { forwardRef } from "react";
import type { TooltipView } from "../../bindings/TooltipView";

export const TooltipCard = forwardRef<HTMLDivElement, { tip: TooltipView }>(function TooltipCard({ tip }, ref) {
  const pending = tip.summary === null;
  const body = tip.summary ?? tip.lastPrompt;
  return (
    <div ref={ref} className="tip-card" role="tooltip" data-slot={tip.slot}>
      {tip.origin.length > 0 && (
        <div className="tip-origin">
          {tip.origin.map((line) => (
            <div key={line} className={line.startsWith("↳") ? "tip-origin-line tip-origin-line--wt" : "tip-origin-line"}>
              {line}
            </div>
          ))}
        </div>
      )}
      {tip.references && <div className="tip-refs">{tip.references}</div>}
      {body && (
        <div className={pending ? "tip-body tip-body--pending" : "tip-body"}>
          {pending && <span className="tip-pulse" aria-hidden="true" />}
          {body}
        </div>
      )}
    </div>
  );
});
