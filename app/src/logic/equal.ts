// Vergleich von Props für React.memo.
//
// Jeder Poll schickt die ganze Ansicht neu (frische Objekte, auch für Karten, die sich
// nicht geändert haben), und eine Verschiebung beim Ziehen ist ein neues {x, y}. Darum
// reicht der flache Vergleich von memo nicht: Objekte in den Props werden eine Ebene
// tief verglichen. Das genügt, weil TileView, Verschiebung, Rasterplatz und Handler flach
// sind.

/** Gleiche Schlüssel, Werte gleich nach Object.is. */
function shallowEqual(a: object, b: object): boolean {
  const ka = Object.keys(a);
  if (ka.length !== Object.keys(b).length) return false;
  return ka.every(
    (k) => Object.prototype.hasOwnProperty.call(b, k) && Object.is((a as Record<string, unknown>)[k], (b as Record<string, unknown>)[k]),
  );
}

/** Props gleich, wenn jeder Wert gleich ist oder (bei Objekten) eine Ebene tief gleich. */
export function propsEqual<P extends object>(prev: P, next: P): boolean {
  const keys = Object.keys(prev);
  if (keys.length !== Object.keys(next).length) return false;
  return keys.every((k) => {
    const a = (prev as Record<string, unknown>)[k];
    const b = (next as Record<string, unknown>)[k];
    if (Object.is(a, b)) return true;
    return (
      typeof a === "object" && a !== null && typeof b === "object" && b !== null && !Array.isArray(a) && !Array.isArray(b) && shallowEqual(a, b)
    );
  });
}
