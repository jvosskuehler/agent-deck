// Beispieldaten des Info-Fensters für den Browser-Mock (`pnpm dev`): Kosten je Ticket
// über vier Zeiträume, PRs, Reviews, ein verwaister Titan-Lauf, Worktrees. Die Zahlen
// sind erfunden, aber in der Größenordnung echter Wochen (Stand 16.11.2026).

import type { CostRange } from "../../bindings/CostRange";
import type { DashView } from "../../bindings/DashView";
import type { Lang } from "../../logic/i18n";
import { mockFeed } from "./mockFeed";

const TICKETS = ["agent-deck", "ORPH-4258", "ORPH-4259", "ORPH-3289", "ORPH-3879", "ORPH-4256", "ORPH-4254", "ORPH-4260"];
const SMALL = ["ORPH-4081", "ORPH-4220", "ORPH-4173", "ORPH-4096", "ORPH-4189"];

interface Spec {
  key: string;
  step: number;
  n: number;
  total: number;
  prev: number;
  /** Je Ticket: Anteil und Mitte des Höckers (0–1). */
  mix: [string, number, number][];
}

const SPECS: Spec[] = [
  { key: "h1", step: 60, n: 60, total: 1.8e6, prev: 1.1e6, mix: [["agent-deck", 0.9, 0.6], ["", 0.1, 0.5]] },
  { key: "d1", step: 900, n: 96, total: 42e6, prev: 197e6, mix: [["agent-deck", 0.38, 0.95], ["ORPH-4254", 0.32, 0.7], ["ORPH-4259", 0.17, 0.75], ["ORPH-4260", 0.1, 0.65], ["", 0.03, 0.5]] },
  {
    key: "w1",
    step: 3600,
    n: 168,
    total: 637e6,
    prev: 351e6,
    mix: [["agent-deck", 0.13, 0.9], ["ORPH-4258", 0.13, 0.4], ["ORPH-4259", 0.1, 0.55], ["ORPH-3289", 0.08, 0.2], ["ORPH-3879", 0.05, 0.3], ["ORPH-4256", 0.05, 0.6], ["ORPH-4254", 0.03, 0.95], ["ORPH-4081", 0.12, 0.45], ["ORPH-4220", 0.11, 0.5], ["ORPH-4173", 0.06, 0.35], ["ORPH-4096", 0.05, 0.25], ["", 0.08, 0.5]],
  },
  {
    key: "m1",
    step: 21600,
    n: 120,
    total: 988e6,
    prev: 0,
    mix: [["agent-deck", 0.13, 0.9], ["ORPH-4258", 0.08, 0.75], ["ORPH-4259", 0.06, 0.8], ["ORPH-3289", 0.05, 0.6], ["ORPH-3879", 0.03, 0.7], ["ORPH-4256", 0.03, 0.8], ["ORPH-4189", 0.1, 0.6], ["ORPH-4173", 0.1, 0.65], ["", 0.37, 0.55]],
  },
];

function range(spec: Spec, now: number): CostRange {
  const last = Math.floor((now + 3600) / spec.step) * spec.step - 3600;
  const starts = Array.from({ length: spec.n }, (_, i) => last - (spec.n - 1 - i) * spec.step);
  let seed = spec.key.charCodeAt(0) * 31 + spec.n;
  const rnd = () => (seed = (seed * 16807) % 2147483647) / 2147483647;
  const series = spec.mix.map(([ticket, share, mid]) => {
    const raw = starts.map((s, i) => {
      const hour = new Date(s * 1000).getHours();
      const day = hour >= 8 && hour < 23 ? 1 : 0.05;
      const x = i / (spec.n - 1);
      return day * Math.exp(-((x - mid) ** 2) / 0.02) * (0.6 + rnd());
    });
    const sum = raw.reduce((a, b) => a + b, 0) || 1;
    const values = raw.map((v) => Math.round((v / sum) * spec.total * share));
    const v = spec.total * share;
    return { ticket, values, mix: { input: 0, output: Math.round(v * 0.18), write: Math.round(v * 0.28), read: Math.round(v * 0.54) } };
  });
  series.sort((a, b) => b.mix.read - a.mix.read);
  const models =
    spec.key === "w1" || spec.key === "m1"
      ? [
          { model: "claude-opus-5-5", value: spec.total * 0.75 },
          { model: "claude-fable-5-2", value: spec.total * 0.24 },
          { model: "claude-sonnet-5-5", value: spec.total * 0.01 },
        ]
      : [{ model: "claude-opus-5-5", value: spec.total }];
  return { key: spec.key, starts, stepS: spec.step, series, prev: spec.prev, models };
}

