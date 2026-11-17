// Ansicht `#/dash`: das Info-Fenster (`dash_window.rs`). Oben die Tokenkosten je Ticket
// mit Zeitraum und Rangliste, unten eine Leiste aus Kacheln (PRs, Reviews, Titan nur bei
// verwaisten Läufen, Aufräumen nur mit Entfernbarem). Ein Klick auf eine Kachel zeigt ihre
// Liste statt des Graphen, nochmal, Esc oder „Tokenkosten“ in den Brotkrumen zurück. Per Info-Taste
// aufgeklappt (`deck://dash-popup`) steht oben eine Leiste mit ×. Hinter „PRs offen“ steht der
// PR-Feed (FeedPane) mit Umschalter auf die bisherige Liste und dem Not-Aus der Automatik.

import { useEffect, useState } from "react";
import type { DashView } from "../../bindings/DashView";
import { commands, getBackend } from "../../ipc";
import { useDash, useDeckView } from "../../ipc/hooks";
import { chartModel, fmtCost, nameOf, rangeLabel, RANGE_KEYS, type Focus, type RangeKey } from "../../logic/costChart";
import { langFromView, type Lang } from "../../logic/i18n";
import { CostChart } from "./CostChart";
import { CleanupList, PrList, ReviewList, TitanList } from "./DashLists";
import { FeedHeadControls, FeedPane } from "./FeedPane";
import "../../styles/dash.css";

type Pane = "chart" | "prs" | "rev" | "titan" | "wt";
const t = (lang: Lang, de: string, en: string) => (lang === "en" ? en : de);
const RANGE_STORE = "deck.dash.range";

function usePopup(): boolean {
  const [popup, setPopup] = useState(false);
  useEffect(() => getBackend().listen("deck://dash-popup", setPopup), []);
  return popup;
}

function storedRange(): RangeKey {
  try {
    const v = window.localStorage.getItem(RANGE_STORE);
    return RANGE_KEYS.includes(v as RangeKey) ? (v as RangeKey) : "w1";
  } catch {
    return "w1";
  }
}

