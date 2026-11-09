// „Läuft noch ohne Fenster“ in der unteren Leiste: nur wenn es Waisen gibt, ein leiser
// Hinweis „2 ohne Fenster“. Hover (wie bei der Nutzung) oder Klick öffnet darüber die
// Liste: Port, Programm, Befehl, Herkunft, „Beenden“; oben „Alle beenden“.
//
// Hover allein öffnet, der Klick hält offen (bis zum zweiten Klick, Escape oder einem
// Klick daneben). Liste und Hinweis sind ein Hover-Bereich, damit der Zeiger vom Hinweis
// in die Liste wandern kann, ohne dass sie zuklappt.

import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { OrphanView } from "../../bindings/OrphanView";
import { t, type Lang } from "../../logic/i18n";
import { orphanHint, popoverMaxHeight } from "../../logic/orphans";
import "../../styles/orphans.css";

interface Props {
  orphans: OrphanView[];
  lang: Lang;
  onKill(id: string): void;
  onKillAll(): void;
}

export function OrphanHint({ orphans, lang, onKill, onKillAll }: Props) {
  const [hover, setHover] = useState(false);
  const [pinned, setPinned] = useState(false);
  const [maxHeight, setMaxHeight] = useState<number | undefined>(undefined);
  const boxRef = useRef<HTMLDivElement>(null);
  const open = hover || pinned;

  // Festgehalten schließt Escape oder ein Klick außerhalb.
  useEffect(() => {
    if (!pinned) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setPinned(false);
    const onDown = (e: PointerEvent) => {
      if (!boxRef.current?.contains(e.target as Node)) setPinned(false);
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("pointerdown", onDown);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("pointerdown", onDown);
    };
  }, [pinned]);

  // Platz über der Leiste messen: das Fenster ist so groß wie das Panel.
  useLayoutEffect(() => {
    if (!open) return;
    const bar = boxRef.current?.closest(".bar");
    const panel = boxRef.current?.closest(".panel");
    if (bar && panel) setMaxHeight(popoverMaxHeight(panel.getBoundingClientRect().top, bar.getBoundingClientRect().top));
  }, [open]);

  const anyBusy = orphans.some((o) => o.busy);
  return (
    <div className="orphans" ref={boxRef} onPointerEnter={() => setHover(true)} onPointerLeave={() => setHover(false)}>
      <button
        type="button"
        className="orphans-hint"
        aria-expanded={open}
        data-open={open || undefined}
        onClick={() => setPinned((p) => !p)}
      >
        {orphanHint(orphans.length, lang)}
      </button>
      {open && (
        <div className="orphans-pop">
          <section className="orphans-card" role="dialog" aria-label={t(lang, "orphansHead")} style={{ maxHeight }}>
            <header className="orphans-head">
              <span>{t(lang, "orphansHead")}</span>
              <button type="button" className="orphans-all" disabled={anyBusy} onClick={onKillAll}>
                {t(lang, "orphansKillAll")}
              </button>
            </header>
            <div className="orphans-list">
              {orphans.map((o) => (
                <OrphanRow key={o.id} orphan={o} lang={lang} onKill={onKill} />
              ))}
            </div>
            <p className="orphans-note">{t(lang, "orphansNote")}</p>
          </section>
        </div>
      )}
    </div>
  );
}

function OrphanRow({ orphan: o, lang, onKill }: { orphan: OrphanView; lang: Lang; onKill(id: string): void }) {
  return (
    <div className="orphans-row" data-busy={o.busy || undefined}>
      <span className="orphans-port" data-none={o.ports.length === 0 || undefined}>
        {o.portText}
      </span>
      <span className="orphans-prog">{o.program}</span>
      <span className="orphans-cmd" title={o.commandFull}>
        {o.command}
      </span>
      <span className="orphans-origin">{o.origin}</span>
      <button type="button" className="orphans-kill" disabled={o.busy} onClick={() => onKill(o.id)}>
        {o.busy ? t(lang, "orphansKilling") : t(lang, "orphansKill")}
      </button>
      {o.error && <span className="orphans-error">{o.error}</span>}
    </div>
  );
}
