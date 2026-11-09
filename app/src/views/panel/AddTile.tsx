// Die ＋-Karte am Reihenende: so groß wie eine Karte ihres Blocks, gestrichelt, leise.

import type { GridPlace } from "../../logic/layout";

interface Props {
  label: string;
  onClick(): void;
  /** Fester Platz im Raster, wenn die Karten davor fest liegen. */
  place?: GridPlace;
}

export function AddTile({ label, onClick, place }: Props) {
  return (
    <button type="button" className="add-tile" aria-label={label} title={label} onClick={onClick} style={place}>
      <svg viewBox="0 0 18 18" width="14" height="14" aria-hidden="true">
        {/* Zwei Striche statt des Zeichens „+“: die Glyphe sitzt je nach Schrift nicht
            mittig in der Zeile, die Striche schon. */}
        <path d="M9 2.5v13M2.5 9h13" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" />
      </svg>
    </button>
  );
}
