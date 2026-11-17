//! Tokenkosten je Ticket über die Zeit: die Hauptansicht des Info-Fensters.
//!
//! **Kosten statt Tokens.** Roh gezählt sind über 95 % der Tokens Cache-Lesen, weil jede
//! Antwort das ganze Gespräch noch einmal mitschickt; die Summe sagt über die Kosten
//! nichts. Jede Antwort zählt darum mit den API-Preisen ihres Modells ([`PRICES`], von
//! platform.claude.com/docs/en/about-claude/pricing, abgerufen 16.11.2026), umgerechnet
//! in Input-Tokens von Opus 5.5 ([`UNIT_USD_PER_MTOK`]). So lassen sich Modelle
//! zusammenzählen, und eine Fable-Antwort zählt doppelt so viel wie dieselbe mit Opus.
//!
//! **Ticket je Antwort.** Das Ticket, in dessen Worktree die Sitzung zuletzt gearbeitet hat:
//! Pfade in den Werkzeugaufrufen der Antworten und im `cwd` (`wt/<nr>-…` = ORPH-<nr>,
//! `wt/deck-…` und `Repos/agent-deck` = agent-deck), dazu ein Branch mit `ORPH-<nr>`.
//! Ein Aufruf, der mehr als drei Tickets berührt (`ls wt/`), zählt nicht. Vor dem ersten
//! Treffer zählt eine Antwort nur, wenn die Sitzung genau ein Ticket hat; ein Subagent
//! ohne eigenen Treffer erbt das Ticket seiner Sitzung zu diesem Zeitpunkt. Der Rest
//! steht unter „ohne Ticket“ (Schlüssel `""`).
//!
//! Das Lesen der Dateien (inkrementell, im Hintergrund) macht
//! `deck_services::dash::costs`; hier steht die reine Rechnung. Inhalte der Transcripts
//! verlassen diese Funktionen nie, nur Ticketnummern, Zeiten und Zahlen.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::isotime::parse_iso;

/// Preise in $ je Mio. Tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Price {
    pub input: f64,
    pub write_5m: f64,
    pub write_1h: f64,
    pub read: f64,
    pub output: f64,
}

const fn p(input: f64, write_5m: f64, write_1h: f64, read: f64, output: f64) -> Price {
    Price { input, write_5m, write_1h, read, output }
}

/// API-Preise je Modell (Standard-Tempo). Haiku 5.5 kostet ab 100 000 Tokens im Prompt das
/// Fünffache ([`HAIKU_LONG`]). Ein Modell, das hier fehlt, zählt nicht mit (das
/// Protokoll des Dienstes nennt es).
pub const PRICES: [(&str, Price); 5] = [
    ("claude-opus-5-5", p(4.0, 5.0, 8.0, 0.20, 20.0)),
    ("claude-fable-5-2", p(8.0, 10.0, 16.0, 0.20, 40.0)),
    ("claude-sonnet-5-5", p(2.0, 2.50, 4.0, 0.10, 10.0)),
    ("claude-haiku-5-5", p(0.10, 0.125, 0.20, 0.01, 0.50)),
    ("claude-opus-5", p(5.0, 6.25, 10.0, 0.50, 25.0)),
];
/// Haiku 5.5 mit mehr als 100 000 Tokens im Prompt.
pub const HAIKU_LONG: Price = p(0.50, 0.625, 1.0, 0.05, 2.50);
/// Einheit der Kosten: Input von Opus 5.5, $ je Mio. Tokens.
pub const UNIT_USD_PER_MTOK: f64 = 4.0;

/// Preis einer Antwort; `None` = unbekanntes Modell oder ein anderes Tempo als Standard.
pub fn price_for(model: &str, prompt_tokens: u64, speed: Option<&str>) -> Option<Price> {
    if speed.is_some_and(|s| s != "standard") {
        return None;
    }
    if model == "claude-haiku-5-5" && prompt_tokens > 100_000 {
        return Some(HAIKU_LONG);
    }
    PRICES.iter().find(|(m, _)| *m == model).map(|(_, p)| *p)
}

