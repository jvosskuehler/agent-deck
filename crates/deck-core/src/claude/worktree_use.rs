//! Der worktree, in dem ein Agent WIRKLICH arbeitet, aus seinen Werkzeugaufrufen.
//!
//! Die Agenten des Besitzers starten in `Repos` (kein git-Repo) und arbeiten an Tickets
//! in `Repos\wt\<Nummer>-<Kürzel>`, meist mit `git -C wt/4116-ui …`, `cd` oder Edit mit
//! absolutem Pfad. Branch und Ordner des cwd sagen dann nichts. Was der Agent anfasst,
//! steht aber in den EINGABEN seiner Werkzeugaufrufe im Transcript (`tool_use`): Pfade
//! von Edit/Write/Read, Befehle von Bash/PowerShell, `path` von Glob/Grep. Die Ergebnisse
//! (`tool_result`) zählen nicht: eine Liste aller worktrees sagt nichts über die Arbeit.
//!
//! Gewichtung: Schreiben (Edit, Write, NotebookEdit, `git commit`/`push` im worktree)
//! zählt [`WRITE_WEIGHT`]-fach, Lesen einfach; jeder Treffer verliert mit dem Alter an
//! Gewicht (Halbwertszeit [`HALF_LIFE_CALLS`] Werkzeugaufrufe), damit der worktree von
//! jetzt den von vor zwei Stunden schlägt. Ein Befehl, der viele worktrees nennt, ist eine
//! Übersicht (Statusrunde, Aufräumen) und zählt nicht; Entfernen auch nicht.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

/// So viel mehr zählt ein schreibender Aufruf als ein lesender.
pub const WRITE_WEIGHT: f64 = 8.0;
/// Nach so vielen Werkzeugaufrufen zählt ein Treffer noch halb.
pub const HALF_LIFE_CALLS: f64 = 40.0;
/// Ohne Schreiben braucht ein worktree so viele lesende Aufrufe, sonst war es ein Blick.
pub const MIN_READS: u32 = 3;
/// Nennt ein Befehl mehr worktrees als das, ist er eine Übersicht.
const OVERVIEW_DIRS: usize = 3;

/// Ein worktree, wie das Transcript ihn zeigt.
#[derive(Debug, Clone, PartialEq)]
pub struct WorktreeUse {
    /// Bester bekannter Pfad (absolut, sobald ein Aufruf ihn absolut nannte oder er sich
    /// aus dem cwd auflösen ließ), z. B. `C:\Users\x\Repos\wt\4116-ui`.
    pub dir: String,
    pub writes: u32,
    pub reads: u32,
    /// Gewichtete, mit dem Alter abklingende Summe.
    pub score: f64,
    /// Index des jüngsten Aufrufs, der ihn traf (unter allen Werkzeugaufrufen).
    pub last_call: usize,
}

/// `wt/<Name>` bzw. `wt\<Name>` oder (alte Form, so liegen die älteren worktrees des
/// Besitzers) `wt-<Name>`. Ränder prüft `wt_paths`.
static WT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)wt(?:[\\/]+|-)([A-Za-z0-9][A-Za-z0-9_-]*)").expect("WT_RE"));
/// Schreibende git-/gh-Befehle.
static WRITE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:git\b.*\b(?:commit|push|merge|rebase|cherry-pick|worktree\s+add)\b|gh\s+pr\s+create\b)")
        .expect("WRITE_RE")
});
/// Wegräumen ist keine Arbeit am Ticket.
static DROP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:\bworktree\s+(?:remove|prune)\b|\brm\s+-\w*r|\bremove-item\b|\brmdir\b)").expect("DROP_RE")
});
static CD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(?:cd|pushd|set-location|push-location|sl)\s+(.+)$").expect("CD_RE"));

/// Zeichen, an denen ein Pfad im Befehl beginnt bzw. endet.
fn token_delim(c: char) -> bool {
    c.is_whitespace() || "\"'`=(),;|&<>".contains(c)
}

fn is_sep(c: char) -> bool {
    c == '/' || c == '\\'
}

