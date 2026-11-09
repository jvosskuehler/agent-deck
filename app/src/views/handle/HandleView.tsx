// Ansicht `#/handle`: im eingeklappten Zustand bleibt nur der Griff, eine Kapsel 120 × 7
// in der Farbe des dringlichsten Zustands, mit leichtem Schein; sie atmet bei Rückfrage
// und denkt. Alles ruhig: gedämpftes Grau der Palette ohne Schein.
//
// Zeiger auf den Griff klappt das Panel aus; das entscheidet Rust über die Zeiger-
// position, die Ansicht zeichnet nur. Die Kapsel liegt an der Kante (3 px Luft), der
// Rest des Fensters ist Raum für den Schein.

import { useState } from "react";
import { isNative } from "../../ipc";
import { useDeckView, useDock } from "../../ipc/hooks";
import { normalizeEdge } from "../../logic/dock";
import { handleLook, syncedDelay } from "../../logic/status";
import { useScheme } from "../useScheme";
import "../../styles/handle.css";

export function HandleView() {
  const view = useDeckView();
  const dock = useDock();
  const edge = normalizeEdge(dock?.edge ?? "top");
  const scheme = useScheme();
  const look = handleLook(view?.urgent ?? null, scheme);
  const [delay] = useState(() => syncedDelay(performance.now()));
  const vertical = edge === "left" || edge === "right";

  const handle = (
    <div className="handle-window" data-edge={edge} data-native={isNative() || undefined}>
      <div
        className={`handle${look.breathe ? " breathe" : ""}`}
        data-vertical={vertical || undefined}
        style={{
          background: look.color,
          boxShadow: look.glow,
          animationDelay: look.breathe ? delay : undefined,
        }}
      />
    </div>
  );

  if (isNative()) return handle;
  // Im Browser: der Griff an der Oberkante der Ersatzfläche.
  return <div className="mock-stage">{handle}</div>;
}
