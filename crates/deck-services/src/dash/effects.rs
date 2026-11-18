//! Die Wirkungen des PR-Feeds nach außen, je hinter einem Trait: GitHub schreiben
//! ([`GhWrite`]: Reviewer, Assignee, Merge, Branch löschen, antworten, Thread auflösen),
//! Jira-Status ([`JiraApi`] über `acli`). Tests bekommen Attrappen; die echten Fassungen
//! prüfen zusätzlich das Schutzgitter (`guard`), damit kein Test je etwas auslöst.
//!
//! Gemergt wird mit `gh pr merge --merge`, nie `--auto` und nie `--delete-branch` (das
//! löscht auch lokal und wechselt im Haupt-Checkout den Branch); der Remote-Branch geht
//! danach über die REST-API.

use std::process::Command;
use std::time::Duration;

use serde_json::Value;

use super::hidden_cmd::{self, short_error};

/// Zeitlimit je Aufruf.
pub const TIMEOUT: Duration = Duration::from_secs(60);

/// GitHub schreiben.
pub trait GhWrite: Send + Sync {
    fn add_reviewer(&self, url: &str, login: &str) -> Result<(), String>;
    fn add_assignee_me(&self, url: &str) -> Result<(), String>;
    fn merge(&self, url: &str) -> Result<(), String>;
    fn delete_branch(&self, repo: &str, branch: &str) -> Result<(), String>;
    /// Antwort in einen Review-Thread.
    fn reply_thread(&self, thread_id: &str, body: &str) -> Result<(), String>;
    fn resolve_thread(&self, thread_id: &str) -> Result<(), String>;
    /// Kommentar unter den PR.
    fn comment(&self, url: &str, body: &str) -> Result<(), String>;
}

/// Jira über `acli`.
pub trait JiraApi: Send + Sync {
    /// Aktueller Status (`waiting for review`, `Done` …).
    fn status(&self, key: &str) -> Result<String, String>;
    fn transition(&self, key: &str, status: &str) -> Result<(), String>;
}

fn gh(args: &[&str]) -> Result<String, String> {
    crate::guard::check("gh schreibt").map_err(|e| e.to_string())?;
    let mut cmd = Command::new("gh");
    cmd.args(args).env_remove("GITHUB_TOKEN").env_remove("GH_TOKEN");
    let out = hidden_cmd::run(cmd, TIMEOUT)?;
    if out.code == 0 {
        return Ok(out.stdout);
    }
    let why = short_error(&out.stderr);
    Err(if why.is_empty() { format!("gh: Exit {}", out.code) } else { format!("gh: {}", why.trim_start_matches("gh: ")) })
}

fn graphql(query: &str, vars: &[(&str, &str)]) -> Result<(), String> {
    let mut args: Vec<String> = vec!["api".into(), "graphql".into(), "-f".into(), format!("query={query}")];
    for (k, v) in vars {
        args.push("-f".into());
        args.push(format!("{k}={v}"));
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = gh(&refs)?;
    let v: Value = serde_json::from_str(&out).map_err(|_| "Antwort kein JSON".to_string())?;
    match v.pointer("/errors/0/message").and_then(Value::as_str) {
        Some(e) => Err(short_error(e)),
        None => Ok(()),
    }
}

/// Die echte Fassung über `gh`.
#[derive(Debug, Clone, Copy, Default)]
pub struct GhCliWrite;

impl GhWrite for GhCliWrite {
    fn add_reviewer(&self, url: &str, login: &str) -> Result<(), String> {
        gh(&["pr", "edit", url, "--add-reviewer", login]).map(drop)
    }
    fn add_assignee_me(&self, url: &str) -> Result<(), String> {
        gh(&["pr", "edit", url, "--add-assignee", "@me"]).map(drop)
    }
    fn merge(&self, url: &str) -> Result<(), String> {
        gh(&["pr", "merge", url, "--merge"]).map(drop)
    }
    fn delete_branch(&self, repo: &str, branch: &str) -> Result<(), String> {
        gh(&["api", "-X", "DELETE", &format!("repos/{repo}/git/refs/heads/{branch}")]).map(drop)
    }
    fn reply_thread(&self, thread_id: &str, body: &str) -> Result<(), String> {
        graphql(
            "mutation($t:ID!,$b:String!){addPullRequestReviewThreadReply(input:{pullRequestReviewThreadId:$t,body:$b}){comment{id}}}",
            &[("t", thread_id), ("b", body)],
        )
    }
    fn resolve_thread(&self, thread_id: &str) -> Result<(), String> {
        graphql("mutation($t:ID!){resolveReviewThread(input:{threadId:$t}){thread{id}}}", &[("t", thread_id)])
    }
    fn comment(&self, url: &str, body: &str) -> Result<(), String> {
        gh(&["pr", "comment", url, "--body", body]).map(drop)
    }
}

/// Die echte Fassung über `acli` (angemeldet, Site wie in acli eingerichtet).
#[derive(Debug, Clone, Copy, Default)]
pub struct Acli;

fn acli(args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("acli");
    cmd.args(args);
    let out = hidden_cmd::run(cmd, TIMEOUT)?;
    if out.code == 0 {
        return Ok(out.stdout);
    }
    let why = short_error(if out.stderr.trim().is_empty() { &out.stdout } else { &out.stderr });
    Err(if why.is_empty() { format!("acli: Exit {}", out.code) } else { format!("acli: {why}") })
}

/// Status aus der Antwort von `acli jira workitem search --json` (Liste mit `key`,
/// `fields.status.name`).
pub fn status_from_search(text: &str, key: &str) -> Result<String, String> {
    let v: Value = serde_json::from_str(text).map_err(|_| "acli: Antwort kein JSON".to_string())?;
    let items = v.as_array().or_else(|| v.get("issues").and_then(Value::as_array)).ok_or("acli: keine Liste")?;
    items
        .iter()
        .find(|i| i.get("key").and_then(Value::as_str) == Some(key))
        .and_then(|i| i.pointer("/fields/status/name").and_then(Value::as_str))
        .map(str::to_string)
        .ok_or_else(|| format!("{key} nicht gefunden"))
}

impl JiraApi for Acli {
    fn status(&self, key: &str) -> Result<String, String> {
        // Lesen ist ungefährlich, aber in Tests läuft trotzdem kein echtes acli.
        crate::guard::check("acli").map_err(|e| e.to_string())?;
        let jql = format!("key in ({key})");
        let out = acli(&["jira", "workitem", "search", "--jql", &jql, "--fields", "key,status", "--json"])?;
        status_from_search(&out, key)
    }
    fn transition(&self, key: &str, status: &str) -> Result<(), String> {
        crate::guard::check("acli transition").map_err(|e| e.to_string())?;
        acli(&["jira", "workitem", "transition", "--key", key, "--status", status, "--yes"]).map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_jira_status_is_read_from_the_search_list() {
        let text = r#"[{"key":"ORPH-1","fields":{"status":{"name":"waiting for review"}}},{"key":"ORPH-2","fields":{"status":{"name":"Done"}}}]"#;
        assert_eq!(status_from_search(text, "ORPH-2"), Ok("Done".to_string()));
        assert!(status_from_search(text, "ORPH-3").is_err());
        assert!(status_from_search("kaputt", "ORPH-1").is_err());
    }

    #[test]
    fn real_writers_are_locked_in_tests() {
        assert!(GhCliWrite.merge("https://github.com/o/r/pull/1").unwrap_err().contains("gesperrt"));
        assert!(Acli.transition("ORPH-1", "Done").unwrap_err().contains("gesperrt"));
    }
}