export function mockDash(lang: Lang, now: number): DashView {
  const en = lang === "en";
  const gh = (repo: string, n: number) => `https://github.com/Org/orpheus-${repo}/pull/${n}`;
  const pr = (repo: string, number: number, ticket: string, title: string, status: string, tone: DashView["prs"]["rows"][0]["tone"], meta: string) => ({
    repo: repo.replace("management-", "").replace("client-electron", "client"),
    number,
    ticket,
    title,
    url: gh(repo, number),
    status,
    tone,
    meta,
  });
  const wait = en ? "awaiting re-review" : "wartet auf Re-Review";
  const review = en ? "awaiting review" : "wartet auf Review";
  return {
    costs: { ranges: SPECS.map((s) => range(s, now)), colored: TICKETS, firstAt: now - 14 * 86400, stand: now },
    prs: {
      rows: [
        pr("management-api", 212, "ORPH-4255", "Run each scheduled job on one replica only", en ? "ready to merge" : "mergebereit", "good", "CI grün"),
        pr("client-electron", 339, "ORPH-4081", "Install downloaded updates at the next start", en ? "re-request review" : "Reviewer anfragen", "warn", "CI grün"),
        pr("client-electron", 383, "ORPH-4212", "Log every request and send a W3C traceparent header", wait, "plain", "bei reviewer1 · CI grün"),
        pr("client-electron", 385, "ORPH-4220", "Log startup, discovery, sign-in, feature flags and updates", wait, "plain", "bei reviewer1 · CI grün"),
        pr("management-ui", 212, "ORPH-4099", "Run the customer portal on-prem", wait, "plain", "bei reviewer1 · CI grün"),
        pr("management-api", 205, "ORPH-4189", "Unique Keycloak id per user", review, "plain", "bei reviewer1 · CI grün"),
        pr("management-api", 213, "ORPH-4256", "Tests pin the Management API behaviour", review, "plain", "CI grün"),
        pr("devops", 90, "ORPH-4257", "Promote by release group", "Draft", "muted", "CI grün"),
      ],
      ready: 1,
      mine: 1,
      waiting: 5,
      stale: false,
      note: "",
    },
    reviews: [
      { repo: "homepage", number: 77, title: "Admin-Portal-Zeile aus Portal 2026.11.3 entfernen", url: gh("homepage", 77), meta: "seit 2 Tagen · +0 −2" },
      { repo: "helm-charts", number: 54, title: "Mark cloud discovery feature flags as deprecated", url: gh("helm-charts", 54), meta: "seit 3 Tagen · +15 −0" },
      { repo: "ui", number: 141, title: "Angabe Auftragsverarbeitung für Dritte bei der Buchung", url: gh("management-ui", 141), meta: "seit 21 Tagen · +1128 −47" },
      { repo: "api", number: 119, title: "Donation option in the checkout", url: gh("management-api", 119), meta: "seit 40 Tagen · +15302 −51 · CI rot" },
    ],
    hiddenReviews: [{ repo: "dotnet", number: 88, title: "Spike: alternative transcription backend", url: gh("orpheus-services-dotnet", 88), meta: "seit 63 Tagen · +2210 −340" }],
    titan: {
      runs: [
        { name: "4258-w201-c59673", label: "4258-w201", age: en ? "for 42 h" : "seit 42 Std." },
        { name: "4259-p7-5cb7dc", label: "4259-p7", age: en ? "for 26 h" : "seit 26 Std." },
      ],
      facts: "10 Container · 2 VMs an · Last 14 % · 290 GB frei",
    },
    cleanup: {
      removable: [...SMALL, ...TICKETS.slice(1, 4)].map((t, i) => ({ folder: `${t.slice(5)}-${["api", "client", "dotnet", "ui"][i % 4]}`, repo: ["api", "client", "dotnet", "ui"][i % 4], branch: `feat/${t}` })),
      changed: [{ folder: "3289-homepage", repo: "homepage", branch: "feat/ORPH-3289" }],
      busy: false,
      result: "",
    },
    notice: null,
    feed: mockFeed(lang, now),
    popup: null,
  };
}