/// Kosten einer Antwort je Bestandteil, in Input-Tokens von Opus 5.5.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CostMix {
    pub input: f64,
    pub output: f64,
    /// Cache schreiben (5 Min. und 1 Std.).
    pub write: f64,
    /// Cache lesen.
    pub read: f64,
}

impl CostMix {
    pub fn total(&self) -> f64 {
        self.input + self.output + self.write + self.read
    }

    pub(crate) fn add(&mut self, o: &CostMix) {
        self.input += o.input;
        self.output += o.output;
        self.write += o.write;
        self.read += o.read;
    }

    pub(crate) fn rounded(self) -> CostMix {
        CostMix { input: self.input.round(), output: self.output.round(), write: self.write.round(), read: self.read.round() }
    }
}

/// Rohe Tokens einer Antwort.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub write_5m: u64,
    pub write_1h: u64,
    pub read: u64,
}

/// Kosten dieser Tokens zu diesem Preis.
pub fn cost(u: &Usage, p: &Price) -> CostMix {
    let k = |tokens: u64, usd: f64| tokens as f64 * usd / UNIT_USD_PER_MTOK;
    CostMix {
        input: k(u.input, p.input),
        output: k(u.output, p.output),
        write: k(u.write_5m, p.write_5m) + k(u.write_1h, p.write_1h),
        read: k(u.read, p.read),
    }
}

/// Eine Antwort des Modells mit Kosten.
#[derive(Debug, Clone, PartialEq)]
pub struct CostReply {
    pub id: String,
    pub model: String,
    pub mix: CostMix,
}

/// Was eine Transcript-Zeile hergibt.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LineFacts {
    pub at: f64,
    /// Antwort mit bekanntem Preis.
    pub reply: Option<CostReply>,
    /// Antwort mit Verbrauch, aber ohne Preis (Modellname fürs Protokoll).
    pub unpriced: Option<String>,
    /// Tickets, die diese Zeile berührt (cwd, Branch, Werkzeugaufrufe).
    pub hits: Vec<String>,
}

#[derive(Deserialize)]
struct Line {
    #[serde(rename = "type")]
    kind: Option<String>,
    timestamp: Option<String>,
    cwd: Option<String>,
    #[serde(rename = "gitBranch")]
    git_branch: Option<String>,
    message: Option<Message>,
}

#[derive(Deserialize)]
struct Message {
    id: Option<String>,
    model: Option<String>,
    // `content` fehlt mit Absicht: serde überliest ihn; nur Zeilen mit Pfad liest
    // [`tool_hits`] ein zweites Mal ganz.
    usage: Option<RawUsage>,
}

#[derive(Deserialize)]
struct RawUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    cache_creation: Option<CacheCreation>,
    speed: Option<String>,
}

#[derive(Deserialize)]
struct CacheCreation {
    ephemeral_1h_input_tokens: Option<u64>,
}

static WT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|[^A-Za-z0-9_.-])wt[\\/]+([A-Za-z0-9._-]+)").expect("WT"));
static DECK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?:Repos|Programmchen)[\\/]+agent-deck(?:[\\/]|$)").expect("DECK"));
static BRANCH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b([A-Z][A-Z0-9]+)-(\d{2,6})\b").expect("BRANCH"));
static WT_NR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d{3,5})(?:-|$)").expect("WT_NR"));
static WT_KEY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(cs|isd)-(\d+)").expect("WT_KEY"));

/// Höchstens so viele Tickets darf ein Werkzeugaufruf berühren, sonst ist er ein Überblick.
const OVERVIEW_TICKETS: usize = 3;

/// Ticket zu einem Worktree-Ordner: `4258-api` → `ORPH-4258` (ohne Jira-Projekt die
/// bloße Nummer `4258`, kein ausgedachter Key), `deck-dashboard` → `agent-deck`,
/// `cs-231-x` → `CS-231`; alles andere (`mono`, `manual49-client`) → keins.
pub fn ticket_of_worktree(folder: &str, prefix: &str) -> Option<String> {
    if let Some(c) = WT_NR.captures(folder) {
        return Some(if prefix.is_empty() { c[1].to_string() } else { format!("{prefix}-{}", &c[1]) });
    }
    if folder.to_ascii_lowercase().starts_with("deck") {
        return Some("agent-deck".into());
    }
    WT_KEY.captures(folder).map(|c| format!("{}-{}", c[1].to_ascii_uppercase(), &c[2]))
}

