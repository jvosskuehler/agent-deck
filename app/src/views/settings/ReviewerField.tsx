// Reviewer-Feld der PR-Automatik: Namen, Mailadressen oder Logins, mehrere mit Komma.
// Beim Verlassen oder mit Enter löst Rust sie gegen die Mitglieder der GitHub-Organisation
// auf (set_reviewers, gh) und speichert die Logins; das Feld zeigt sie danach als
// „Name (login)“. Was offen bleibt, steht klein darunter: eine Wahl bei mehreren
// Treffern oder mehreren Organisationen, ein Hinweis bei Einträgen ohne Treffer.

import { useEffect, useState } from "react";
import type { ReviewerReport } from "../../bindings/ReviewerReport";
import type { ReviewerResult } from "../../bindings/ReviewerResult";
import type { SettingsView } from "../../bindings/SettingsView";
import { commands } from "../../ipc";
import { t, type Lang } from "../../logic/i18n";

interface Props {
  lang: Lang;
  settings: SettingsView;
  /** Neuer Stand aus Rust (gespeichert). */
  onSaved(next: SettingsView): void;
  onError(message: string): void;
}

function personLabel(login: string, name: string): string {
  return name ? `${name} (${login})` : login;
}

export function ReviewerField({ lang, settings, onSaved, onError }: Props) {
  const value = settings.prReviewerLabel;
  const [draft, setDraft] = useState(value);
  const [report, setReport] = useState<ReviewerReport | null>(null);
  const [lastText, setLastText] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => setDraft(value), [value]);

  const resolve = (text: string) => {
    setBusy(true);
    setLastText(text);
    commands
      .setReviewers(text)
      .then((r) => {
        setReport(r.report);
        onSaved(r.settings);
      })
      .catch((err: unknown) => onError(String(err)))
      .finally(() => setBusy(false));
  };

  const commit = () => {
    const text = draft.trim();
    if (text !== value) resolve(text);
  };

  // Eine Wahl: gespeicherte Liste + gewählter Login + die übrigen noch offenen Einträge.
  const choose = (open: ReviewerResult, login: string) => {
    const rest = (report?.results ?? []).filter((r) => r.status === "ambiguous" && r !== open).map((r) => r.input);
    resolve([value, login, ...rest].filter((x) => x.trim() !== "").join(", "));
  };

  const chooseOrg = (org: string) => {
    commands
      .setSetting("githubOrg", org)
      .then((next) => {
        onSaved(next);
        resolve(lastText);
      })
      .catch((err: unknown) => onError(String(err)));
  };

  const org = report?.org || settings.githubOrg || t(lang, "reviewerNoOrg");
  const why =
    report?.note === "ghUnavailable"
      ? t(lang, "reviewerWhyGh")
      : report?.note === "noOrg"
        ? t(lang, "reviewerWhyNoOrg")
        : report?.note === "membersFailed"
          ? t(lang, "reviewerWhyMembers", { org })
          : null;
  const looked = report !== null && report.note === "none";

  return (
    <div className="set-field">
      <label className="set-label" htmlFor="set-reviewers">
        {t(lang, "prReviewer")}
      </label>
      <input
        id="set-reviewers"
        className="set-input"
        value={draft}
        placeholder="Leo, Anna Schmidt, name@firma.de"
        spellCheck={false}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") commit();
          if (e.key === "Escape") setDraft(value);
        }}
      />
      {busy && <span className="set-row-hint">{t(lang, "reviewerBusy")}</span>}
      {!busy && report && (
        <div className="rev-notes">
          {report.note === "chooseOrg" && (
            <div className="rev-pick">
              <span className="rev-note">{t(lang, "reviewerOrgPick")}</span>
              <div className="rev-choices">
                {report.orgs.map((o) => (
                  <button key={o} type="button" className="tool-btn rev-choice" onClick={() => chooseOrg(o)}>
                    {o}
                  </button>
                ))}
              </div>
            </div>
          )}
          {why && <span className="rev-note">{t(lang, "reviewerNoLookup", { why })}</span>}
          {report.results.map((r) =>
            r.status === "ambiguous" ? (
              <div key={r.input} className="rev-pick">
                <span className="rev-note">{t(lang, "reviewerPick", { input: r.input })}</span>
                <div className="rev-choices">
                  {r.candidates.map((c) => (
                    <button key={c.login} type="button" className="tool-btn rev-choice" onClick={() => choose(r, c.login)}>
                      {personLabel(c.login, c.name)}
                    </button>
                  ))}
                </div>
              </div>
            ) : r.status === "none" && (looked || !r.login) ? (
              <span key={r.input} className="rev-note rev-note--warn">
                {t(lang, !looked ? "reviewerNotLogin" : r.login ? "reviewerKept" : "reviewerSkipped", { input: r.input, org })}
              </span>
            ) : null,
          )}
        </div>
      )}
    </div>
  );
}
