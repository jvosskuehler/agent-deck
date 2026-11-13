//! Aus dem Merker die Zeilen für „Läuft noch ohne Fenster“, und was „Beenden“ einer
//! Zeile genau trifft.
//!
//! Eine Zeile ist ein Prozessbaum: `pnpm nx serve` startet node, das node, das den Port
//! hält. Angezeigt wird der oberste noch lebende, nicht geschützte Prozess (dessen
//! Kommandozeile sagt, was es ist), mit den Ports aller darunter. „Beenden“ trifft genau
//! diese Prozesse, von oben nach unten, nie einen geschützten dazwischen.

use std::collections::{HashMap, HashSet};

use super::model::{Key, Tracked, row_id};
use super::rules::{display_command, exe_stem, is_deck_exe, protected};
use super::tracker::Tracker;
use crate::i18n::Lang;
use crate::view::OrphanView;

/// Darf dieser gemerkte Prozess angeboten werden?
fn offerable(t: &Tracked) -> bool {
    protected(t.pid, &t.exe, t.image.as_deref(), t.cmdline.as_deref(), t.session).is_none()
}

/// Lebender gemerkter Elter (vor dem Kind gestartet, siehe tracker).
fn parent<'a>(alive: &HashMap<u32, &'a Tracked>, t: &Tracked) -> Option<&'a Tracked> {
    alive.get(&t.ppid).copied().filter(|p| p.start <= t.start && p.key() != t.key())
}

/// Anbietbare Waisen, je mit ihrer Wurzel (oberster anbietbarer Vorfahr, sonst sie selbst)
/// und ihrer Tiefe unter dieser Wurzel.
fn grouped(tr: &Tracker) -> Vec<(&Tracked, Key, usize)> {
    let orphans = tr.orphans();
    let alive: HashMap<u32, &Tracked> = orphans.iter().map(|t| (t.pid, *t)).collect();
    let mut out = Vec::new();
    for t in orphans.iter().copied().filter(|t| offerable(t)) {
        let (mut root, mut depth, mut steps) = (t.key(), 0usize, 0usize);
        let mut cur = t;
        let mut seen = HashSet::new();
        let mut under_deck = false;
        while let Some(p) = parent(&alive, cur) {
            if is_deck_exe(&p.exe) {
                under_deck = true;
                break;
            }
            if !seen.insert(p.key()) || steps > 64 {
                break;
            }
            steps += 1;
            if offerable(p) {
                root = p.key();
                depth = steps;
            }
            cur = p;
        }
        // Schon früher gemerkte Prozesse unter dem Deck (vor dieser Regel): nie anbieten.
        if !under_deck {
            out.push((t, root, depth));
        }
    }
    out
}

/// Was „Beenden“ für die Zeile `root` trifft, von oben nach unten. `None` = keine
/// solche Zeile (mehr): Prozess weg, Fenster wieder offen, geschützt oder nicht oben.
pub fn kill_order(tr: &Tracker, root: Key) -> Option<Vec<Key>> {
    let mut members: Vec<(usize, Key)> =
        grouped(tr).into_iter().filter(|(_, r, _)| *r == root).map(|(t, _, depth)| (depth, t.key())).collect();
    if !members.iter().any(|(_, k)| *k == root) {
        return None;
    }
    members.sort();
    Some(members.into_iter().map(|(_, k)| k).collect())
}

/// Die Wurzeln aller Zeilen (für „Alle beenden“).
pub fn roots(tr: &Tracker) -> Vec<Key> {
    let mut r: Vec<Key> = grouped(tr).into_iter().filter(|(t, root, _)| t.key() == *root).map(|(t, ..)| t.key()).collect();
    r.sort();
    r
}

/// Laufende oder gescheiterte Beenden-Aufträge je Zeile.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KillState {
    /// Zeilen, die gerade beendet werden.
    pub busy: HashSet<Key>,
    /// Letzter Fehler je Zeile (bleibt, bis die Zeile verschwindet oder neu beendet wird).
    pub errors: HashMap<Key, String>,
}

/// Die Zeilen für die Oberfläche: zuletzt geschlossenes Fenster zuerst, dann nach
/// kleinstem Port (ohne Port zuletzt), dann PID. `ports` = lauschende Ports je PID.
pub fn orphan_rows(tr: &Tracker, ports: &HashMap<u32, Vec<u16>>, kills: &KillState, now: f64, lang: Lang) -> Vec<OrphanView> {
    let groups = grouped(tr);
    let mut rows: Vec<(f64, OrphanView)> = Vec::new();
    for (t, root, _) in &groups {
        if t.key() != *root {
            continue;
        }
        let mut p: Vec<u16> =
            groups.iter().filter(|(_, r, _)| r == root).flat_map(|(m, ..)| ports.get(&m.pid).cloned().unwrap_or_default()).collect();
        p.sort_unstable();
        p.dedup();
        let shell = tr.shell(t.shell_key());
        let closed_at = shell.and_then(|s| s.closed_at).unwrap_or(now);
        let slot = shell.map_or("?", |s| s.slot.as_str());
        rows.push((
            closed_at,
            OrphanView {
                id: row_id(t.key()),
                pid: t.pid,
                port_text: port_text(&p, lang),
                ports: p,
                program: exe_stem(&t.exe),
                command: display_command(t.cmdline.as_deref(), &t.exe),
                command_full: t.cmdline.clone().unwrap_or_else(|| t.exe.clone()),
                origin: format!("{slot} · {}", closed_since(now - closed_at, lang)),
                busy: kills.busy.contains(root),
                error: kills.errors.get(root).cloned(),
            },
        ));
    }
    rows.sort_by(|(a, x), (b, y)| {
        b.total_cmp(a)
            .then_with(|| x.ports.first().unwrap_or(&u16::MAX).cmp(y.ports.first().unwrap_or(&u16::MAX)))
            .then_with(|| x.pid.cmp(&y.pid))
    });
    rows.into_iter().map(|(_, r)| r).collect()
}

/// `:3000`, `:3000 :3001`, ab drei `:3000 +2`; ohne Port „kein Port“.
pub fn port_text(ports: &[u16], lang: Lang) -> String {
    match ports {
        [] => lang.l("kein Port", "no port").into(),
        [a] => format!(":{a}"),
        [a, b] => format!(":{a} :{b}"),
        [a, rest @ ..] => format!(":{a} +{}", rest.len()),
    }
}

/// „zu seit 14 min“ / „closed 14 min ago“.
pub fn closed_since(secs: f64, lang: Lang) -> String {
    let s = secs.max(0.0);
    let span = if s < 60.0 {
        return lang.l("gerade zu", "just closed").into();
    } else if s < 3600.0 {
        format!("{} min", (s / 60.0).floor())
    } else if s < 86_400.0 {
        format!("{} h", (s / 3600.0).floor())
    } else {
        let d = (s / 86_400.0).floor();
        if lang.is_english() { format!("{d} d") } else { format!("{d} Tg.") }
    };
    if lang.is_english() { format!("closed {span} ago") } else { format!("zu seit {span}") }
}
