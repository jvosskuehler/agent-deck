// Die Listen hinter den Kacheln der Leiste: eigene PRs, Reviews mit Approve, verwaiste
// Titan-Läufe, aufräumbare Worktrees. Alles, was etwas tut, fragt erst nach
// (ConfirmButton); Rust prüft danach selbst noch einmal.

import { useEffect, useState } from "react";
import type { DashView } from "../../bindings/DashView";
import type { Lang } from "../../logic/i18n";

const t = (lang: Lang, de: string, en: string) => (lang === "en" ? en : de);

interface ListProps {
  dash: DashView;
  lang: Lang;
  onOpen(url: string): void;
  onAction(command: string): void;
}

/** Ein Knopf mit Rückfrage: erster Klick zeigt `confirm`, zweiter führt aus. */
export function ConfirmButton({ label, confirm, tone, onConfirm }: { label: string; confirm: string; tone?: "ok" | "bad"; onConfirm(): void }) {
  const [armed, setArmed] = useState(false);
  useEffect(() => {
    if (!armed) return;
    const id = window.setTimeout(() => setArmed(false), 4000);
    return () => window.clearTimeout(id);
  }, [armed]);
  return (
    <button
      type="button"
      className="dbtn"
      data-tone={tone}
      data-armed={armed ? "" : undefined}
      onClick={(e) => {
        e.stopPropagation();
        if (armed) {
          setArmed(false);
          onConfirm();
        } else {
          setArmed(true);
        }
      }}
    >
      {armed ? confirm : label}
    </button>
  );
}

export function PrList({ dash, lang, onOpen }: ListProps) {
  const { rows, note } = dash.prs;
  return (
    <div className="dlist">
      {rows.map((p) => (
        <button key={p.url || p.number} type="button" className="dli dli-pr" onClick={() => p.url && onOpen(p.url)} title={`${p.repo} #${p.number}: ${p.title}`}>
          <span>
            <span className="dchip" data-tone={p.tone}>
              {p.status}
            </span>
          </span>
          <span className="dli-repo">
            {p.repo} #{p.number}
          </span>
          <span className="dli-title">{p.title}</span>
          <span className="dli-meta">{[p.ticket, p.meta].filter(Boolean).join(" · ")}</span>
        </button>
      ))}
      {rows.length === 0 && <p className="dempty">{t(lang, "Keine offenen PRs.", "No open PRs.")}</p>}
      {note && <p className="dempty">{note}</p>}
    </div>
  );
}

export function ReviewList({ dash, lang, onOpen, onAction }: ListProps) {
  return (
    <div className="dlist">
      {dash.reviews.map((r) => (
        <div key={r.url} className="dli dli-rev">
          <span className="dli-repo">
            {r.repo} #{r.number}
          </span>
          <span className="dli-title" title={r.title}>
            {r.title}
          </span>
          <span className="dli-meta">{r.meta}</span>
          <span className="dli-acts">
            <button type="button" className="dbtn" onClick={() => onOpen(r.url)}>
              {t(lang, "Ansehen", "View")}
            </button>
            <ConfirmButton label="Approve" confirm={t(lang, "Wirklich?", "Sure?")} tone="ok" onConfirm={() => onAction(`approve:${r.url}`)} />
          </span>
        </div>
      ))}
      {dash.reviews.length === 0 && <p className="dempty">{t(lang, "Nichts offen.", "Nothing open.")}</p>}
    </div>
  );
}

export function TitanList({ dash, lang, onAction }: ListProps) {
  const titan = dash.titan;
  if (!titan) return <p className="dempty">{t(lang, "Keine hängenden Läufe.", "No stuck runs.")}</p>;
  return (
    <>
      <p className="dfacts">{titan.facts}</p>
      <div className="dlist">
        {titan.runs.map((r) => (
          <div key={r.name} className="dli dli-run">
            <span className="dli-strong">{r.label}</span>
            <span className="dli-bad">{r.age}</span>
            <span className="dli-meta">{t(lang, "verwaist: niemand wartet mehr auf das Ergebnis", "orphaned: nobody waits for the result")}</span>
            <span className="dli-acts">
              <ConfirmButton label={t(lang, "Beenden", "Stop")} confirm={t(lang, "Wirklich?", "Sure?")} tone="bad" onConfirm={() => onAction(`titanKill:${r.name}`)} />
            </span>
          </div>
        ))}
      </div>
      <div className="dfoot">
        <span>
          {t(
            lang,
            "Verwaist heißt: über dem Lauf hängt keine ssh-Sitzung mehr. Läufe, auf die noch jemand wartet, stehen hier nie.",
            "Orphaned means no ssh session is attached any more. Runs someone still waits for never show here.",
          )}
        </span>
        <ConfirmButton label={t(lang, "Alle beenden", "Stop all")} confirm={t(lang, "Wirklich alle?", "Really all?")} tone="bad" onConfirm={() => onAction("titanKillAll")} />
      </div>
    </>
  );
}

export function CleanupList({ dash, lang, onAction }: ListProps) {
  const c = dash.cleanup;
  if (!c) return <p className="dempty">{t(lang, "Nichts aufzuräumen.", "Nothing to clean up.")}</p>;
  const n = c.removable.length;
  return (
    <>
      <div className="dwt">
        {c.removable.map((w) => (
          <div key={w.folder} className="dwt-row" title={`${w.repo} · ${w.branch}`}>
            <span className="dli-strong">{w.folder}</span>
            <span className="dli-meta">{w.repo}</span>
          </div>
        ))}
      </div>
      <div className="dfoot">
        <span>
          {t(
            lang,
            "Nur Worktrees ohne nicht committete Arbeit, deren Stand genau der gemergte PR ist und in denen kein offener Agent arbeitet. Entfernt werden Worktree und lokaler Branch.",
            "Only worktrees without uncommitted work, whose head is exactly the merged PR and that no open agent uses. Removes the worktree and its local branch.",
          )}
          {c.changed.length > 0 && ` ${t(lang, "Bleiben stehen (Änderungen oder neuerer Stand):", "Left in place (changes or newer head):")} ${c.changed.map((w) => w.folder).join(", ")}.`}
          {c.result && ` ${t(lang, "Zuletzt:", "Last run:")} ${c.result}.`}
        </span>
        {c.busy ? (
          <span className="dbtn" data-busy="">
            {t(lang, "räumt auf…", "cleaning…")}
          </span>
        ) : (
          n > 0 && (
            <ConfirmButton
              label={t(lang, `Alle ${n} entfernen`, `Remove all ${n}`)}
              confirm={t(lang, `Wirklich ${n} entfernen?`, `Really remove ${n}?`)}
              tone="bad"
              onConfirm={() => onAction("cleanupWorktrees")}
            />
          )
        )}
      </div>
    </>
  );
}
