//! Lange Texte aus `config.py`: die Ticket-Prompts, dazu das Einsetzen der Platzhalter
//! (wie `str.format` mit benannten Feldern).
//!
//! Jeder Prompt ist EINZEILIG: er geht als eine Bracketed-Paste in den pty und wird per
//! eigenem Enter abgeschickt. Ein Zeilenumbruch würde ihn zerreißen.

use std::path::Path;

use crate::binding::{jira_key, ticket_branch, ticket_slug};
use crate::paths::worktree_marker_path;
use crate::pystr::squash_ws;

/// Prompt für ein zugewiesenes Ticket in einem Repo. Platzhalter: {jira_key} {ticket}
/// {branch} {slug} {wt_marker} {task}.
pub const TICKET_PROMPT: &str = "You are assigned Jira ticket {jira_key}. Work on it in an ISOLATED git worktree so you don't get in the way of other agents in this repo. Set yourself up first: find the repo root with `git rev-parse --show-toplevel` and create a worktree for the branch `{branch}` in a location NEXT TO the repo (e.g. `<repo-root>/../<repo-name>.wt/{slug}`) — if the branch already exists, attach the worktree to it (`git worktree add <path> {branch}`), otherwise create it fresh (`git worktree add <path> -b {branch}`); if the worktree already exists, just use it. Then switch into that worktree (`cd <path>`) and from now on treat it as your working directory for EVERYTHING (reading, editing, commits, commands) — do NOT touch the main checkout under the repo root or the worktrees of other tickets. Write the ABSOLUTE path of this worktree (only the path, one line, with forward slashes, nothing else) into the file `{wt_marker}` — the dashboard uses this to detect the worktree and clean it up again when the agent is closed. Next, look up the Jira ticket {jira_key} with your Atlassian/Jira integration (e.g. the Jira MCP getJiraIssue, or search by that key) and read its summary, description and acceptance criteria. Then give me a SHORT briefing: the worktree path and active branch, plus the ticket's title, type, status and — in 2-4 bullet points — what actually needs to be done. If you cannot reach Jira, tell me so and show the ticket key instead of inventing its content. Then get started with: {task}";

/// Dasselbe für den Heimat-Block über allen Repos: dort gibt es kein
/// `git rev-parse --show-toplevel`, und bewusst KEINE Marker-Datei (das Deck räumt
/// markierte worktrees beim Schließen per `--force` weg; die worktrees unter `wt/` leben
/// aber bis zum Merge). Platzhalter: {jira_key} {number} {root} {task}.
pub const TICKET_PROMPT_HUB: &str = "You are assigned Jira ticket {jira_key}. You are running in {root}, the folder that holds ALL repositories; it is not a git repository itself, and its CLAUDE.md has the repo map and the worktree convention. Set yourself up first: look up {jira_key} with your Atlassian/Jira integration (e.g. the Jira MCP getJiraIssue) and read its summary, description and acceptance criteria. Decide which repositories the ticket touches. For EACH of them, work in a git worktree in the shared worktree folder `{root}/wt/{number}-<short>` (short name as in the CLAUDE.md, e.g. api, ui, dotnet, client) on a branch that follows that repository's own convention from its CLAUDE.md (typically `<type>/{jira_key}-<slug>` from the default branch). If such a worktree or branch already exists, reuse it instead of creating a new one. From now on work ONLY inside those worktrees; never touch the main checkouts or the worktrees of other tickets, and never remove a worktree yourself. Then give me a SHORT briefing: worktree path(s) and branch(es), the ticket's title, type, status and, in 2-4 bullet points, what actually needs to be done. If you cannot reach Jira, tell me so and show the ticket key instead of inventing its content. Then get started with: {task}";

/// Aufgabe, wenn keine mitgegeben wurde.
pub const TICKET_TASK_FALLBACK: &str = "Then wait for my next instruction.";

/// Benannte Platzhalter `{name}` in einem Durchgang einsetzen (eingesetzte Werte werden
/// nicht erneut ersetzt, wie bei `str.format`); Unbekanntes bleibt stehen.
pub fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let hit = after.find('}').and_then(|close| {
            let name = &after[..close];
            values.iter().find(|(k, _)| *k == name).map(|(_, v)| (close, *v))
        });
        match hit {
            Some((close, v)) => {
                out.push_str(v);
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Aufgabe auf eine Zeile glätten; leer → `TICKET_TASK_FALLBACK`.
fn one_line_task(task: &str) -> String {
    let t = squash_ws(task);
    if t.is_empty() { TICKET_TASK_FALLBACK.into() } else { t }
}

fn forward(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// Prompt für den Heimat-Block. {number} ist die reine Ticketnummer
/// (ORPH-4116 → 4116) für den worktree-Ordner `wt/4116-<kurz>`.
pub fn hub_prompt(jira: &str, root: &str, task: &str) -> String {
    let last = jira.rsplit('-').next().unwrap_or(jira);
    let digits: String = last.chars().filter(|c| c.is_numeric()).collect();
    let number = if digits.is_empty() { jira.to_string() } else { digits };
    fill(
        TICKET_PROMPT_HUB,
        &[("jira_key", jira), ("number", &number), ("root", &root.replace('\\', "/")), ("task", task)],
    )
}

/// Den einzeiligen worktree-Prompt für ein zugewiesenes Ticket bauen. `hub_root`: der
/// Ordner, wenn der Block über allen Repos läuft (dann die Hub-Fassung ohne Marker).
pub fn ticket_prompt(
    state_dir: &Path,
    slot: &str,
    ticket: &str,
    task: &str,
    jira_prefix: &str,
    hub_root: Option<&str>,
) -> String {
    let trimmed = crate::pystr::strip(ticket);
    let branch = Some(ticket_branch(ticket, None)).filter(|b| !b.is_empty()).unwrap_or_else(|| format!("ticket/{trimmed}"));
    let slug = Some(ticket_slug(ticket)).filter(|s| !s.is_empty()).unwrap_or_else(|| "ticket".into());
    let jira = Some(jira_key(ticket, Some(jira_prefix))).filter(|j| !j.is_empty()).unwrap_or_else(|| trimmed.into());
    let task = one_line_task(task);
    if let Some(root) = hub_root {
        return hub_prompt(&jira, root, &task);
    }
    let wt_marker = forward(&worktree_marker_path(state_dir, slot));
    fill(
        TICKET_PROMPT,
        &[("ticket", ticket), ("jira_key", &jira), ("branch", &branch), ("slug", &slug), ("wt_marker", &wt_marker), ("task", &task)],
    )
}
