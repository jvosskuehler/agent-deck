// Der Inhalt des Hover-Tooltips: ganz oben klein die Herkunft (Repo · Fenster · Slot,
// bei einem worktree „↳ wt/…“, wie hover.py), dann Ticket und PR (violett, je mit
// Herkunft: zugewiesen, Branch, Ordner, Claude Code, Titel, Chat; Geratenes gedimmt),
// darunter die Kurzform des Lagebilds (Ziel, in einer zweiten Zeile die offene Frage,
// wenn der Agent auf eine Antwort wartet); solange es rechnet, gedämpft die letzte Frage.

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
      {(tip.ticket || tip.pr) && (
        <div className="tip-refs">
          {[tip.ticket, tip.pr].map(
            (r) =>
              r && (
                <span key={r.text} className={r.detected ? "tip-ref tip-ref--detected" : "tip-ref"}>
                  {r === tip.pr ? `PR ${r.text}` : r.text} <span className="tip-ref-origin">{r.origin}</span>
                </span>
              ),
          )}
        </div>
      )}
      {body && (
        <div className={pending ? "tip-body tip-body--pending" : "tip-body"}>
          {pending && <span className="tip-pulse" aria-hidden="true" />}
          {pending
            ? body
            : body.split("\n").map((line, i) => (
                <div key={i} className={i > 0 ? "tip-line tip-line--waiting" : "tip-line"}>
                  {line}
                </div>
              ))}
        </div>
      )}
    </div>
  );
});
