//! Lange Texte aus `config.py`: die Standard-Belegung der Buttons und die Ticket-Prompts,
//! dazu das Einsetzen der Platzhalter (wie `str.format` mit benannten Feldern).
//!
//! Jeder Prompt ist EINZEILIG: er geht als eine Bracketed-Paste in den pty und wird per
//! eigenem Enter abgeschickt. Ein Zeilenumbruch würde ihn zerreißen.

use std::path::Path;

use crate::binding::{jira_key, ticket_branch, ticket_slug};
use crate::config::TICKET_BRANCH_PREFIX;
use crate::paths::{found_ticket_path, worktree_marker_path};
use crate::pystr::squash_ws;

/// Standard-Slash-Buttons: (Beschriftung, Kommando an den aktiven Agenten). Aliasse ohne
/// Version, Claude Code löst sie auf das neueste Modell der Reihe auf.
pub const SEND_BUTTONS: [(&str, &str); 5] = [
    ("Clear", "/clear"),
    ("Opus", "/model opus[1m]"),
    ("Fable", "/model fable"),
    ("Ultracode", "/effort ultracode"),
    ("xhigh", "/effort xhigh"),
];

/// Standard-Prompt-Buttons: (Beschriftung, fertiger Prompt).
pub const PROMPT_BUTTONS: [(&str, &str); 5] = [
    ("Erklären", "Please summarize your last message in German, in simple, clear language: short sentences, no jargon (and if it's unavoidable, briefly explained), at most three to four sentences, so that even someone with no prior knowledge understands it immediately."),
    ("Commit", "Create a git commit for the current changes. Look at the diff first, stage what belongs together, and write one concise Conventional-Commits message (type(scope): summary) that captures the intent — not a file list. Leave out unrelated changes and don't push."),
    ("Push", "Push the current branch to origin. If it has no upstream yet, set it with `git push -u origin HEAD`. Never force-push unless I explicitly ask."),
    ("MR", "Push the current branch and open a GitLab merge request into the default branch. Give it a clear title and a short description of what changed and why (plus how it was tested). Use `glab mr create` if available, otherwise push with `-o merge_request.create`."),
    ("Review", "Rigorously review my current uncommitted changes (git diff plus staged — the work headed into this MR). Use the Task tool to fan out parallel Sonnet subagents, one per lens — correctness, security, concurrency/races, error-handling/resource-leaks, and regressions against stated intent — each reporting every candidate with file:line, severity, and confidence, filtering nothing. Then, as Opus at high reasoning effort, merge and dedupe; for each Critical/High candidate spawn a fresh adversarial subagent given only the diff and the bare claim (not the finder's reasoning), tasked to refute it — trace the real code path or run the relevant test where feasible. Mark each CONFIRMED/PLAUSIBLE/REFUTED; never drop anything silently, just tag Medium/Low. Return one severity-ranked list of verified, real issues, each with file:line and a one-line failure scenario, no style nitpicks in the ranking. If nothing significant surfaces, say so plainly."),
];

/// Prompt für ein zugewiesenes Ticket in einem Repo. Platzhalter: {jira_key} {ticket}
/// {branch} {slug} {wt_marker} {task}.
pub const TICKET_PROMPT: &str = "You are assigned Jira ticket {jira_key}. Work on it in an ISOLATED git worktree so you don't get in the way of other agents in this repo. Set yourself up first: find the repo root with `git rev-parse --show-toplevel` and create a worktree for the branch `{branch}` in a location NEXT TO the repo (e.g. `<repo-root>/../<repo-name>.wt/{slug}`) — if the branch already exists, attach the worktree to it (`git worktree add <path> {branch}`), otherwise create it fresh (`git worktree add <path> -b {branch}`); if the worktree already exists, just use it. Then switch into that worktree (`cd <path>`) and from now on treat it as your working directory for EVERYTHING (reading, editing, commits, commands) — do NOT touch the main checkout under the repo root or the worktrees of other tickets. Write the ABSOLUTE path of this worktree (only the path, one line, with forward slashes, nothing else) into the file `{wt_marker}` — the dashboard uses this to detect the worktree and clean it up again when the agent is closed. Next, look up the Jira ticket {jira_key} with your Atlassian/Jira integration (e.g. the Jira MCP getJiraIssue, or search by that key) and read its summary, description and acceptance criteria. Then give me a SHORT briefing: the worktree path and active branch, plus the ticket's title, type, status and — in 2-4 bullet points — what actually needs to be done. If you cannot reach Jira, tell me so and show the ticket key instead of inventing its content. Then get started with: {task}";

