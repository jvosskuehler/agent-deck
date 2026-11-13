// Farbwahl: ein Feld Sättigung × Helligkeit, darunter die Farbton-Leiste, daneben die
// aktuelle Farbe mit ihrem Hex-Wert zum Eintippen. Ziehen meldet jede Bewegung.

import { useEffect, useState } from "react";
import { hexToHsv, hsvToHex, parseHex, type Hsv } from "../../logic/glow";

interface Props {
  value: string;
  onChange(hex: string): void;
}

export function ColorField({ value, onChange }: Props) {
  // Den Farbton selbst halten: bei Grau oder Schwarz geht er im Hex verloren.
  const [hsv, setHsv] = useState<Hsv>(() => hexToHsv(value));
  const [draft, setDraft] = useState(value);
  useEffect(() => {
    if (hsvToHex(hsv) !== value.toUpperCase()) setHsv(hexToHsv(value));
    setDraft(value);
    // Nur von außen kommende Werte übernehmen.
  }, [value]);

  const pick = (next: Hsv) => {
    setHsv(next);
    const hex = hsvToHex(next);
    setDraft(hex);
    onChange(hex);
  };

  const drag = (at: (x: number, y: number) => void) => (e: React.PointerEvent<HTMLDivElement>) => {
    const el = e.currentTarget;
    el.setPointerCapture(e.pointerId);
    const move = (ev: { clientX: number; clientY: number }) => {
      const r = el.getBoundingClientRect();
      at(Math.min(1, Math.max(0, (ev.clientX - r.left) / r.width)), Math.min(1, Math.max(0, (ev.clientY - r.top) / r.height)));
    };
    move(e);
    const onMove = (ev: PointerEvent) => move(ev);
    const onUp = () => {
      el.removeEventListener("pointermove", onMove);
      el.removeEventListener("pointerup", onUp);
    };
    el.addEventListener("pointermove", onMove);
    el.addEventListener("pointerup", onUp);
  };

  const hueColor = hsvToHex({ h: hsv.h, s: 1, v: 1 });
  return (
    <div className="glow-color">
      <div
        className="glow-sv"
        style={{ backgroundColor: hueColor }}
        onPointerDown={drag((x, y) => pick({ ...hsv, s: x, v: 1 - y }))}
      >
        <span className="glow-sv-dot" style={{ left: `${hsv.s * 100}%`, top: `${(1 - hsv.v) * 100}%`, background: value }} />
      </div>
      <div className="glow-hue" onPointerDown={drag((x) => pick({ ...hsv, h: Math.min(359.9, x * 360) }))}>
        <span className="glow-hue-dot" style={{ left: `${(hsv.h / 360) * 100}%`, background: hueColor }} />
      </div>
      <div className="glow-current">
        <span className="glow-swatch" style={{ background: value }} />
        <input
          className="ticket-input glow-hex"
          spellCheck={false}
          value={draft}
          onChange={(e) => {
            setDraft(e.target.value);
            const rgb = parseHex(e.target.value);
            if (rgb) {
              const hex = e.target.value.startsWith("#") ? e.target.value.toUpperCase() : `#${e.target.value.toUpperCase()}`;
              setHsv(hexToHsv(hex));
              onChange(hex);
            }
          }}
          onBlur={() => setDraft(value)}
        />
      </div>
    </div>
  );
}