fn is_abs(p: &str) -> bool {
    let b = p.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

/// `/c/Users/x` (Git Bash) → `C:/Users/x`.
fn msys_to_win(p: &str) -> String {
    let b = p.as_bytes();
    if b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && (b.len() == 2 || b[2] == b'/') {
        return format!("{}:{}", (b[1] as char).to_ascii_uppercase(), &p[2..]);
    }
    p.to_string()
}

/// `.` und `..` auflösen, Trenner vereinheitlichen (`\`).
fn normalize(p: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for part in p.split(is_sep).filter(|s| !s.is_empty() && *s != ".") {
        if part == ".." {
            if out.len() > 1 {
                out.pop();
            }
        } else {
            out.push(part);
        }
    }
    out.join("\\")
}

/// Heimatordner aus einem cwd unter `C:\Users\<Name>` (für `~` im Befehl).
fn home_of(cwd: &str) -> Option<String> {
    let parts: Vec<&str> = cwd.split(is_sep).filter(|s| !s.is_empty()).collect();
    (parts.len() >= 3 && parts[1].eq_ignore_ascii_case("users")).then(|| parts[..3].join("\\"))
}

/// Pfad absolut machen, wenn es geht (relativ zum cwd, `~`, Git-Bash-Form).
fn resolve(path: &str, cwd: Option<&str>) -> Option<String> {
    let p = msys_to_win(path);
    let cwd = cwd.filter(|c| is_abs(c));
    if is_abs(&p) {
        return Some(normalize(&p));
    }
    if let Some(rest) = p.strip_prefix('~') {
        return cwd.and_then(home_of).map(|h| normalize(&format!("{h}\\{rest}")));
    }
    if p.starts_with('$') || p.starts_with('%') || p.starts_with('/') {
        return None;
    }
    cwd.map(|c| normalize(&format!("{c}\\{p}")))
}

/// Ein worktree-Treffer: Schlüssel (`wt/4116-ui`, klein) und Pfad, wie er sich auflösen ließ.
#[derive(Debug, Clone, PartialEq)]
struct Hit {
    key: String,
    dir: String,
    resolved: bool,
}

/// Alle worktrees in einem Text (Pfad oder Befehl). `wt` muss ein eigener Pfadteil sein
/// (`newt/x` und `--wt-x` nicht), und hinter dem Namen darf keine Dateiendung oder ein
/// `*` folgen (`wt/README.md`, `wt/*`).
fn wt_paths(text: &str, cwd: Option<&str>) -> Vec<Hit> {
    let mut out: Vec<Hit> = Vec::new();
    for c in WT_RE.captures_iter(text) {
        let (m, name) = (c.get(0).expect("Treffer"), c.get(1).expect("Name"));
        let before = text[..m.start()].chars().next_back();
        let after = text[m.end()..].chars().next();
        if !before.is_none_or(|b| is_sep(b) || token_delim(b)) || !after.is_none_or(|a| is_sep(a) || token_delim(a)) {
            continue;
        }
        let start = text[..m.start()].rfind(token_delim).map_or(0, |i| i + 1);
        let raw = &text[start..m.end()];
        let legacy = !text[m.start() + 2..].starts_with(is_sep);
        // Die alte Form nur klein: groß ist es fast immer Windows Terminal („WT-Fenster“).
        if legacy && !m.as_str().starts_with("wt") {
            continue;
        }
        let key = format!("wt{}{}", if legacy { "-" } else { "/" }, name.as_str().to_lowercase());
        let (dir, resolved) = match resolve(raw, cwd) {
            Some(d) => (d, true),
            None => (raw.to_string(), false),
        };
        if !out.iter().any(|h| h.key == key) {
            out.push(Hit { key, dir, resolved });
        }
    }
    out
}

/// Treffer eines Bash-/PowerShell-Befehls. Zerlegt an `&&`, `||`, `;`, `|` und Zeilen;
/// ein `cd` gilt für die folgenden Teile, ein Teil ohne Pfad zählt für den worktree, in
/// dem er läuft.
fn command_hits(cmd: &str, cwd: Option<&str>) -> Vec<(Hit, bool)> {
    let mut dir: Option<String> = cwd.map(String::from);
    let mut out: Vec<(Hit, bool)> = Vec::new();
    for seg in cmd.split(['\n', ';', '|', '&']).map(str::trim).filter(|s| !s.is_empty()) {
        if DROP_RE.is_match(seg) {
            continue;
        }
        let write = WRITE_RE.is_match(seg);
        // Pfade dieses Teils gelten relativ zum Ordner VOR einem `cd`.
        let mut hits = wt_paths(seg, dir.as_deref());
        if let Some(c) = CD_RE.captures(seg) {
            let target = c[1].trim().trim_matches(['"', '\'']);
            if let Some(d) = resolve(target, dir.as_deref()) {
                dir = Some(d);
            }
        }
        if hits.is_empty() {
            hits = dir.as_deref().map(|d| wt_paths(d, None)).unwrap_or_default().into_iter().rev().take(1).collect();
        }
        out.extend(hits.into_iter().map(|h| (h, write)));
    }
    out
}

/// Treffer eines Werkzeugaufrufs als (Treffer, schreibt?).
fn call_hits(name: &str, input: &Value, cwd: Option<&str>) -> Vec<(Hit, bool)> {
    let s = |k: &str| input.get(k).and_then(Value::as_str);
    let paths = |keys: &[&str], write: bool| -> Vec<(Hit, bool)> {
        keys.iter().filter_map(|k| s(k)).flat_map(|p| wt_paths(p, cwd)).map(|h| (h, write)).collect()
    };
    match name {
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => paths(&["file_path", "notebook_path"], true),
        "Read" | "LS" => paths(&["file_path", "path"], false),
        "Glob" | "Grep" => paths(&["path", "pattern"], false),
        "Bash" | "PowerShell" => s("command").map(|c| command_hits(c, cwd)).unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Alle Werkzeugaufrufe eines Transcripts als (Name, Eingabe, cwd), in Reihenfolge.
fn tool_calls(records: &[Value]) -> Vec<(&str, &Value, Option<&str>)> {
    let mut out = Vec::new();
    for rec in records.iter().filter(|r| r.get("type").and_then(Value::as_str) == Some("assistant")) {
        let cwd = rec.get("cwd").and_then(Value::as_str);
        let Some(items) = rec.get("message").and_then(|m| m.get("content")).and_then(Value::as_array) else { continue };
        for it in items.iter().filter(|i| i.get("type").and_then(Value::as_str) == Some("tool_use")) {
            if let (Some(name), Some(input)) = (it.get("name").and_then(Value::as_str), it.get("input")) {
                out.push((name, input, cwd));
            }
        }
    }
    out
}

/// Alle worktrees, die der Agent angefasst hat, stärkster zuerst (ohne Mindestmaß).
pub fn worktree_uses(records: &[Value]) -> Vec<WorktreeUse> {
    let calls = tool_calls(records);
    let total = calls.len();
    let mut by_key: HashMap<String, WorktreeUse> = HashMap::new();
    for (i, (name, input, cwd)) in calls.into_iter().enumerate() {
        let mut hits: Vec<(Hit, bool)> = Vec::new();
        for (h, w) in call_hits(name, input, cwd) {
            match hits.iter_mut().find(|(o, _)| o.key == h.key) {
                Some((o, ow)) => {
                    *ow |= w;
                    if h.resolved && !o.resolved {
                        *o = h;
                    }
                }
                None => hits.push((h, w)),
            }
        }
        if hits.len() > OVERVIEW_DIRS {
            continue;
        }
        let decay = 0.5f64.powf((total - 1 - i) as f64 / HALF_LIFE_CALLS);
        for (h, write) in hits {
            let e = by_key.entry(h.key.clone()).or_insert_with(|| WorktreeUse {
                dir: h.dir.clone(),
                writes: 0,
                reads: 0,
                score: 0.0,
                last_call: i,
            });
            if h.resolved || !is_abs(&e.dir) {
                e.dir = h.dir;
            }
            if write {
                e.writes += 1;
            } else {
                e.reads += 1;
            }
            e.score += decay * if write { WRITE_WEIGHT } else { 1.0 };
            e.last_call = i;
        }
    }
    let mut all: Vec<WorktreeUse> = by_key.into_values().collect();
    all.sort_by(|a, b| b.score.total_cmp(&a.score).then(b.last_call.cmp(&a.last_call)));
    all
}

/// Der worktree, in dem der Agent arbeitet: der stärkste, der geschrieben wurde oder
/// mindestens [`MIN_READS`]-mal gelesen.
pub fn active_worktree(records: &[Value]) -> Option<WorktreeUse> {
    worktree_uses(records).into_iter().find(|w| w.writes > 0 || w.reads >= MIN_READS)
}
