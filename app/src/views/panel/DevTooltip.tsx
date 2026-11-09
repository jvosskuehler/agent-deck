// Nur im Browser (Mock): der Tooltip, der in Tauri ein eigenes Fenster ist, erscheint
// hier als Überlagerung unter der Karte. So lässt sich das Zusammenspiel ohne Rust
// anschauen. In Tauri wird diese Komponente nie gerendert.

import { useTooltip } from "../../ipc/hooks";
import { TooltipCard } from "../tooltip/TooltipCard";
import "../../styles/tooltip.css";

export function DevTooltip({ anchor }: { anchor: DOMRect | null }) {
  const tip = useTooltip();
  if (!tip || !anchor) return null;
  return (
    <div className="dev-tip" style={{ left: anchor.left, top: anchor.bottom + 8 }}>
      <TooltipCard tip={tip} />
    </div>
  );
}