/// Dasselbe für den Heimat-Block über allen Repos: dort gibt es kein
/// `git rev-parse --show-toplevel`, und bewusst KEINE Marker-Datei (das Deck räumt
/// markierte worktrees beim Schließen per `--force` weg; die `wt-*`-worktrees leben aber
/// bis zum Merge). Platzhalter: {jira_key} {number} {root} {task}.
pub const TICKET_PROMPT_HUB: &str = "You are assigned Jira ticket {jira_key}. You are running in {root}, the folder that holds ALL repositories; it is not a git repository itself, and its CLAUDE.md has the repo map and the worktree convention. Set yourself up first: look up {jira_key} with your Atlassian/Jira integration (e.g. the Jira MCP getJiraIssue) and read its summary, description and acceptance criteria. Decide which repositories the ticket touches. For EACH of them, work in a git worktree next to the repos named `{root}/wt-{number}-<short>` (short name as in the CLAUDE.md, e.g. api, ui, dotnet, client) on a branch that follows that repository's own convention from its CLAUDE.md (typically `<type>/{jira_key}-<slug>` from the default branch). If such a worktree or branch already exists, reuse it instead of creating a new one. From now on work ONLY inside those worktrees; never touch the main checkouts or the worktrees of other tickets, and never remove a worktree yourself. Then give me a SHORT briefing: worktree path(s) and branch(es), the ticket's title, type, status and, in 2-4 bullet points, what actually needs to be done. If you cannot reach Jira, tell me so and show the ticket key instead of inventing its content. Then get started with: {task}";

/// Aufgabe, wenn keine mitgegeben wurde.
pub const TICKET_TASK_FALLBACK: &str = "Then wait for my next instruction.";

/// „Im Chat suchen“: der Agent findet die ID selbst und schreibt sie in {marker}.
/// Platzhalter: {prefix} {marker} {wt_marker} {task}.
pub const TICKET_SEARCH_PROMPT: &str = "Look through OUR previous chat and find the ticket number/ID this is about (e.g. ABC-123 / PROJ-42). Then work on that ticket in an ISOLATED git worktree so you don't get in the way of other agents in this repo: find the repo root (`git rev-parse --show-toplevel`) and create a worktree for the branch `{prefix}<lowercased-id-with-hyphens>` in a location NEXT TO the repo (if the branch/worktree already exists, use it), switch into it (`cd`) and from now on work EXCLUSIVELY there — do NOT touch the main checkout or other worktrees. Write the ticket ID you found (only the ID, nothing else, no line breaks) into the file `{marker}` and the ABSOLUTE path of the worktree (only the path, one line, forward slashes) into the file `{wt_marker}` — with this the dashboard shows the ticket and cleans up the worktree again when the agent is closed. Then look up that Jira ticket with your Atlassian/Jira integration (e.g. the Jira MCP getJiraIssue) and read its summary, description and acceptance criteria. Give me a SHORT briefing: which ticket you found, the worktree path, and the ticket's title, type, status and — in 2-4 bullet points — what actually needs to be done. Then get started with: {task}. If you find NO unambiguous ticket number in the chat, ask me instead of guessing.";

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
/// (ORPH-4116 → 4116) für den worktree-Namen `wt-4116-<kurz>`.
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

/// Den einzeiligen Prompt für „Im Chat suchen“ bauen.
pub fn ticket_search_prompt(state_dir: &Path, slot: &str, task: &str) -> String {
    let marker = forward(&found_ticket_path(state_dir, slot));
    let wt_marker = forward(&worktree_marker_path(state_dir, slot));
    fill(
        TICKET_SEARCH_PROMPT,
        &[("prefix", TICKET_BRANCH_PREFIX), ("marker", &marker), ("wt_marker", &wt_marker), ("task", &one_line_task(task))],
    )
}
