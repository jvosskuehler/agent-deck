//! Der Merker: welche Prozesse stammen aus welchem Agentenfenster, und welche Fenster
//! sind zu.
//!
//! Windows hängt Waisen nirgends um: stirbt die Shell eines Agenten, zeigt die
//! Eltern-PID ihrer Kinder ins Leere (oder später auf einen fremden Prozess, der die
//! Nummer neu bekommen hat). Darum merkt sich das Deck, solange der Agent lebt, jeden
//! Nachkommen mit PID UND Startzeit. Zugehörig ist ein Prozess, wenn
//!
//! 1. er selbst schon einmal unter der Shell gesehen wurde, oder
//! 2. seine Eltern-Kette eine Agenten-Shell oder einen gemerkten Prozess erreicht. Ein
//!    lebender Elter zählt nur, wenn er VOR dem Kind gestartet ist (sonst trägt er die
//!    Nummer eines toten Elters); ein toter nur, wenn das Kind vor dessen Tod (bzw. dem
//!    ersten Blick, der ihn vermisste) gestartet ist. So werden Kinder erkannt, die kurz
//!    vor dem Schließen gestartet und noch nie gesehen wurden.
//!
//! 3. die Eltern-Kette reißt (ein Zwischenglied endete, bevor ein Blick es sah), aber
//!    seine Umgebung die Marke der Shell trägt (`super::marker`). Gelesen wird sie nur für
//!    Prozesse, die nach der ältesten bekannten Shell gestartet sind, je Prozess einmal.
//!
//! Ist die Shell weg, ist jeder lebende gemerkte Prozess eine Waise. Tote Einträge
//! bleiben noch [`KEEP_GONE_S`] stehen: genau für Regel 2, wenn ein Kind erst beim
//! nächsten Blick auftaucht.

use std::collections::{BTreeMap, HashMap, HashSet};

use super::marker::marks_shell;
use super::model::{Key, ProcDetails, ProcRow, Saved, ShellId, ShellRecord, Tracked, filetime_to_unix};
use crate::wt::same_start;

/// So lange (s) bleiben tote Prozesse und geschlossene Shells ohne Waisen gemerkt.
pub const KEEP_GONE_S: f64 = 30.0;
/// Lag der letzte Blick auf eine Shell länger zurück (s; das Deck lief nicht), gilt er als
/// Zeitpunkt des Schließens: genauer weiß es niemand.
pub const STALE_SEEN_S: f64 = 10.0;
/// Spielraum (s) beim Vergleich „Kind gestartet vor dem Tod des Elters“: der Tod ist nur
/// auf einen Blick genau bekannt, und Startzeiten aus der `.wtpid` auf eine Sekunde.
pub const CLOSE_SLACK_S: f64 = 5.0;
/// Tiefer sucht niemand nach Eltern (schützt vor Zyklen aus neu vergebenen PIDs).
const MAX_DEPTH: usize = 64;

type Alive<'a> = HashMap<u32, (&'a ProcRow, u64)>;

/// Liest die Herkunftsmarke eines Prozesses (PID, Startzeit) aus seiner Umgebung; `None` =
/// keine Marke oder nicht lesbar.
pub type MarkerFn<'a> = dyn FnMut(u32, u64) -> Option<(u32, i64)> + 'a;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tracker {
    shells: Vec<ShellRecord>,
    procs: BTreeMap<Key, Tracked>,
    /// Schon gelesene Marken lebender, nicht gemerkter Prozesse (nur im Speicher): jede
    /// Umgebung wird einmal gelesen, eine Marke ohne passende Shell bei jedem Blick neu
    /// verglichen (die `.wtpid` kann später kommen).
    markers: HashMap<Key, Option<(u32, i64)>>,
}

impl Tracker {
    pub fn from_saved(saved: Saved) -> Tracker {
        Tracker { shells: saved.shells, procs: saved.procs.into_iter().map(|t| (t.key(), t)).collect(), markers: HashMap::new() }
    }

    pub fn to_saved(&self, now: f64) -> Saved {
        Saved { saved_at: now, shells: self.shells.clone(), procs: self.procs.values().cloned().collect() }
    }

    pub fn shells(&self) -> &[ShellRecord] {
        &self.shells
    }

    pub fn shell(&self, key: Key) -> Option<&ShellRecord> {
        self.shells.iter().find(|r| r.key() == key)
    }

