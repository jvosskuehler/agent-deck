//! Transcript finden, Zusammenfassung und erkannte Bezüge je Session cachen, und
//! entscheiden, wann `claude` neu gefragt wird.
//!
//! Ein `claude`-Aufruf ist schwer (~8 s Startup). Darum wird je Session eine kleine
//! JSON-Datei in `state\summaries\` gehalten und nur bei echtem Zuwachs des Transcripts
//! neu erzeugt. Ticket-Scan und Zusammenfassung teilen sich diese Datei; jeder ersetzt
//! nur seine eigenen Felder (`merge_cache`), damit der eine den anderen nicht
//! überbügelt. Den `claude`-Prozess startet die App: `plan_summary` sagt ihr, ob und mit
//! welchem Prompt, `finish_summary` nimmt die Antwort entgegen.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::claude::refs::{Refs, find_refs};
use crate::claude::summarize::{build_digest, clean_summary, enc_cwd, extract_turns, instruction, transcript_lines};
use crate::paths::{home_dir, load_json, save_json};
use crate::field::number;

/// `~/.claude/projects`: die Transcripts von Claude Code.
pub fn projects_dir() -> PathBuf {
    home_dir().join(".claude").join("projects")
}

/// `state\summaries`: der Cache.
pub fn summary_dir(state_dir: &Path) -> PathBuf {
    state_dir.join("summaries")
}

fn mtime(p: &Path) -> Option<std::time::SystemTime> {
    fs::metadata(p).and_then(|m| m.modified()).ok()
}

fn size_of(p: &Path) -> u64 {
    fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

/// Transcript einer Session: erst der Direktweg über das kodierte cwd, sonst über die
/// (eindeutige) Session-ID in allen Projektordnern; das deckt auch ein worktree-cwd ab.
/// Mehrere Treffer: der zuletzt geänderte.
pub fn transcript_path(projects: &Path, session_id: &str, cwd: Option<&str>) -> Option<PathBuf> {
    if session_id.is_empty() {
        return None;
    }
    let file = format!("{session_id}.jsonl");
    if let Some(cwd) = cwd.filter(|c| !c.is_empty()) {
        let direct = projects.join(enc_cwd(cwd)).join(&file);
        if direct.is_file() {
            return Some(direct);
        }
    }
    fs::read_dir(projects)
        .ok()?
        .flatten()
        // Wie Pythons glob: `*` trifft keine Namen mit führendem Punkt.
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| e.path().join(&file))
        .filter(|p| p.is_file())
        .max_by_key(|p| mtime(p))
}

fn read_lines_lossy(path: &Path) -> Vec<String> {
    let Ok(bytes) = fs::read(path) else { return Vec::new() };
    let text = String::from_utf8_lossy(&bytes);
    transcript_lines(&text).into_iter().map(String::from).collect()
}

fn cache_path(dir: &Path, session_id: &str) -> PathBuf {
    dir.join(format!("{session_id}.json"))
}

/// Cache-Datei der Session (fehlt/kaputt → `None`).
pub fn read_cache(dir: &Path, session_id: &str) -> Option<Map<String, Value>> {
    if session_id.is_empty() {
        return None;
    }
    match load_json(&cache_path(dir, session_id)) {
        Some(Value::Object(m)) => Some(m),
        _ => None,
    }
}

