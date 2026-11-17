// Mock für set_reviewers: eine erfundene Organisation und eine vereinfachte Fassung der
// Zuordnung aus `deck_core::dash::reviewer_match` (Login genau, Mailadresse genau, ganzer
// Name, Wörter genau, Wortanfang). Nur für den Browser, die echte Rechnung macht Rust.

import type { ReviewerReport } from "../../bindings/ReviewerReport";
import type { ReviewerResult } from "../../bindings/ReviewerResult";

interface Person {
  login: string;
  name: string;
  email?: string;
}

export const MOCK_ORG = "beispiel-org";

const MEMBERS: Person[] = [
  { login: "leo-mxyz", name: "Leo Müller" },
  { login: "lkrause", name: "Leon Krause" },
  { login: "anna-s", name: "Anna Schmidt" },
  { login: "aberger", name: "Anna-Lena Berger", email: "a.berger@firma.example" },
  { login: "tbrandt", name: "Tim Brandt" },
];

function fold(s: string): string {
  return s
    .toLowerCase()
    .replace(/ß/g, "ss")
    .normalize("NFD")
    .replace(/\p{M}/gu, "")
    .replace(/ae/g, "a")
    .replace(/oe/g, "o")
    .replace(/ue/g, "u");
}

const words = (s: string) => fold(s).split(/[^\p{L}\p{N}]+/u).filter(Boolean);
const validLogin = (s: string) => /^[A-Za-z0-9][A-Za-z0-9-]{0,38}$/.test(s);

function split(raw: string): string[] {
  const out: string[] = [];
  for (const part of raw.split(/[,;\n]/)) {
    let t = part.trim();
    const m = /\(([^()]+)\)$/.exec(t);
    if (m && validLogin(m[1].trim())) t = m[1].trim();
    t = t.replace(/^@/, "").trim();
    if (t && !out.some((o) => o.toLowerCase() === t.toLowerCase())) out.push(t);
  }
  return out;
}

function match(input: string): Person[] {
  const lower = input.toLowerCase();
  const tiers: ((p: Person) => boolean)[] = [(p) => p.login.toLowerCase() === lower];
  let text = input;
  if (input.includes("@")) {
    tiers.push((p) => p.email?.toLowerCase() === lower);
    text = input.split("@")[0];
  }
  const q = words(text);
  const own = (p: Person) => [...words(p.name), ...words(p.login)];
  tiers.push(
    (p) => words(p.name).join(" ") === q.join(" "),
    (p) => q.length > 0 && q.every((w) => own(p).includes(w)),
    (p) => q.length > 0 && q.every((w) => own(p).some((o) => o.startsWith(w))),
  );
  for (const tier of tiers) {
    const hits = MEMBERS.filter(tier);
    if (hits.length) return hits;
  }
  return [];
}

/** `org`: eingestellte Organisation; `several`: das Konto hat mehrere (`?orgs=2`). */
export function mockResolve(raw: string, org: string, several: boolean): ReviewerReport {
  const tokens = split(raw);
  const none = (input: string): ReviewerResult => ({
    input,
    status: "none",
    login: validLogin(input) ? input : "",
    name: "",
    candidates: [],
  });
  if (tokens.length && !org && several) {
    return { org: "", orgs: [MOCK_ORG, "privat-projekte"], note: "chooseOrg", detail: "", results: tokens.map(none) };
  }
  const results = tokens.map((input): ReviewerResult => {
    const hits = match(input);
    if (hits.length === 1) return { input, status: "unique", login: hits[0].login, name: hits[0].name, candidates: [] };
    if (hits.length > 1)
      return { input, status: "ambiguous", login: "", name: "", candidates: hits.map(({ login, name }) => ({ login, name })) };
    return none(input);
  });
  return { org: tokens.length ? org || MOCK_ORG : "", orgs: [], note: "none", detail: "", results };
}

/** Gespeicherte Logins und ihre Anzeige wie `Deck::apply_reviewers` und `reviewers::label`. */
export function mockApply(report: ReviewerReport, names: Map<string, string>): { logins: string; label: string } {
  const logins: string[] = [];
  for (const r of report.results) {
    if (r.status === "ambiguous" || !r.login) continue;
    if (r.status === "unique" && r.name) names.set(r.login, r.name);
    if (!logins.some((l) => l.toLowerCase() === r.login.toLowerCase())) logins.push(r.login);
  }
  const label = logins.map((l) => (names.get(l) ? `${names.get(l)} (${l})` : l)).join(", ");
  return { logins: logins.join(", "), label };
}
