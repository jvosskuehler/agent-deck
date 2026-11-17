// Beispieldaten des PR-Feeds und des Kommentar-Fensters für den Browser-Mock. Alles
// erfunden und neutral (das Repo ist öffentlich): Platzhalter-Logins, kurze Repo-Namen,
// ausgedachte Titel. `?popup=0` blendet das Kommentar-Fenster aus, `?popup=draft` zeigt
// es mit einem fertigen Entwurf.

import type { CommentPopup } from "../../bindings/CommentPopup";
import type { FeedEvent } from "../../bindings/FeedEvent";
import type { FeedView } from "../../bindings/FeedView";
import type { NeedCard } from "../../bindings/NeedCard";
import type { Lang } from "../../logic/i18n";

const url = (repo: string, n: number, frag = "") => `https://github.com/example/${repo}/pull/${n}${frag}`;

function cards(lang: Lang, now: number): NeedCard[] {
  const en = lang === "en";
  return [
    {
      key: "example/deck#92",
      initials: "R1",
      login: "reviewer1",
      label: "deck#92",
      title: "Review gate: count large refactors separately",
      url: url("deck", 92, "#discussion_r1"),
      at: now - 4 * 60,
      verb: en ? "commented" : "kommentiert",
      excerpt: "Why does an incomplete review always count as high? That would send every large refactor to a human.",
      more: 1,
      agent: "",
      draft: null,
      canDelegate: true,
    },
    {
      key: "example/api#214",
      initials: "R2",
      login: "reviewer2",
      label: "api#214",
      title: "Retry the export once on a timeout",
      url: url("api", 214, "#discussion_r2"),
      at: now - 52 * 60,
      verb: en ? "requested changes" : "Änderungen gewünscht",
      excerpt: "Please move the retry into the client, the handler should stay thin.",
      more: 0,
      agent: "",
      draft: {
        text: "Good point, moved the retry into the export client in a1b2c3d. The handler now only maps the result.",
        thread: true,
        sending: false,
        error: "",
      },
      canDelegate: false,
    },
  ];
}

function events(lang: Lang, now: number): FeedEvent[] {
  const en = lang === "en";
  const ev = (minAgo: number, kind: FeedEvent["kind"], label: string, text: string, detail = "", report = ""): FeedEvent => {
    const [repo, n] = label.split("#");
    return { at: now - minAgo * 60, kind, label, url: label ? url(repo, Number(n)) : "", text, detail, report };
  };
  return [
    ev(9, "merged", "client#393", en ? "merged, branch and worktree removed" : "gemergt, Branch und Worktree weg", en ? "Hotkey fields with pen" : "Hotkey-Felder mit Stift"),
    ev(31, "agent", "client#392", en ? "2 bot findings fixed, threads answered and resolved" : "2 Bot-Funde umgesetzt, Threads beantwortet und aufgelöst", "", "Fixed both findings in 4e5f6a7:\n- null check before the update call\n- log line without the window title\n\nAnswered and resolved both threads."),
    ev(46, "problem", "ui#141", en ? "CI turned red" : "CI rot geworden", en ? "unit tests" : "Unit-Tests"),
    ev(58, "minor", "api#214", en ? "CI red, explained" : "CI rot, erklärt", en ? "code security is off in the organisation" : "Code Security ist in der Organisation aus"),
    ev(74, "review", "api#214", en ? "review from reviewer2" : "Review von reviewer2", en ? "changes requested" : "Änderungen gewünscht"),
    ev(95, "agent", "client#339", en ? "4 conflicts in the updater resolved" : "4 Konflikte im Updater gelöst", "", "Rebased onto main, resolved 4 conflicts in the updater. Tests green."),
    ev(130, "problem", "deck#88", en ? "merge conflict" : "Merge-Konflikt entstanden"),
    ev(190, "minor", "", en ? "4 PRs: reviewer and assignee filled in" : "4 PRs: Reviewer und Assignee nachgetragen"),
    ev(260, "minor", "client#392", en ? "CI turned green" : "CI grün geworden"),
  ];
}

export function mockFeed(lang: Lang, now: number): FeedView {
  const en = lang === "en";
  return {
    needs: cards(lang, now),
    agents: [
      { id: "run-1", label: "client#395", url: url("client", 395), task: en ? "fix bot findings" : "Bot-Funde umsetzen", state: en ? "for 3 min" : "seit 3 Min.", waiting: false },
      { id: "", label: "ui#141", url: url("ui", 141), task: en ? "look into red CI" : "rote CI untersuchen", state: en ? "waiting, session active" : "wartet, Session aktiv", waiting: true },
    ],
    events: events(lang, now),
    auto: [
      { key: "example/client#395", label: "client#395", url: url("client", 395), title: "Show the update size before the download", state: en ? "merges once everything is green" : "mergt, sobald alles grün ist", held: false },
      { key: "example/ui#142", label: "ui#142", url: url("ui", 142), title: "Export button in the report list", state: en ? "waits for api#214" : "wartet auf api#214", held: false },
      { key: "example/deck#88", label: "deck#88", url: url("deck", 88), title: "Group the info tiles by repo", state: en ? "held" : "gehalten", held: true },
    ],
    automation: true,
    note: "",
  };
}

export function mockPopup(lang: Lang, now: number, mode: string | null): CommentPopup | null {
  if (mode === "0") return null;
  const all = cards(lang, now);
  const card = mode === "draft" ? all[1] : all[0];
  const text =
    mode === "draft"
      ? card.excerpt
      : "Why does an incomplete review always count as high? That would send every large refactor to a human, even when the bot only ran out of time. Could we count it as medium and only escalate on real findings?";
  return { card, text };
}

/** Wie das echte Deck auf die Knöpfe des Feeds reagiert, soweit man es im Browser sieht. */
export function applyFeedAction(feed: FeedView, popup: CommentPopup | null, command: string): { feed: FeedView; popup: CommentPopup | null } {
  const i = command.indexOf(":");
  const verb = i < 0 ? command : command.slice(0, i);
  const arg = i < 0 ? "" : command.slice(i + 1);
  const f: FeedView = structuredClone(feed);
  let p = popup;
  switch (verb) {
    case "automation":
      f.automation = arg === "on";
      break;
    case "hold":
    case "release":
      f.auto = f.auto.map((r) => (r.key === arg ? { ...r, held: verb === "hold", state: verb === "hold" ? "gehalten" : "mergt, sobald alles grün ist" } : r));
      break;
    case "later":
      f.needs = f.needs.filter((c) => c.key !== arg);
      if (p?.card.key === arg) p = null;
      break;
    case "popupClose":
      if (p?.card.key === arg) p = null;
      break;
    case "delegate":
      f.needs = f.needs.map((c) => (c.key === arg ? { ...c, canDelegate: false, agent: "Claude setzt um, seit 0 Min." } : c));
      break;
    case "draftSend":
      f.needs = f.needs.filter((c) => c.key !== arg);
      if (p?.card.key === arg) p = null;
      break;
    case "agentAbort":
      f.agents = f.agents.filter((a) => a.id !== arg);
      break;
  }
  return { feed: f, popup: p };
}
