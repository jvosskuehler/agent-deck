//! Ticket und PR einer Kachel aus mehreren Quellen, die stärkste gewinnt.
//!
//! Früher kam beides fast nur aus dem Chat (Regex über das Transcript) und lag oft
//! daneben: der Chat nennt viele Tickets, der Agent arbeitet an einem. Was sicher ist,
//! steht woanders. Darum feste Reihenfolge, stärkste zuerst:
//!
//! Ticket: 1. von Hand zugewiesen (Kontextmenü) bzw. danach vom Agenten gemeldet
//! (`.ticket`), 2. git-Branch im Ordner des Agenten (`feature/ORPH-4116`, `4116-ui`),
//! 3. worktree-Ordner (`Repos/wt/4116-ui`), 4. Sessiontitel, 5. Chat.
//! PR: 1. was Claude Code selbst meldet (`pr` in `live.json`, maßgeblich), 2. Titel,
//! 3. Chat.
//!
//! Titel und Chat sind geraten: die Kachel zeigt sie gedimmt (`RefView::detected`).
//! Branch und Ordner lesen sich aus Dateien; das tut die App (`git_head.rs` mit
//! Cache), hier kommt nur das Ergebnis an.

use std::sync::LazyLock;

use regex::Regex;

use crate::binding::jira_key;
use crate::claude::refs::{KEY_STOP, PR_CTX_RE, PR_URL_RE, Refs, find_ticket, find_with, no_digit};
use crate::i18n::Lang;
use crate::text::take_chars;
use crate::view::{RefSource, RefView};

/// So viele Zeichen hat das Ticket auf der Karte höchstens („ORPH-4116“ = 9).
pub const TICKET_MAX_CHARS: usize = 14;

/// Was über eine Kachel bekannt ist, in der Reihenfolge der Quellen.
#[derive(Debug, Clone, Copy, Default)]
pub struct RefEvidence<'a> {
    /// Zugewiesenes (`tickets.json`) oder danach gemeldetes Ticket (`<slot>.ticket`).
    pub assigned: Option<&'a str>,
    /// Branches der Ordner des Agenten, wichtigster zuerst.
    pub branches: &'a [String],
    /// Ordner des Agenten (`ref_dirs`), wichtigster zuerst.
    pub dirs: &'a [String],
    /// PR-Nummer aus der Statuszeile (`live.json` `pr.number`).
    pub statusline_pr: Option<u64>,
    /// Sessiontitel (`live.json` `session_name`).
    pub title: Option<&'a str>,
    /// Im Chat Erratenes.
    pub chat: Option<&'a Refs>,
    /// Jira-Projekt (`deck_settings.json` `jira_prefix`): macht aus `4116` `ORPH-4116`.
    pub prefix: &'a str,
}

/// Die Ordner, in denen ein Agent arbeitet, wichtigster zuerst und ohne Doppelte: der
/// gemeldete worktree (`<slot>.worktree`), der aktuelle Ordner aus der Statuszeile
/// (`live.json` `cwd`), das cwd der Hooks (`<slot>.json`).
pub fn ref_dirs(worktree: Option<&str>, live_cwd: Option<&str>, state_cwd: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for d in [worktree, live_cwd, state_cwd].into_iter().flatten().map(str::trim) {
        if !d.is_empty() && !out.iter().any(|o| o == d) {
            out.push(d.to_string());
        }
    }
    out
}

static NAME_KEY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([A-Za-z][A-Za-z0-9]{1,9})-(\d{1,6})").expect("NAME_KEY"));
static LEADING_NUM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d{2,6})(?:[-_][A-Za-z].*)?$").expect("LEADING_NUM"));

/// Ein Jira-Key in einem Branch- oder Ordnernamen. Das konfigurierte Projekt gilt in
/// jeder Schreibweise (`ticket/orph-4116`, so baut das Deck selbst Branches); fremde
/// Projekte nur groß geschrieben (`feature/CS-231`) oder mit `lenient`, damit
/// `hotfix/login-2` oder `node-18` kein Ticket wird.
fn key_in_name(name: &str, prefix: &str, lenient: bool) -> Option<String> {
    let mut other = None;
    for c in find_with(&NAME_KEY, name, |p| !p.is_some_and(|c| c.is_ascii_alphanumeric()), no_digit) {
        let (raw, num) = (&c[1], &c[2]);
        let pre = raw.to_uppercase();
        if !prefix.is_empty() && pre == prefix {
            return Some(format!("{pre}-{num}"));
        }
        let upper = raw.chars().all(|ch| !ch.is_ascii_lowercase());
        if other.is_none() && (lenient || upper) && !KEY_STOP.contains(&pre.as_str()) && num.len() >= 2 {
            other = Some(format!("{pre}-{num}"));
        }
    }
    other
}

/// Eine führende Nummer (`4116-ui`, `4116`) mit dem Projekt davor. Ein Datum
/// (`2026-11-09`) ist keine: nach dem Bindestrich muss ein Buchstabe kommen.
fn number_in_name(name: &str, prefix: &str) -> Option<String> {
    if prefix.is_empty() {
        return None;
    }
    let c = LEADING_NUM.captures(name)?;
    Some(format!("{prefix}-{}", &c[1]))
}

