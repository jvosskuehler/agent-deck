// Der PR-Feed hinter der Kachel „PRs offen“ (Entwurf F1 „Zeitleiste“, 17.11.2026). Oben
// „Braucht dich“: Kommentare von Menschen, auf die ich noch nicht reagiert habe. Dann die
// laufenden Hintergrund-Agenten, die Zeitleiste der letzten 24 Stunden und je eigenem PR,
// was die Automatik vorhat (mit „nicht mergen“). Unten die Legende der Punktfarben.
// Das Modell rechnet Rust (`deck_core::dash::feed`); hier wird nur gezeichnet.

import { useEffect, useState } from "react";
import type { AgentRow } from "../../bindings/AgentRow";
import type { AutoRow } from "../../bindings/AutoRow";
import type { FeedEvent } from "../../bindings/FeedEvent";
import type { FeedView } from "../../bindings/FeedView";
import type { NeedCard } from "../../bindings/NeedCard";
import { ago, clock, moreText } from "../../logic/feedTime";
import type { Lang } from "../../logic/i18n";
import { ArmButton, Avatar, CardButtons, DraftBlock, tr, type FeedActions } from "./FeedParts";
import "../../styles/feed.css";

/** Jetzt in Unix-Sekunden, alle 30 s neu („vor 4 Min.“ soll mitlaufen). */
function useNow(): number {
  const [now, setNow] = useState(() => Date.now() / 1000);
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now() / 1000), 30_000);
    return () => window.clearInterval(id);
  }, []);
  return now;
}

export function FeedPane({ feed, lang, onOpen, onAction }: { feed: FeedView | null; lang: Lang } & FeedActions) {
  const now = useNow();
  if (!feed) {
    return <p className="dempty">{tr(lang, "GitHub wird abgefragt…", "Asking GitHub…")}</p>;
  }
  return (
    <>
      <div className="feed">
        <h3 className="fsec">{tr(lang, "Braucht dich", "Needs you")}</h3>
        {feed.needs.length === 0 && <p className="fempty">{tr(lang, "Nichts offen.", "Nothing open.")}</p>}
        {feed.needs.map((c) => (
          <NeedCardView key={c.key} card={c} now={now} lang={lang} onOpen={onOpen} onAction={onAction} />
        ))}
        {feed.agents.length > 0 && (
          <>
            <h3 className="fsec">{tr(lang, "Hintergrund-Agenten", "Background agents")}</h3>
            <div className="fagents">
              {feed.agents.map((a) => (
                <AgentLine key={a.id || `${a.label}-${a.task}`} agent={a} lang={lang} onOpen={onOpen} onAction={onAction} />
              ))}
            </div>
          </>
        )}
        <h3 className="fsec">{tr(lang, "Was passiert ist", "What happened")}</h3>
        {feed.events.length === 0 ? (
          <p className="fempty">{tr(lang, "Keine Ereignisse in den letzten 24 Std.", "No events in the last 24 h.")}</p>
        ) : (
          <div className="ftimeline">
            {feed.events.map((e, i) => (
              <EventRow key={`${e.at}-${e.label}-${i}`} event={e} last={i === feed.events.length - 1} lang={lang} onOpen={onOpen} />
            ))}
          </div>
        )}
        {feed.auto.length > 0 && (
          <>
            <h3 className="fsec">
              {tr(lang, "Automatik", "Automation")}
              {!feed.automation && <span className="fsec-off">{tr(lang, " · aus, zeigt nur an", " · off, display only")}</span>}
            </h3>
            <div className="fauto">
              {feed.auto.map((r) => (
                <AutoLine key={r.key} row={r} lang={lang} onOpen={onOpen} onAction={onAction} />
              ))}
            </div>
          </>
        )}
        {feed.note && <p className="fempty">{feed.note}</p>}
      </div>
      <Legend lang={lang} />
    </>
  );
}

function NeedCardView({ card, now, lang, onOpen, onAction }: { card: NeedCard; now: number; lang: Lang } & FeedActions) {
  const more = moreText(card.more, lang);
  return (
    <div className="fcard">
      <div className="fcard-row">
        <Avatar card={card} size={28} />
        <div className="fcard-main">
          <div className="fcard-who">
            <b>{card.login}</b>
            <span>
              {card.verb} · {ago(card.at, now, lang)}
            </span>
          </div>
          <div className="fcard-pr">
            <button type="button" className="flabel" onClick={() => onOpen(card.url)}>
              {card.label}
            </button>{" "}
            {card.title}
          </div>
          <div className="fcard-text" title={card.excerpt}>
            „{card.excerpt}“{more && <span className="fmuted"> {more}</span>}
          </div>
          {card.agent && <div className="fcard-agent">◐ {card.agent}</div>}
        </div>
        {!card.draft && (
          <div className="fcard-acts">
            <CardButtons card={card} lang={lang} onOpen={onOpen} onAction={onAction} />
          </div>
        )}
      </div>
      {card.draft && <DraftBlock cardKey={card.key} draft={card.draft} lang={lang} onAction={onAction} />}
    </div>
  );
}