fn text_field(cache: &Map<String, Value>, key: &str) -> String {
    cache.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

/// Nur die schon gecachte Zusammenfassung (schnell, kein Prozess).
pub fn cached_summary(dir: &Path, session_id: &str) -> Option<String> {
    let c = read_cache(dir, session_id)?;
    Some(text_field(&c, "summary")).filter(|s| !s.is_empty())
}

/// Nur die schon erkannten Bezüge, ohne das Transcript zu lesen.
pub fn cached_refs(dir: &Path, session_id: &str) -> Refs {
    let c = read_cache(dir, session_id).unwrap_or_default();
    Refs { ticket: text_field(&c, "ticket"), pr: text_field(&c, "pr") }
}

/// Cache frisch lesen und nur die übergebenen Felder ersetzen.
pub fn merge_cache(dir: &Path, session_id: &str, fields: Vec<(&str, Value)>) -> std::io::Result<Map<String, Value>> {
    let mut data = read_cache(dir, session_id).unwrap_or_default();
    for (k, v) in fields {
        data.insert(k.into(), v);
    }
    save_json(&cache_path(dir, session_id), &Value::Object(data.clone()))?;
    Ok(data)
}

/// Ticket-ID und PR-Nummer der Session bestimmen und cachen. Rein lokal (Regex), aber
/// das Transcript kann ein paar MB haben: nicht im UI-Thread aufrufen. Neu gescannt wird
/// nur, wenn sich die Größe geändert hat; schon gefundene Werte bleiben stehen, wenn ein
/// neuer Scan nichts findet (ein gekürztes Transcript soll sie nicht löschen).
pub fn ensure_refs(projects: &Path, dir: &Path, session_id: &str, cwd: Option<&str>, project: Option<&str>) -> Refs {
    if session_id.is_empty() {
        return Refs::default();
    }
    let cache = read_cache(dir, session_id).unwrap_or_default();
    let old = Refs { ticket: text_field(&cache, "ticket"), pr: text_field(&cache, "pr") };
    let Some(path) = transcript_path(projects, session_id, cwd) else { return old };
    let size = size_of(&path);
    if cache.get("refs_size").and_then(Value::as_u64) == Some(size) {
        return old;
    }
    let found = find_refs(&extract_turns(&read_lines_lossy(&path)), project);
    let refs = Refs {
        ticket: if found.ticket.is_empty() { old.ticket } else { found.ticket },
        pr: if found.pr.is_empty() { old.pr } else { found.pr },
    };
    let _ = merge_cache(
        dir,
        session_id,
        vec![("refs_size", size.into()), ("ticket", refs.ticket.clone().into()), ("pr", refs.pr.clone().into())],
    );
    refs
}

/// Stellschrauben der Neu-Erzeugung (Python-Vorgaben in `Default`).
#[derive(Debug, Clone, Copy)]
pub struct SummaryGates {
    /// So viele Bytes muss das Transcript seit dem letzten Lauf gewachsen sein.
    pub min_growth: u64,
    /// So viele Sekunden muss der letzte Lauf her sein (deckelt arbeitende Chats).
    pub cooldown: f64,
    /// Zeichenbudget des Digests.
    pub max_chars: usize,
}

impl Default for SummaryGates {
    fn default() -> Self {
        SummaryGates { min_growth: 8000, cooldown: 45.0, max_chars: 2400 }
    }
}

/// Was die App für eine Session tun soll.
#[derive(Debug, Clone, PartialEq)]
pub enum SummaryPlan {
    /// Nichts aufrufen; das ist der Stand (eventuell keiner).
    Cached(Option<String>),
    /// `claude` mit diesem Prompt fragen und die Antwort an `finish_summary` geben.
    Generate { prompt: String, size: u64, cached: Option<String> },
}

/// Entscheiden, ob neu erzeugt wird: nur ohne Cache, bei anderer Sprache oder wenn das
/// Transcript um `min_growth` gewachsen UND der letzte Lauf `cooldown` her ist. Die
/// Gates sind bewusst grob: das Thema ändert sich kaum von Zug zu Zug.
pub fn plan_summary(
    projects: &Path,
    dir: &Path,
    session_id: &str,
    cwd: Option<&str>,
    lang: &str,
    now: f64,
    gates: SummaryGates,
) -> SummaryPlan {
    if session_id.is_empty() {
        return SummaryPlan::Cached(None);
    }
    let cache = read_cache(dir, session_id).unwrap_or_default();
    let cached = Some(text_field(&cache, "summary")).filter(|s| !s.is_empty());
    let Some(path) = transcript_path(projects, session_id, cwd) else { return SummaryPlan::Cached(cached) };
    let size = size_of(&path);
    // Alt-Caches ohne "lang" gelten als deutsch.
    let cache_lang = cache.get("lang").and_then(Value::as_str).unwrap_or("german");
    if cached.is_some() && cache_lang == lang {
        let grown = size as f64 - number(cache.get("size")).unwrap_or(0.0).trunc();
        let age = now - number(cache.get("ts")).unwrap_or(0.0);
        if grown < gates.min_growth as f64 || age < gates.cooldown {
            return SummaryPlan::Cached(cached);
        }
    }
    let digest = build_digest(&extract_turns(&read_lines_lossy(&path)), gates.max_chars, 600);
    if digest.is_empty() {
        return SummaryPlan::Cached(cached);
    }
    SummaryPlan::Generate { prompt: format!("{}{digest}", instruction(lang)), size, cached }
}

/// Antwort von `claude` übernehmen (`None` = Aufruf gescheitert). Bei leerem Ergebnis
/// bleibt der alte Cache-Wert. Ersetzt nur die Summary-Felder.
pub fn finish_summary(
    dir: &Path,
    session_id: &str,
    output: Option<&str>,
    size: u64,
    lang: &str,
    now: f64,
    cached: Option<String>,
) -> Option<String> {
    let summary = clean_summary(output.unwrap_or(""), 200);
    if summary.is_empty() {
        return cached;
    }
    let _ = merge_cache(
        dir,
        session_id,
        vec![("summary", summary.clone().into()), ("size", size.into()), ("ts", now.into()), ("lang", lang.into())],
    );
    Some(summary)
}

/// Cache-Dateien längst geschlossener Sessions entfernen (best effort, beim Start).
pub fn prune(dir: &Path, max_age_days: f64, now: std::time::SystemTime) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let max_age = std::time::Duration::from_secs_f64(max_age_days * 86_400.0);
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "json")
            && let Some(m) = mtime(&p)
            && now.duration_since(m).is_ok_and(|age| age > max_age)
        {
            let _ = fs::remove_file(&p);
        }
    }
}