export function DashBoardView() {
  const dash = useDash();
  const lang = langFromView(useDeckView()?.lang);
  const popup = usePopup();
  // ?pane=prs usw. öffnet im Browser-Mock gleich eine Liste (für Bildschirmfotos).
  const [pane, setPane] = useState<Pane>(() => (new URLSearchParams(window.location.search).get("pane") as Pane | null) ?? "chart");
  const [range, setRange] = useState<RangeKey>(storedRange);
  const [focus, setFocus] = useState<Focus>(null);
  // Hinter „PRs offen“: der Feed oder die bisherige Liste (?feedtab=list im Browser-Mock).
  const [feedTab, setFeedTab] = useState<"feed" | "list">(() => (new URLSearchParams(window.location.search).get("feedtab") === "list" ? "list" : "feed"));
  // Verschwindet die Kachel (letzter Lauf beendet), zurück zum Graphen.
  const gone = dash !== null && ((pane === "titan" && !dash.titan) || (pane === "wt" && !dash.cleanup));
  useEffect(() => {
    if (gone) setPane("chart");
  }, [gone]);
  useEffect(() => {
    if (pane === "chart") return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setPane("chart");
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [pane]);
  const pickRange = (k: RangeKey) => {
    setRange(k);
    setFocus(null);
    try {
      window.localStorage.setItem(RANGE_STORE, k);
    } catch {
      // ohne Speicher eben jedes Mal 7 Tage
    }
  };
  const toggle = (p: Pane) => setPane((cur) => (cur === p ? "chart" : p));
  const titles: Record<Pane, string> = {
    chart: t(lang, "Tokenkosten", "Token cost"),
    prs: t(lang, "Deine PRs", "Your PRs"),
    rev: t(lang, "Review für dich", "Review for you"),
    titan: t(lang, "Titan: verwaiste Läufe", "Titan: orphaned runs"),
    wt: t(lang, "Worktrees aufräumen", "Clean up worktrees"),
  };
  const listProps = dash ? { dash, lang, onOpen: commands.openDashUrl, onAction: commands.dashAction } : null;
  return (
    <main className="dash-window" data-popup={popup ? "" : undefined}>
      {popup && (
        <div className="dash-popup-bar">
          <span>Info</span>
          <button type="button" className="dash-close" aria-label={t(lang, "Schließen", "Close")} title={t(lang, "Schließen (Strg+Alt+D)", "Close (Ctrl+Alt+D)")} onClick={() => commands.dashAction("closePopup")}>
            ×
          </button>
        </div>
      )}
      <div className="dash-body">
        <section className="dcard dmain">
          <header className="dhead">
            {/* Brotkrumen statt Zurück-Knopf: „Tokenkosten“ bleibt an seiner Stelle, die Liste
                hängt sich dahinter, so springt beim Wechsel nichts. Esc geht ebenfalls zurück. */}
            {pane === "chart" ? (
              <span>{titles.chart}</span>
            ) : (
              <nav className="dcrumbs" aria-label={t(lang, "Ansicht", "View")}>
                <button type="button" className="dcrumb" title={t(lang, "Zurück (Esc)", "Back (Esc)")} onClick={() => setPane("chart")}>
                  {titles.chart}
                </button>
                <span className="dcrumb-sep" aria-hidden="true">›</span>
                <span className="dcrumb-here" aria-current="page">{titles[pane]}</span>
              </nav>
            )}
            {pane === "chart" && dash?.costs && <span className="dhead-sub">{standText(dash.costs.stand, lang)}</span>}
            {pane === "chart" && (
              <div className="dseg" role="group" aria-label={t(lang, "Zeitraum", "Range")}>
                {RANGE_KEYS.map((k) => (
                  <button key={k} type="button" data-on={k === range ? "" : undefined} onClick={() => pickRange(k)}>
                    {rangeLabel(k, lang)}
                  </button>
                ))}
              </div>
            )}
            {pane === "prs" && <FeedHeadControls tab={feedTab} setTab={setFeedTab} automation={dash?.feed?.automation ?? null} lang={lang} onAction={commands.dashAction} />}
          </header>
          {pane === "chart" && <CostPane dash={dash} range={range} focus={focus} setFocus={setFocus} lang={lang} />}
          {listProps && pane === "prs" && feedTab === "feed" && <FeedPane feed={listProps.dash.feed} lang={lang} onOpen={commands.openDashUrl} onAction={commands.dashAction} />}
          {listProps && pane === "prs" && feedTab === "list" && <PrList {...listProps} />}
          {listProps && pane === "rev" && <ReviewList {...listProps} />}
          {listProps && pane === "titan" && <TitanList {...listProps} />}
          {listProps && pane === "wt" && <CleanupList {...listProps} />}
        </section>
        {dash && <Footer dash={dash} pane={pane} toggle={toggle} lang={lang} />}
      </div>
      {dash?.notice && <div className="dnotice">{dash.notice}</div>}
    </main>
  );
}

function standText(stand: number, lang: Lang): string {
  const d = new Date(stand * 1000);
  return `· ${t(lang, "Stand", "as of")} ${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

function CostPane({ dash, range, focus, setFocus, lang }: { dash: DashView | null; range: RangeKey; focus: Focus; setFocus(f: Focus): void; lang: Lang }) {
  const [plotPx, setPlotPx] = useState(0);
  const model = dash?.costs ? chartModel(dash.costs, range, focus, lang, plotPx) : null;
  if (!model || !dash?.costs) {
    return <p className="dempty">{t(lang, "Die Transcripts werden gelesen…", "Reading transcripts…")}</p>;
  }
  const first = dash.costs.firstAt;
  const short = range === "m1" && first !== null && first > dash.costs.stand - 29 * 86_400;
  return (
    <>
      <div className="dbig">
        <b>{fmtCost(model.shown, lang)}</b>
        <span className="dsub">{model.headline}</span>
        {model.delta && <span className="dsub dstrong">{model.delta}</span>}
      </div>
      <div className="dmix">
        {model.mix.map((m) => (
          <span key={m.name}>
            {m.name} <b>{m.pct}</b>
          </span>
        ))}
        <span className="dmix-models">{model.models}</span>
      </div>
      <div className="dchart-row">
        <CostChart model={model} rangeKey={range} lang={lang} onWidth={setPlotPx} />
        <div className="drank">
          {model.rank.map((r) => (
            <button
              key={r.key || "none"}
              type="button"
              data-on={focus === r.key ? "" : undefined}
              data-dim={focus !== null && focus !== r.key ? "" : undefined}
              onClick={() => setFocus(focus === r.key ? null : r.key)}
              title={`${nameOf(r.key, lang)}: ${Math.round((r.value / (model.total || 1)) * 100)} %. Cache ${t(lang, "lesen", "read")} ${fmtCost(r.mix.read, lang)}, Cache ${t(lang, "schreiben", "write")} ${fmtCost(r.mix.write, lang)}, Output ${fmtCost(r.mix.output, lang)}, Input ${fmtCost(r.mix.input, lang)}`}
            >
              <span className="drank-l">
                <i style={{ background: r.color }} />
                <span className="drank-name">{r.name}</span>
                <b>{fmtCost(r.value, lang)}</b>
              </span>
              <span className="drank-bar">
                <span style={{ width: `${r.share * 100}%`, background: r.color }} />
              </span>
            </button>
          ))}
          <p className="drank-hint">
            {short
              ? t(lang, `Die Transcripts reichen bis ${new Date((first ?? 0) * 1000).toLocaleDateString("de-DE")} zurück.`, `Transcripts go back to ${new Date((first ?? 0) * 1000).toLocaleDateString("en-GB")}.`)
              : t(lang, "Klick auf ein Ticket zeigt nur dieses.", "Click a ticket to show only it.")}
          </p>
        </div>
      </div>
    </>
  );
}

function Footer({ dash, pane, toggle, lang }: { dash: DashView; pane: Pane; toggle(p: Pane): void; lang: Lang }) {
  const p = dash.prs;
  const on = (x: Pane) => (pane === x ? "" : undefined);
  return (
    <div className="dfootbar">
      <button type="button" className="dtile" data-on={on("prs")} data-stale={p.stale ? "" : undefined} onClick={() => toggle("prs")}>
        <b>{p.rows.length}</b>
        <span>
          {t(lang, "PRs offen", "open PRs")}
          <br />
          <em className="t-good">
            {p.ready} {t(lang, "bereit", "ready")}
          </em>
          {" · "}
          <em className={p.mine > 0 ? "t-warn" : undefined}>
            {p.mine} {t(lang, "bei dir", "on you")}
          </em>
          {" · "}
          {p.waiting} {t(lang, "warten", "waiting")}
        </span>
      </button>
      <button type="button" className="dtile" data-on={on("rev")} onClick={() => toggle("rev")}>
        <b>{dash.reviews.length}</b>
        <span>
          {t(lang, "Reviews", "Reviews")}
          <br />
          {t(lang, "für dich", "for you")}
        </span>
      </button>
      {dash.titan && (
        <button type="button" className="dtile" data-tone="bad" data-on={on("titan")} onClick={() => toggle("titan")}>
          <b>{dash.titan.runs.length}</b>
          <span>
            {t(lang, "Titan-Läufe", "Titan runs")}
            <br />
            {t(lang, "verwaist", "orphaned")}
          </span>
        </button>
      )}
      {dash.cleanup && (
        <button type="button" className="dtile" data-on={on("wt")} onClick={() => toggle("wt")}>
          <b>{dash.cleanup.removable.length}</b>
          <span>
            Worktrees
            <br />
            {dash.cleanup.busy ? t(lang, "werden entfernt", "being removed") : t(lang, "aufräumbar", "to clean up")}
          </span>
        </button>
      )}
    </div>
  );
}
