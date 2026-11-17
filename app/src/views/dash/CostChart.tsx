// Kostengraph: gestapelte Fläche je Ticket über die Eimer des Zeitraums, mit
// Fadenkreuz und Tooltip beim Überfahren. Die Rechnung steht in logic/costChart.ts.

import { useState } from "react";
import { bandPath, bucketLabel, fmtCost, stack, type ChartModel, type RangeKey } from "../../logic/costChart";
import type { Lang } from "../../logic/i18n";

/** Höhe der Zeichenfläche im viewBox; die Fläche wird auf die Kastenhöhe gestreckt. */
const H = 300;

export function CostChart({ model, rangeKey, lang }: { model: ChartModel; rangeKey: RangeKey; lang: Lang }) {
  const [hover, setHover] = useState<number | null>(null);
  const n = model.range.starts.length;
  const layers = stack(model.bands);
  const tip = hover === null ? null : tooltip(model, layers, hover, rangeKey, lang);
  const pos = hover === null ? 0 : ((hover + 0.5) / n) * 100;
  return (
    <div className="cchart">
      <div className="cchart-plot">
        <div className="cchart-grid" aria-hidden="true">
          <span className="cchart-line" style={{ top: 0 }} />
          <span className="cchart-line" style={{ top: "50%" }} />
          <span className="cchart-y" style={{ top: 0 }}>
            {model.axisTop}
          </span>
          <span className="cchart-y" style={{ top: "50%" }}>
            {model.axisMid}
          </span>
        </div>
        <svg viewBox={`0 0 1000 ${H}`} preserveAspectRatio="none" role="img" aria-label={model.headline}>
          {layers.map(({ band, top, bottom }) => (
            <path key={band.key || "none"} d={bandPath(top, bottom, model.max, H)} fill={band.color} className="cchart-band" />
          ))}
          <line x1="0" y1={H} x2="1000" y2={H} className="cchart-base" />
        </svg>
        {tip && (
          <>
            <span className="cchart-cross" style={{ left: `${pos}%` }} />
            <div className="cchart-tip" style={pos > 55 ? { right: `calc(${100 - pos}% + 12px)` } : { left: `calc(${pos}% + 12px)` }}>
              <div className="cchart-tip-head">{tip.head}</div>
              {tip.rows.map((r) => (
                <div key={r.name} className="cchart-tip-row">
                  <i style={{ background: r.color }} />
                  {r.name}
                  <b>{r.value}</b>
                </div>
              ))}
            </div>
          </>
        )}
        <div className="cchart-hits" onMouseLeave={() => setHover(null)}>
          {model.range.starts.map((s, i) => (
            <div key={s} onMouseEnter={() => setHover(i)} />
          ))}
        </div>
      </div>
      <div className="cchart-axis" aria-hidden="true">
        {model.ticks.map((t) => (
          <span key={t.at} style={{ left: `${t.at * 100}%`, transform: t.at > 0.93 ? "translateX(-100%)" : undefined }}>
            {t.label}
          </span>
        ))}
      </div>
    </div>
  );
}

function tooltip(model: ChartModel, layers: ReturnType<typeof stack>, i: number, rangeKey: RangeKey, lang: Lang) {
  const start = model.range.starts[i];
  const total = layers.reduce((a, l) => a + l.band.values[i], 0);
  const head = `${bucketLabel(rangeKey, start, model.range.stepS, lang)} · ${fmtCost(total, lang)}`;
  const rows = layers
    .filter((l) => l.band.values[i] > 0)
    .reverse()
    .map((l) => ({ name: l.band.name, color: l.band.color, value: fmtCost(l.band.values[i], lang) }));
  return { head, rows };
}