    pub fn get(&self, key: Key) -> Option<&Tracked> {
        self.procs.get(&key)
    }

    pub fn tracked(&self) -> impl Iterator<Item = &Tracked> {
        self.procs.values()
    }

    /// Lebende Agenten-Shells (PID, Startzeit): auf deren Ende wartet der Dienst.
    pub fn live_shells(&self) -> Vec<Key> {
        self.shells.iter().filter(|r| r.closed_at.is_none()).map(ShellRecord::key).collect()
    }

    /// Lebende gemerkte Prozesse, deren Fenster zu ist. Ungefiltert: was davon geschützt
    /// ist, entscheidet [`super::rows`].
    pub fn orphans(&self) -> Vec<&Tracked> {
        self.procs
            .values()
            .filter(|t| t.alive() && self.shell(t.shell_key()).is_some_and(|s| s.closed_at.is_some()))
            .collect()
    }

    fn is_shell(&self, key: Key) -> bool {
        self.shells.iter().any(|r| r.key() == key)
    }

    /// Einen Schnappschuss einarbeiten. `shells` sind die Agenten laut `.wtpid`-Dateien;
    /// `marker` liest die Herkunftsmarke (Regel 3); `details` liest Kommandozeile und
    /// Sitzung nach (nur für neu gemerkte Prozesse, `None` = nicht mehr derselbe Prozess).
    /// Rückgabe: hat sich der Merker geändert?
    pub fn observe(
        &mut self,
        now: f64,
        shells: &[ShellId],
        procs: &[ProcRow],
        marker: &mut MarkerFn,
        details: &mut dyn FnMut(u32, u64) -> Option<ProcDetails>,
    ) -> bool {
        let alive: Alive = procs.iter().filter_map(|p| p.start.map(|s| (p.pid, (p, s)))).collect();
        let is_alive = |k: Key| alive.get(&k.0).is_some_and(|(_, s)| *s == k.1);
        let mut changed = self.update_shells(now, shells, &alive);
        // Erst vermissen, dann zuordnen: ein Elter, der seit dem letzten Blick gestorben
        // ist, zählt schon als tot (Regel 2 mit seinem Todeszeitpunkt).
        for t in self.procs.values_mut() {
            if t.gone_at.is_none() && !is_alive(t.key()) {
                t.gone_at = Some(now);
                changed = true;
            }
        }
        let mut memo = HashMap::new();
        let mut found = Vec::new();
        for p in procs {
            let Some(start) = p.start else { continue };
            if self.procs.contains_key(&(p.pid, start)) || self.is_shell((p.pid, start)) {
                continue;
            }
            if let Some(shell) = self.owner(p, start, &alive, &mut memo) {
                found.push((p, start, shell));
            }
        }
        found.extend(self.by_marker(procs, &found, marker));
        self.markers.retain(|k, _| is_alive(*k));
        for (p, start, shell) in found {
            let Some(d) = details(p.pid, start) else { continue };
            let t = Tracked {
                pid: p.pid,
                start,
                ppid: p.ppid,
                exe: p.exe.clone(),
                cmdline: d.cmdline,
                image: d.image,
                session: d.session,
                shell_pid: shell.0,
                shell_start: shell.1,
                first_seen: now,
                gone_at: None,
            };
            self.procs.insert(t.key(), t);
            changed = true;
        }
        changed | self.prune(now)
    }

    fn update_shells(&mut self, now: f64, shells: &[ShellId], alive: &Alive) -> bool {
        let mut changed = false;
        for id in shells {
            let Some((_, start)) = alive.get(&id.pid) else { continue };
            if !same_start(*start as i64, id.created) {
                continue;
            }
            match self.shells.iter_mut().find(|r| r.key() == (id.pid, *start)) {
                Some(r) if r.slot != id.slot => {
                    r.slot = id.slot.clone();
                    changed = true;
                }
                Some(_) => {}
                None => {
                    self.shells.push(ShellRecord { slot: id.slot.clone(), pid: id.pid, start: *start, last_seen: now, closed_at: None });
                    changed = true;
                }
            }
        }
        for r in &mut self.shells {
            let lives = alive.get(&r.pid).is_some_and(|(_, s)| *s == r.start);
            if lives {
                r.last_seen = now;
                changed |= r.closed_at.take().is_some();
            } else if r.closed_at.is_none() {
                r.closed_at = Some(if now - r.last_seen > STALE_SEEN_S { r.last_seen } else { now });
                changed = true;
            }
        }
        changed
    }

