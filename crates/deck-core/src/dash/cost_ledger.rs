//! Gedächtnis der Kostenrechnung: alle Antworten der letzten 30 Tage mit Kosten und den
//! Ticket-Hinweisen ihrer Datei, daraus die Ansicht je Zeitraum ([`CostLedger::view`]).
//! Regeln der Zuordnung stehen in `costs.rs`.

use std::collections::{BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::costs::{read_line, CostMix};

/// Eine Antwort im Gedächtnis des Ledgers.
#[derive(Debug, Clone)]
struct Booked {
    at: f64,
    /// Ticket der Sitzung zu diesem Zeitpunkt (zuletzt benutzter Worktree).
    current: Option<u32>,
    model: u16,
    mix: CostMix,
}

/// Was der Ledger je Transcript-Datei weiß.
#[derive(Debug, Clone, Default)]
struct Track {
    /// Sitzung, deren Ticket ein Subagent ohne eigenen Treffer erbt.
    parent: Option<String>,
    current: Option<u32>,
    tickets: BTreeSet<u32>,
    /// (Zeit, Ticket) je Treffer, für Subagenten.
    timeline: Vec<(f64, u32)>,
    replies: Vec<Booked>,
}

impl Track {
    fn only(&self) -> Option<u32> {
        (self.tickets.len() == 1).then(|| *self.tickets.iter().next().unwrap_or(&0))
    }

    fn at(&self, t: f64) -> Option<u32> {
        let i = self.timeline.partition_point(|(at, _)| *at <= t);
        if i > 0 { Some(self.timeline[i - 1].1) } else { self.only() }
    }
}

/// Alle Antworten der letzten Wochen mit Kosten und Ticket-Hinweisen.
#[derive(Debug, Clone, Default)]
pub struct CostLedger {
    tracks: HashMap<String, Track>,
    seen: HashSet<String>,
    tickets: Vec<String>,
    ticket_ix: HashMap<String, u32>,
    models: Vec<String>,
    /// Antworten ohne Preis je Modell (fürs Protokoll).
    pub unpriced: HashMap<String, u64>,
}

impl CostLedger {
    fn ticket(&mut self, name: &str) -> u32 {
        if let Some(i) = self.ticket_ix.get(name) {
            return *i;
        }
        let i = self.tickets.len() as u32;
        self.tickets.push(name.to_string());
        self.ticket_ix.insert(name.to_string(), i);
        i
    }

    fn model(&mut self, name: &str) -> u16 {
        match self.models.iter().position(|m| m == name) {
            Some(i) => i as u16,
            None => {
                self.models.push(name.to_string());
                (self.models.len() - 1) as u16
            }
        }
    }

    /// Eine Zeile der Datei `key` verbuchen. `parent` = Sitzung eines Subagenten
    /// (Schlüssel ihrer Datei). Gibt zurück, ob eine neue Antwort dazukam.
    pub fn add_line(&mut self, key: &str, parent: Option<&str>, line: &str, prefix: &str) -> bool {
        let Some(f) = read_line(line, prefix) else { return false };
        let hits: Vec<u32> = f.hits.iter().map(|h| self.ticket(h)).collect();
        let model = f.reply.as_ref().map(|r| self.model(&r.model));
        let new_id = f.reply.as_ref().is_some_and(|r| self.seen.insert(r.id.clone()));
        let track = self.tracks.entry(key.to_string()).or_default();
        if track.parent.is_none() {
            track.parent = parent.map(String::from);
        }
        for h in hits {
            track.current = Some(h);
            track.tickets.insert(h);
            track.timeline.push((f.at, h));
        }
        if let Some(name) = f.unpriced {
            *self.unpriced.entry(name).or_default() += 1;
        }
        match (f.reply, model) {
            (Some(r), Some(m)) if new_id => {
                track.replies.push(Booked { at: f.at, current: track.current, model: m, mix: r.mix });
                true
            }
            _ => false,
        }
    }

    /// Antworten vor `before` vergessen (die Ansicht reicht 30 Tage zurück).
    pub fn prune(&mut self, before: f64) {
        for t in self.tracks.values_mut() {
            t.replies.retain(|r| r.at >= before);
        }
    }

    /// Zahl der gemerkten Antworten.
    pub fn len(&self) -> usize {
        self.tracks.values().map(|t| t.replies.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Jede Antwort mit ihrem endgültigen Ticket: (Zeit, Ticket oder `None`, Modell, Kosten).
    fn attributed(&self) -> Vec<(f64, Option<u32>, u16, CostMix)> {
        let mut out = Vec::with_capacity(self.len());
        for t in self.tracks.values() {
            let parent = t.parent.as_ref().and_then(|p| self.tracks.get(p));
            for r in &t.replies {
                let ticket = r.current.or_else(|| t.only()).or_else(|| parent.and_then(|p| p.at(r.at)));
                out.push((r.at, ticket, r.model, r.mix));
            }
        }
        out
    }

    /// Die Ansicht für `now` (Unix-Sekunden) bei Ortszeit `offset_s` (MEZ: 3600).
    pub fn view(&self, now: f64, offset_s: i64) -> CostView {
        let all = self.attributed();
        let month_start = now - 30.0 * 86_400.0;
        let mut month: HashMap<u32, f64> = HashMap::new();
        for (at, t, _, mix) in &all {
            if let (Some(t), true) = (t, *at >= month_start) {
                *month.entry(*t).or_default() += mix.total();
            }
        }
        let mut ranges: Vec<CostRange> = RANGES.iter().map(|r| self.range(r, &all, now, offset_s)).collect();
        // Farb-Tickets: je Zeitraum die bis zu sechs größten mit mindestens 3 %, vereinigt,
        // nach 30-Tage-Verbrauch, höchstens acht. Ein Ticket behält so seine Farbe.
        let mut pick: BTreeSet<String> = BTreeSet::new();
        for r in &ranges {
            let total: f64 = r.series.iter().map(|s| s.mix.total()).sum();
            let mut named: Vec<&CostSeries> = r.series.iter().filter(|s| !s.ticket.is_empty()).collect();
            named.sort_by(|a, b| b.mix.total().total_cmp(&a.mix.total()));
            pick.extend(named.iter().take(6).filter(|s| total > 0.0 && s.mix.total() / total >= 0.03).map(|s| s.ticket.clone()));
        }
        let vol = |name: &str| self.ticket_ix.get(name).and_then(|i| month.get(i)).copied().unwrap_or(0.0);
        let mut colored: Vec<String> = pick.into_iter().collect();
        colored.sort_by(|a, b| vol(b).total_cmp(&vol(a)).then(a.cmp(b)));
        colored.truncate(8);
        for r in &mut ranges {
            r.series.sort_by(|a, b| b.mix.total().total_cmp(&a.mix.total()).then(a.ticket.cmp(&b.ticket)));
        }
        let first_at = all.iter().map(|(at, ..)| *at).filter(|at| *at >= month_start).fold(None, |m: Option<f64>, at| Some(m.map_or(at, |m| m.min(at))));
        CostView { ranges, colored, first_at, stand: now }
    }

    fn range(&self, spec: &RangeSpec, all: &[(f64, Option<u32>, u16, CostMix)], now: f64, offset_s: i64) -> CostRange {
        let step = spec.step_s;
        let off = offset_s as f64;
        // Letzter Eimer: der, in dem `now` liegt, an Ortszeit ausgerichtet.
        let last = ((now + off) / step).floor() * step - off;
        let n = spec.buckets;
        let t0 = last - (n as f64 - 1.0) * step;
        let t1 = last + step;
        let mut series: HashMap<Option<u32>, (Vec<f64>, CostMix)> = HashMap::new();
        let mut models: HashMap<u16, f64> = HashMap::new();
        let mut prev = 0.0;
        for (at, t, m, mix) in all {
            if *at >= t0 - (t1 - t0) && *at < t0 {
                prev += mix.total();
            }
            if *at < t0 || *at >= t1 {
                continue;
            }
            let i = (((*at - t0) / step) as usize).min(n - 1);
            let e = series.entry(*t).or_insert_with(|| (vec![0.0; n], CostMix::default()));
            e.0[i] += mix.total();
            e.1.add(mix);
            *models.entry(*m).or_default() += mix.total();
        }
        let mut models: Vec<CostModel> =
            models.into_iter().map(|(m, v)| CostModel { model: self.models[m as usize].clone(), value: v.round() }).collect();
        models.sort_by(|a, b| b.value.total_cmp(&a.value));
        CostRange {
            key: spec.key.into(),
            starts: (0..n).map(|i| t0 + i as f64 * step).collect(),
            step_s: step,
            series: series
                .into_iter()
                .map(|(t, (values, mix))| CostSeries {
                    ticket: t.map(|i| self.tickets[i as usize].clone()).unwrap_or_default(),
                    values: values.into_iter().map(f64::round).collect(),
                    mix: mix.rounded(),
                })
                .collect(),
            prev: prev.round(),
            models,
        }
    }
}

struct RangeSpec {
    key: &'static str,
    step_s: f64,
    buckets: usize,
}

/// Die Zeiträume der Ansicht: letzte Stunde in Minuten, 24 Stunden in Viertelstunden,
/// 7 Tage in Stunden, 30 Tage in Viertel-Tagen.
const RANGES: [RangeSpec; 4] = [
    RangeSpec { key: "h1", step_s: 60.0, buckets: 60 },
    RangeSpec { key: "d1", step_s: 900.0, buckets: 96 },
    RangeSpec { key: "w1", step_s: 3_600.0, buckets: 168 },
    RangeSpec { key: "m1", step_s: 21_600.0, buckets: 120 },
];

/// Eine Reihe: ein Ticket über die Eimer eines Zeitraums.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CostSeries {
    /// Ticket; `""` = ohne Ticket.
    pub ticket: String,
    /// Kosten je Eimer (Input-Tokens von Opus 5.5).
    pub values: Vec<f64>,
    /// Zusammensetzung über den ganzen Zeitraum.
    pub mix: CostMix,
}

/// Anteil eines Modells an einem Zeitraum.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CostModel {
    pub model: String,
    pub value: f64,
}

/// Ein Zeitraum.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CostRange {
    /// `h1`, `d1`, `w1`, `m1`.
    pub key: String,
    /// Beginn jedes Eimers (Unix-Sekunden).
    pub starts: Vec<f64>,
    pub step_s: f64,
    /// Größte zuerst.
    pub series: Vec<CostSeries>,
    /// Kosten im gleich langen Zeitraum davor.
    pub prev: f64,
    /// Größte zuerst.
    pub models: Vec<CostModel>,
}

/// Die Kostenansicht.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CostView {
    pub ranges: Vec<CostRange>,
    /// Tickets mit eigener Farbe, in Farbreihenfolge (höchstens acht).
    pub colored: Vec<String>,
    /// Älteste Antwort der letzten 30 Tage (die Transcripts reichen nicht weiter zurück).
    pub first_at: Option<f64>,
    /// Stand (Unix-Sekunden).
    pub stand: f64,
}

#[cfg(test)]
#[path = "cost_ledger_tests.rs"]
mod tests;