function AgentLine({ agent, lang, onOpen, onAction }: { agent: AgentRow; lang: Lang } & FeedActions) {
  return (
    <div className="fagent" data-waiting={agent.waiting ? "" : undefined}>
      <span className="fagent-dot" aria-hidden="true" />
      <button type="button" className="flabel" onClick={() => agent.url && onOpen(agent.url)}>
        {agent.label}
      </button>
      <span className="fagent-task">{agent.task}</span>
      <span className="fmuted">{agent.state}</span>
      {agent.id && !agent.waiting && (
        <ArmButton label={tr(lang, "Abbrechen", "Abort")} confirm={tr(lang, "Wirklich?", "Sure?")} kind="plain" onConfirm={() => onAction(`agentAbort:${agent.id}`)} />
      )}
    </div>
  );
}

function EventRow({ event, last, lang, onOpen }: { event: FeedEvent; last: boolean; lang: Lang; onOpen(url: string): void }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <span className="ftime">{clock(event.at)}</span>
      <span className="fdot-cell">
        <span className="fdot" data-kind={event.kind} />
      </span>
      <div className="fevent" data-last={last ? "" : undefined}>
        {event.label && (
          <>
            <button type="button" className="flabel" onClick={() => event.url && onOpen(event.url)}>
              {event.label}
            </button>{" "}
          </>
        )}
        {event.text}
        {event.detail && <span className="fmuted"> · {event.detail}</span>}
        {event.report && (
          <>
            {" "}
            <button type="button" className="flink" aria-expanded={open} onClick={() => setOpen(!open)}>
              {open ? tr(lang, "Bericht zu", "Hide report") : tr(lang, "Bericht", "Report")}
            </button>
          </>
        )}
        {open && event.report && <div className="freport">{event.report}</div>}
      </div>
    </>
  );
}

function AutoLine({ row, lang, onOpen, onAction }: { row: AutoRow; lang: Lang } & FeedActions) {
  return (
    <div className="fautorow" data-held={row.held ? "" : undefined}>
      <button type="button" className="flabel" onClick={() => onOpen(row.url)}>
        {row.label}
      </button>
      <span className="fauto-title" title={row.title}>
        {row.title}
      </span>
      <span className="fauto-state">{row.state}</span>
      {row.held ? (
        <button type="button" className="fbtn" data-kind="plain" onClick={() => onAction(`release:${row.key}`)}>
          {tr(lang, "wieder freigeben", "release")}
        </button>
      ) : (
        <button type="button" className="fbtn" data-kind="quiet" onClick={() => onAction(`hold:${row.key}`)}>
          {tr(lang, "nicht mergen", "don't merge")}
        </button>
      )}
    </div>
  );
}

function Legend({ lang }: { lang: Lang }) {
  const items: [string, string][] = [
    ["merged", tr(lang, "gemergt", "merged")],
    ["agent", tr(lang, "Claude hat gearbeitet", "Claude worked")],
    ["review", "Review"],
    ["problem", tr(lang, "Problem", "problem")],
    ["minor", tr(lang, "Kleinkram", "minor")],
  ];
  return (
    <div className="flegend">
      {items.map(([kind, text]) => (
        <span key={kind}>
          <span className="fdot" data-kind={kind} /> {text}
        </span>
      ))}
    </div>
  );
}

/** Kopf rechts im Feed: Feed oder Liste, dazu der Not-Aus der Automatik. */
export function FeedHeadControls({ tab, setTab, automation, lang, onAction }: { tab: "feed" | "list"; setTab(t: "feed" | "list"): void; automation: boolean | null; lang: Lang; onAction(c: string): void }) {
  return (
    <div className="fhead-ctl">
      {automation !== null && (
        <button
          type="button"
          className="fswitch"
          role="switch"
          aria-checked={automation}
          title={automation ? tr(lang, "Automatik an: mergt, trägt nach, startet Hintergrund-Agenten", "Automation on: merges, fills in, starts background agents") : tr(lang, "Automatik aus: zeigt nur an", "Automation off: display only")}
          onClick={() => onAction(automation ? "automation:off" : "automation:on")}
        >
          <span className="fswitch-track" data-on={automation ? "" : undefined}>
            <span className="fswitch-knob" />
          </span>
          {tr(lang, "Automatik", "Automation")}
        </button>
      )}
      <div className="dseg" role="group" aria-label={tr(lang, "Ansicht", "View")}>
        <button type="button" data-on={tab === "feed" ? "" : undefined} onClick={() => setTab("feed")}>
          Feed
        </button>
        <button type="button" data-on={tab === "list" ? "" : undefined} onClick={() => setTab("list")}>
          {tr(lang, "Liste", "List")}
        </button>
      </div>
    </div>
  );
}