    /// Regel 3: noch nicht zugeordnete Prozesse über ihre Herkunftsmarke. Wer vor der
    /// ältesten bekannten Shell gestartet ist, kann kein Nachkomme sein und wird gar nicht
    /// erst gelesen; die Shell muss vor dem Prozess gestartet sein.
    fn by_marker<'a>(&mut self, procs: &'a [ProcRow], found: &[(&ProcRow, u64, Key)], marker: &mut MarkerFn) -> Vec<(&'a ProcRow, u64, Key)> {
        let Some(earliest) = self.shells.iter().map(|r| r.start).min() else { return Vec::new() };
        let done: HashSet<Key> = found.iter().map(|(p, s, _)| (p.pid, *s)).collect();
        let mut out = Vec::new();
        for p in procs {
            let Some(start) = p.start else { continue };
            let key = (p.pid, start);
            if start < earliest || done.contains(&key) || self.procs.contains_key(&key) || self.is_shell(key) {
                continue;
            }
            let Some(m) = *self.markers.entry(key).or_insert_with(|| marker(p.pid, start)) else { continue };
            if let Some(r) = self.shells.iter().find(|r| r.start <= start && marks_shell(m, r.pid, r.start)) {
                out.push((p, start, r.key()));
            }
        }
        out
    }

    /// Zu welcher Agenten-Shell gehört dieser (noch nicht gemerkte) Prozess?
    fn owner(&self, p: &ProcRow, start: u64, alive: &Alive, memo: &mut HashMap<u32, Option<Key>>) -> Option<Key> {
        let mut visited = Vec::new();
        let (mut pid, mut ppid, mut cur_start) = (p.pid, p.ppid, start);
        let mut result = None;
        for _ in 0..MAX_DEPTH {
            if self.is_shell((pid, cur_start)) {
                result = Some((pid, cur_start));
                break;
            }
            if let Some(t) = self.procs.get(&(pid, cur_start)) {
                result = Some(t.shell_key());
                break;
            }
            if let Some(m) = memo.get(&pid) {
                result = *m;
                break;
            }
            visited.push(pid);
            if ppid == 0 || ppid == pid {
                break;
            }
            match alive.get(&ppid) {
                // Ein lebender Elter, der vor dem Kind da war, IST der Elter: zwei
                // gleichzeitig lebende Prozesse haben nie dieselbe PID.
                Some((q, qs)) if *qs <= cur_start => {
                    (pid, ppid, cur_start) = (q.pid, q.ppid, *qs);
                }
                _ => {
                    result = self.dead_parent(ppid, cur_start);
                    break;
                }
            }
        }
        for v in visited {
            memo.insert(v, result);
        }
        result
    }

    /// Ein toter Elter aus dem Merker: dieselbe PID, vor dem Kind gestartet, und das Kind
    /// ist vor seinem Tod entstanden.
    fn dead_parent(&self, ppid: u32, child_start: u64) -> Option<Key> {
        let child = filetime_to_unix(child_start);
        let before_death = |end: Option<f64>| end.is_some_and(|e| child <= e + CLOSE_SLACK_S);
        if let Some(r) =
            self.shells.iter().filter(|r| r.pid == ppid && r.start <= child_start && before_death(r.closed_at)).max_by_key(|r| r.start)
        {
            return Some(r.key());
        }
        self.procs
            .values()
            .filter(|t| t.pid == ppid && t.start <= child_start && before_death(t.gone_at))
            .max_by_key(|t| t.start)
            .map(Tracked::shell_key)
    }

    fn prune(&mut self, now: f64) -> bool {
        let before = (self.procs.len(), self.shells.len());
        self.procs.retain(|_, t| t.gone_at.is_none_or(|g| now - g <= KEEP_GONE_S));
        let with_orphans: HashSet<Key> = self.procs.values().filter(|t| t.alive()).map(Tracked::shell_key).collect();
        self.shells.retain(|r| r.closed_at.is_none_or(|c| now - c <= KEEP_GONE_S) || with_orphans.contains(&r.key()));
        before != (self.procs.len(), self.shells.len())
    }
}