/// Ticket aus einem Branch: `feature/ORPH-4116-x`, `ticket/orph-4116`, `4116-taskbar`.
pub fn ticket_from_branch(branch: &str, prefix: &str) -> Option<String> {
    let prefix = prefix.trim().to_uppercase();
    let lenient = branch.starts_with(crate::config::TICKET_BRANCH_PREFIX);
    key_in_name(branch, &prefix, lenient).or_else(|| branch.split('/').find_map(|seg| number_in_name(seg, &prefix)))
}

/// Ticket aus dem Ordner: der Teil unter `wt` (`Repos/wt/4116-ui`), ein Ordner
/// `wt-4116-ui` oder ein worktree von Claude Code (`.claude/worktrees/orph-4116`). Der
/// tiefste passende Teil gewinnt.
pub fn ticket_from_folder(dir: &str, prefix: &str) -> Option<String> {
    let prefix = prefix.trim().to_uppercase();
    let parts: Vec<&str> = dir.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    let mut names: Vec<&str> = Vec::new();
    for (i, p) in parts.iter().enumerate() {
        let low = p.to_lowercase();
        if (low == "wt" || low == "worktrees") && i + 1 < parts.len() {
            names.push(parts[i + 1]);
        } else if let Some(rest) = low.strip_prefix("wt-") {
            names.push(&p[p.len() - rest.len()..]);
        }
    }
    names.iter().rev().find_map(|n| key_in_name(n, &prefix, false).or_else(|| number_in_name(n, &prefix)))
}

/// Ticket aus dem Sessiontitel, mit denselben Regeln wie eine getippte Nachricht.
pub fn ticket_from_title(title: &str, prefix: &str) -> Option<String> {
    let t = find_ticket(&[("user".into(), title.to_string())], Some(prefix), 2);
    (!t.is_empty()).then_some(t)
}

/// PR aus dem Sessiontitel: nur mit „PR“ davor oder als PR-Link, eine bloße `#12` kann
/// ebenso ein Issue sein.
pub fn pr_from_title(title: &str) -> Option<String> {
    let url = find_with(&PR_URL_RE, title, |_| true, no_digit).into_iter().next();
    let ctx = || find_with(&PR_CTX_RE, title, |_| true, no_digit).into_iter().next();
    url.or_else(ctx).map(|c| c[1].to_string())
}

/// Herkunft als Wort für den Tooltip.
pub fn origin_word(source: RefSource, lang: Lang) -> &'static str {
    match source {
        RefSource::Assigned => lang.l("zugewiesen", "assigned"),
        RefSource::Branch => "Branch",
        RefSource::Folder => lang.l("Ordner", "folder"),
        RefSource::Statusline => "Claude Code",
        RefSource::Title => lang.l("Titel", "title"),
        RefSource::Chat => "Chat",
    }
}

fn ref_view(text: String, source: RefSource, lang: Lang) -> RefView {
    let text = if text.chars().count() > TICKET_MAX_CHARS { format!("{}…", take_chars(&text, TICKET_MAX_CHARS - 1)) } else { text };
    RefView { text, source, detected: matches!(source, RefSource::Title | RefSource::Chat), origin: origin_word(source, lang).into() }
}

/// Das Ticket der Kachel, aus der stärksten Quelle, die eins hat.
pub fn pick_ticket(ev: &RefEvidence, lang: Lang) -> Option<RefView> {
    let prefix = ev.prefix.trim();
    let nonempty = |s: Option<&str>| s.map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    let found = nonempty(ev.assigned)
        .map(|a| (jira_key(&a, Some(prefix)), RefSource::Assigned))
        .or_else(|| ev.branches.iter().find_map(|b| ticket_from_branch(b, prefix)).map(|t| (t, RefSource::Branch)))
        .or_else(|| ev.dirs.iter().find_map(|d| ticket_from_folder(d, prefix)).map(|t| (t, RefSource::Folder)))
        .or_else(|| ev.title.and_then(|t| ticket_from_title(t, prefix)).map(|t| (t, RefSource::Title)))
        .or_else(|| nonempty(ev.chat.map(|r| r.ticket.as_str())).map(|t| (t, RefSource::Chat)));
    found.map(|(text, source)| ref_view(text, source, lang))
}

/// Der PR der Kachel als „#347“.
pub fn pick_pr(ev: &RefEvidence, lang: Lang) -> Option<RefView> {
    let chat = ev.chat.map(|r| r.pr.trim()).filter(|p| !p.is_empty());
    let found = ev
        .statusline_pr
        .map(|n| (n.to_string(), RefSource::Statusline))
        .or_else(|| ev.title.and_then(pr_from_title).map(|p| (p, RefSource::Title)))
        .or_else(|| chat.map(|p| (p.to_string(), RefSource::Chat)));
    found.map(|(n, source)| ref_view(format!("#{n}"), source, lang))
}
