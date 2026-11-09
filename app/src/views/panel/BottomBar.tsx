// Untere Leiste: links die Nutzung (Ampelpunkt, „Session 34 %“, „Woche 61 %“), rechts das
// Zahnrad. Hover über der Nutzung zeigt Resets, weitere Limits, Stand und Fehler; Klick
// öffnet claude.ai/settings/usage.
//
// Der Hover-Text steht im Panel selbst über der Leiste, nicht in einem eigenen Fenster:
// das Panel-Fenster ist genau so groß wie sein Inhalt, nach unten wäre kein Platz.

import { useState } from "react";
import type { LimitView } from "../../bindings/LimitView";
import type { UsageView } from "../../bindings/UsageView";
import { t, type Lang } from "../../logic/i18n";
import { PALETTES, usageDot, type Scheme } from "../../logic/palette";
import { useScheme } from "../useScheme";
import { GearIcon } from "../icons";

interface Props {
  usage: UsageView | null;
  lang: Lang;
  onOpenUsage(): void;
  onOpenSettings(): void;
}

function percentText(limit: LimitView): string {
  // Ein Limit hinter seinem Reset ist keine Zahl mehr (CLAUDE.md, Fallen): graues „—“.
  return limit.percent === null ? "—" : `${Math.round(limit.percent)} %`;
}

/** Ampelpunkt eines Limits; unbekannt = leiser Punkt. */
function dotColor(limit: LimitView, scheme: Scheme): string {
  return limit.percent === null ? PALETTES[scheme].dotUnknown : usageDot(limit.color, scheme);
}

export function BottomBar({ usage, lang, onOpenUsage, onOpenSettings }: Props) {
  const [tipOpen, setTipOpen] = useState(false);
  const scheme = useScheme();

  return (
    <footer className="bar">
      {usage && usage.primary.length > 0 && (
        <button
          type="button"
          className="bar-usage"
          data-stale={usage.stale || undefined}
          onClick={onOpenUsage}
          onPointerEnter={() => setTipOpen(true)}
          onPointerLeave={() => setTipOpen(false)}
          onFocus={() => setTipOpen(true)}
          onBlur={() => setTipOpen(false)}
        >
          {usage.primary.map((l) => (
            <span key={l.label} className="bar-limit">
              <span className="bar-dot" style={{ background: dotColor(l, scheme) }} />
              {l.label}
              <span className="bar-value" data-unknown={l.percent === null || undefined}>
                {percentText(l)}
              </span>
            </span>
          ))}
        </button>
      )}
      <span className="bar-spacer" />
      <button
        type="button"
        className="bar-gear"
        aria-label={t(lang, "settings")}
        title={t(lang, "settings")}
        onClick={onOpenSettings}
      >
        <GearIcon size={16} />
      </button>
      {tipOpen && usage && <UsageTip usage={usage} lang={lang} scheme={scheme} />}
    </footer>
  );
}

function UsageTip({ usage, lang, scheme }: { usage: UsageView; lang: Lang; scheme: Scheme }) {
  const rows = [...usage.primary, ...usage.extra];
  return (
    <div className="usage-tip" role="tooltip">
      <div className="usage-tip-head">{t(lang, "usageHead")}</div>
      {usage.account && <div className="usage-tip-account">{t(lang, "usageAccount", { account: usage.account })}</div>}
      {rows.map((l) => (
        <div key={l.label} className="usage-tip-row">
          <span className="bar-dot" style={{ background: dotColor(l, scheme) }} />
          <span className="usage-tip-label">{l.label}</span>
          <span className="usage-tip-value" data-unknown={l.percent === null || undefined}>
            {percentText(l)}
          </span>
          {l.resets && <span className="usage-tip-reset">{t(lang, "usageReset", { when: l.resets })}</span>}
        </div>
      ))}
      <div className="usage-tip-foot">
        {usage.asOf}
        {usage.stale && <span> · {t(lang, "usageStale")}</span>}
      </div>
      {usage.error && <div className="usage-tip-error">{usage.error}</div>}
      <div className="usage-tip-hint">{t(lang, "usageOpen")}</div>
    </div>
  );
}
