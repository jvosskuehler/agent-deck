//! Probelauf gegen die echten Transcripts unter `~/.claude/projects`, NUR LESEND.
//! Läuft nicht mit der Suite (`#[ignore]`); von Hand:
//!
//! ```text
//! cargo test -p deck-core --test worktree_use_real -- --ignored --nocapture
//! ```
//!
//! Druckt je Session (jüngste zuerst) den Bezug, den die Kachel zeigen würde, samt
//! Quelle, und die stärksten worktrees aus den Werkzeugaufrufen. So lässt sich die
//! Trefferquote am echten Arbeitsalltag beurteilen.

use std::path::Path;

use deck_core::claude::refs::{Refs, find_refs};
use deck_core::claude::summarize::{extract_turns_from_values, transcript_lines};
use deck_core::claude::summary_cache::projects_dir;
use deck_core::claude::worktree_use::{active_worktree, worktree_uses};
use deck_core::git_head::{GitFs, find_branch};
use deck_core::i18n::Lang;
use deck_core::ref_sources::{RefEvidence, pick_ticket, ticket_from_title};
use serde_json::Value;

/// Liest nur, schreibt nie.
struct ReadOnlyFs;

impl GitFs for ReadOnlyFs {
    fn is_dir(&self, p: &Path) -> bool {
        p.is_dir()
    }
    fn read_text(&self, p: &Path) -> Option<String> {
        let m = std::fs::metadata(p).ok()?;
        (m.is_file() && m.len() < 4096).then(|| std::fs::read_to_string(p).ok()).flatten()
    }
}

fn short(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n - 1).collect::<String>()) }
}

fn last_name(p: &str) -> String {
    p.rsplit(['\\', '/']).find(|s| !s.is_empty()).unwrap_or("").to_string()
}

#[test]
#[ignore = "liest die echten Transcripts; nur von Hand"]
fn print_detected_tickets_of_recent_sessions() {
    let n: usize = std::env::var("DECK_PROBE_N").ok().and_then(|v| v.parse().ok()).unwrap_or(20);
    let prefix = std::env::var("DECK_PROBE_PREFIX").unwrap_or_else(|_| "ORPH".into());
    let mut files: Vec<(std::time::SystemTime, std::path::PathBuf)> = Vec::new();
    for proj in std::fs::read_dir(projects_dir()).expect("projects").flatten() {
        for f in std::fs::read_dir(proj.path()).into_iter().flatten().flatten() {
            let p = f.path();
            if p.extension().is_some_and(|x| x == "jsonl")
                && let Ok(m) = f.metadata().and_then(|m| m.modified())
            {
                files.push((m, p));
            }
        }
    }
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    // DECK_PROBE_ALL=0: nur Sessions, in denen ein worktree oder ein Ticket vorkommt.
    let all = std::env::var("DECK_PROBE_ALL").map(|v| v != "0").unwrap_or(true);
    println!("| Session | Titel | cwd zuletzt | Ticket | Quelle | Ticket im Titel | worktrees (W/R) |");
    println!("|---|---|---|---|---|---|---|");
    for (_, path) in files.into_iter().take(n) {
        let bytes = std::fs::read(&path).unwrap_or_default();
        let text = String::from_utf8_lossy(&bytes);
        let records: Vec<Value> = transcript_lines(&text).into_iter().filter_map(|l| serde_json::from_str(l.trim()).ok()).collect();
        let title = records.iter().rev().find_map(|r| r.get("aiTitle").or_else(|| r.get("customTitle")).and_then(Value::as_str)).unwrap_or("");
        let cwd = records.iter().rev().find_map(|r| r.get("cwd").and_then(Value::as_str)).unwrap_or("").to_string();
        let mut chat: Refs = find_refs(&extract_turns_from_values(&records), Some(&prefix));
        if let Some(w) = active_worktree(&records) {
            chat.worktree = w.dir;
            chat.worktree_writes = w.writes > 0;
        }
        let branches: Vec<String> = find_branch(Path::new(&cwd), &ReadOnlyFs).into_iter().collect();
        let wt_branch = (!chat.worktree.is_empty()).then(|| find_branch(Path::new(&chat.worktree), &ReadOnlyFs)).flatten();
        let dirs = vec![cwd.clone()];
        let ev = RefEvidence {
            branches: &branches,
            dirs: &dirs,
            title: Some(title).filter(|t| !t.is_empty()),
            chat: Some(&chat),
            worktree_branch: wt_branch.as_deref(),
            prefix: &prefix,
            ..RefEvidence::default()
        };
        let (ticket, source) = match pick_ticket(&ev, Lang::German) {
            Some(t) => (t.text, format!("{}{}", t.origin, if t.detected { " (matt)" } else { "" })),
            None => ("-".into(), "-".into()),
        };
        let uses: Vec<String> = worktree_uses(&records)
            .iter()
            .take(3)
            .map(|w| format!("{} {}/{} ({:.1})", last_name(&w.dir), w.writes, w.reads, w.score))
            .collect();
        let in_title = ticket_from_title(title, &prefix).unwrap_or_else(|| "-".into());
        if !all && uses.is_empty() && ticket == "-" {
            continue;
        }
        let sid = path.file_stem().map(|s| s.to_string_lossy().chars().take(8).collect::<String>()).unwrap_or_default();
        println!("| {sid} | {} | {} | {ticket} | {source} | {in_title} | {} |", short(title, 40), last_name(&cwd), uses.join(", "));
    }
}
