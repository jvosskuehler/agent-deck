// Das Gleiten aus der Kante und zurück.
//
// Der Übergang muss im selben Bild gesetzt werden wie das neue Ziel, sonst springt das
// Panel. Darum wird der Zustand während des Renderns abgeleitet (setState im Render,
// das React für genau diesen Fall vorsieht), nicht erst in einem Effekt.
//
// Deadline: nach Dauer + 80 ms wird der Übergang abgeschaltet. Lief er bis dahin nicht
// zu Ende (etwa weil das Fenster verborgen war und die Webview gedrosselt hat), steht
// das Panel damit sofort am Ziel. Ein halb offenes Deck gibt es so nicht.

import { useEffect, useState, type CSSProperties } from "react";
import { slideTiming, slideTransform, type Edge, type Phase } from "../../logic/dock";

interface Shown {
  edge: Edge;
  phase: Phase;
  ready: boolean;
  animate: boolean;
}

export function useSlide(edge: Edge, phase: Phase, ready: boolean): CSSProperties {
  const [shown, setShown] = useState<Shown>({ edge, phase, ready, animate: false });

  if (shown.edge !== edge || shown.phase !== phase || shown.ready !== ready) {
    // Nur ein Phasenwechsel an derselben Kante gleitet. Der allererste Stand und ein
    // Kantenwechsel stehen sofort da.
    const animate = shown.ready && ready && shown.edge === edge && shown.phase !== phase;
    setShown({ edge, phase, ready, animate });
  }

  const timing = slideTiming(shown.phase);
  useEffect(() => {
    if (!shown.animate) return;
    const id = window.setTimeout(() => setShown((s) => ({ ...s, animate: false })), timing.durationMs + 80);
    return () => window.clearTimeout(id);
  }, [shown, timing.durationMs]);

  return {
    transform: slideTransform(shown.edge, shown.phase),
    transition: shown.animate ? `transform ${timing.durationMs}ms ${timing.easing}` : "none",
  };
}
