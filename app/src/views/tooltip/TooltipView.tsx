// Ansicht `#/tooltip`: eigenes, nie aktives Fenster. Rust setzt Inhalt (deck://tooltip)
// und Lage; die Ansicht meldet ihre Größe zurück, damit Rust sie auf dem Monitor der
// Karte halten kann (tooltip_content_size, Vorschlag, siehe Bericht).

import { useEffect, useRef } from "react";
import { isNative, commands } from "../../ipc";
import { useTooltip } from "../../ipc/hooks";
import { TooltipCard } from "./TooltipCard";
import "../../styles/tooltip.css";

/** Luft um die Karte für den Schatten (px), Teil der gemeldeten Größe. */
export const TIP_GUTTER = 8;

export function TooltipView() {
  const tip = useTooltip();
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    let last = "";
    const report = () => {
      const w = Math.ceil(el.offsetWidth) + 2 * TIP_GUTTER;
      const h = Math.ceil(el.offsetHeight) + 2 * TIP_GUTTER;
      const key = `${w}x${h}`;
      if (key === last) return;
      last = key;
      commands.tooltipContentSize(w, h);
    };
    const ro = new ResizeObserver(report);
    ro.observe(el);
    report();
    return () => ro.disconnect();
  }, [tip?.slot]);

  const content = tip && (
    <div className="tip-window" style={{ padding: TIP_GUTTER }}>
      <TooltipCard ref={ref} tip={tip} />
    </div>
  );

  if (isNative()) return content;
  // Im Browser: auf der Ersatzfläche, mit einem Beispiel, damit es etwas zu sehen gibt.
  return (
    <div className="mock-stage">
      {content ?? (
        <div className="tip-window" style={{ padding: 24 }}>
          <TooltipCard
            tip={{
              slot: "B1",
              references: "ORPH-4062",
              summary:
                "Fragt, ob die Testuhr in der Stripe-Sandbox auf den 1. des Monats vorgestellt werden soll, damit der Gratismonat abläuft.",
              lastPrompt: null,
              origin: ["orpheus-management-api · Fenster B · B1", "↳ wt/orph-4062"],
            }}
          />
        </div>
      )}
    </div>
  );
}