/// Tickets, die ein Text (Pfad, Befehl) berührt.
pub fn hits_in(text: &str, prefix: &str) -> BTreeSet<String> {
    let mut out: BTreeSet<String> = WT.captures_iter(text).filter_map(|c| ticket_of_worktree(&c[1], prefix)).collect();
    if DECK.is_match(text) {
        out.insert("agent-deck".into());
    }
    out
}

fn strings<'a>(v: &'a Value, out: &mut Vec<&'a str>) {
    match v {
        Value::String(s) => out.push(s),
        Value::Array(a) => a.iter().for_each(|x| strings(x, out)),
        Value::Object(o) => o.values().for_each(|x| strings(x, out)),
        _ => {}
    }
}

/// Tickets aus den Werkzeugaufrufen einer Antwortzeile (nur Eingaben der Aufrufe, nie
/// Text oder Ergebnisse).
fn tool_hits(line: &str, prefix: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Ok(v) = serde_json::from_str::<Value>(line) else { return out };
    let Some(content) = v.pointer("/message/content").and_then(Value::as_array) else { return out };
    for block in content {
        if block.get("type").and_then(Value::as_str) != Some("tool_use") {
            continue;
        }
        let mut texts = Vec::new();
        if let Some(input) = block.get("input") {
            strings(input, &mut texts);
        }
        let call: BTreeSet<String> = texts.iter().flat_map(|t| hits_in(t, prefix)).collect();
        if call.len() <= OVERVIEW_TICKETS {
            out.extend(call);
        }
    }
    out
}

/// Eine Transcript-Zeile lesen; `None` = nichts Brauchbares (kaputtes JSON, ohne Zeit).
pub fn read_line(line: &str, prefix: &str) -> Option<LineFacts> {
    // Vorfilter: Zeilen ohne Antwort und ohne möglichen Pfad gar nicht erst zerlegen.
    let maybe_path = line.contains("wt/") || line.contains("wt\\\\") || line.contains("agent-deck");
    let maybe_reply = line.contains("\"usage\"");
    let maybe_branch = !prefix.is_empty() && line.contains(&format!("{prefix}-"));
    if !maybe_path && !maybe_reply && !maybe_branch {
        return None;
    }
    let l: Line = serde_json::from_str(line).ok()?;
    let at = parse_iso(l.timestamp.as_deref()?)?;
    let mut hits = BTreeSet::new();
    if let Some(cwd) = &l.cwd {
        hits.extend(hits_in(cwd, prefix));
    }
    if let Some(c) = l.git_branch.as_deref().and_then(|b| BRANCH.captures(b))
        && !prefix.is_empty()
        && &c[1] == prefix
    {
        hits.insert(format!("{prefix}-{}", &c[2]));
    }
    let assistant = l.kind.as_deref() == Some("assistant");
    if assistant && maybe_path {
        hits.extend(tool_hits(line, prefix));
    }
    let mut facts = LineFacts { at, hits: hits.into_iter().collect(), ..LineFacts::default() };
    if let Some(m) = l.message.filter(|_| assistant) {
        let model = m.model.unwrap_or_default();
        if let (Some(u), Some(id)) = (m.usage, m.id.filter(|s| !s.is_empty()))
            && model != "<synthetic>"
        {
            let write = u.cache_creation_input_tokens.unwrap_or(0);
            let write_1h = u.cache_creation.and_then(|c| c.ephemeral_1h_input_tokens).unwrap_or(0).min(write);
            let usage = Usage {
                input: u.input_tokens.unwrap_or(0),
                output: u.output_tokens.unwrap_or(0),
                write_5m: write - write_1h,
                write_1h,
                read: u.cache_read_input_tokens.unwrap_or(0),
            };
            let prompt = usage.input + write + usage.read;
            match price_for(&model, prompt, u.speed.as_deref()) {
                Some(price) => facts.reply = Some(CostReply { id, model, mix: cost(&usage, &price) }),
                None => facts.unpriced = Some(model),
            }
        }
    }
    Some(facts)
}

#[cfg(test)]
#[path = "costs_tests.rs"]
pub(crate) mod tests;
