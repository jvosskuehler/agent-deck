//! Tests zu `pr_extra.rs`: Kommentare, Threads, Reviews, Checks aus dem PR-Knoten.

use super::*;
use serde_json::json;

fn node() -> Value {
    json!({
        "body": "after #12", "mergeStateStatus": "CLEAN", "mergeable": "MERGEABLE",
        "files": { "nodes": [ { "path": "src/a.rs" } ] },
        "assignees": { "nodes": [ { "login": "me" } ] },
        "commits": { "nodes": [ { "commit": { "oid": "abc", "committedDate": "2026-11-17T10:00:00Z", "statusCheckRollup": { "state": "FAILURE", "contexts": { "nodes": [
            { "__typename": "CheckRun", "name": "codeql / Analyze", "conclusion": "FAILURE", "completedAt": "2026-11-17T10:05:00Z", "databaseId": 7,
              "checkSuite": { "workflowRun": { "workflow": { "name": "CodeQL" } } } },
            { "__typename": "CheckRun", "name": "Build", "conclusion": null },
            { "__typename": "StatusContext", "context": "ext", "state": "SUCCESS", "createdAt": "2026-11-17T10:01:00Z" } ] } } } } ] },
        "comments": { "nodes": [
            { "id": "c1", "author": { "login": "reviewer1", "__typename": "User" }, "createdAt": "2026-11-17T11:00:00Z", "bodyText": "Why?", "url": "u1" },
            { "id": "c2", "author": { "login": "some-tool", "__typename": "Bot" }, "createdAt": "2026-11-17T11:01:00Z", "bodyText": "report" },
            { "id": "c3", "author": null } ] },
        "reviewThreads": { "nodes": [ { "id": "t1", "isResolved": false,
            "head": { "nodes": [ { "id": "h1", "author": { "login": "claude" }, "createdAt": "2026-11-17T09:00:00Z" } ] },
            "tail": { "nodes": [ { "id": "h2", "author": { "login": "me", "__typename": "User" }, "createdAt": "2026-11-17T12:00:00Z" } ] } } ] },
        "timelineItems": { "nodes": [
            { "id": "r1", "author": { "login": "reviewer1" }, "state": "CHANGES_REQUESTED", "submittedAt": "2026-11-17T08:00:00Z" },
            { "commit": { "committedDate": "2026-11-17T08:30:00Z" } },
            { "id": "r2", "author": { "login": "reviewer1" }, "state": "APPROVED", "submittedAt": "2026-11-17T09:30:00Z" } ] }
    })
}

#[test]
fn the_node_yields_comments_threads_reviews_and_checks_with_bots_marked() {
    let x = parse(&node());
    assert_eq!((x.body.as_str(), x.merge_state.as_str(), x.conflicting, x.head_oid.as_str()), ("after #12", "CLEAN", false, "abc"));
    assert_eq!(x.files, ["src/a.rs"]);
    assert_eq!(x.comments.len(), 2, "ohne Autor kein Kommentar");
    assert!(!x.comments[0].bot && x.comments[1].bot, "Typ Bot ist ein Bot");
    assert!(x.threads[0].by_bot(), "claude ist ein bekannter Bot");
    assert_eq!(x.threads[0].last().unwrap().author, "me");
    assert_eq!(x.reviews.len(), 2, "Commits in der Zeitleiste sind keine Reviews");
    assert_eq!(x.checks.len(), 3);
    assert!(x.checks[0].failed && x.checks[0].job_id == 7 && x.checks[0].workflow == "CodeQL");
    assert!(x.checks[1].running && !x.checks[2].failed);
}

#[test]
fn my_last_activity_is_my_newest_comment_or_the_last_commit() {
    let x = parse(&node());
    assert_eq!(x.my_last_activity("me"), parse_iso("2026-11-17T12:00:00Z").unwrap());
    assert_eq!(x.my_last_activity("nobody"), parse_iso("2026-11-17T10:00:00Z").unwrap());
}

#[test]
fn a_later_approval_lifts_the_changes_request_of_the_same_human() {
    let x = parse(&node());
    assert!(x.changes_requested_by().is_empty());
    assert!(x.approved_by_human());
    let mut y = x.clone();
    y.reviews[0].comment.at = y.reviews[1].comment.at + 1.0;
    assert_eq!(y.changes_requested_by(), ["reviewer1"]);
    assert!(!y.approved_by_human());
}

#[test]
fn details_attach_to_the_open_pr_with_the_same_repo_and_number() {
    use crate::dash::prs::PrInfo;
    let mut open = vec![PrInfo { repo: "Org/r".into(), number: 3, ..PrInfo::default() }, PrInfo { repo: "Org/s".into(), number: 3, ..PrInfo::default() }];
    let mut n = node();
    n["number"] = json!(3);
    n["repository"] = json!({ "nameWithOwner": "Org/s" });
    attach(&mut open, &json!({ "data": { "detail": { "nodes": [n, {}] } } })).unwrap();
    assert_eq!((open[0].extra.head_oid.as_str(), open[1].extra.head_oid.as_str()), ("", "abc"));
    assert!(attach(&mut open, &json!({ "errors": [ { "message": "502" } ] })).is_err());
}

#[test]
fn reviews_carry_commit_and_claude_classification_and_draft_switches_count_as_my_reaction() {
    let mut n = node();
    n["timelineItems"]["nodes"] = json!([
        { "id": "a", "author": { "login": "claude", "__typename": "Bot" }, "state": "APPROVED", "submittedAt": "2026-11-17T10:10:00Z",
          "raw": "Looks good.
<!-- claude-classification class=high risk=high -->", "commit": { "oid": "abc" } },
        { "actor": { "login": "me" }, "createdAt": "2026-11-17T13:00:00Z" } ]);
    let x = parse(&n);
    assert_eq!((x.reviews[0].oid.as_str(), x.reviews[0].classification.as_str()), ("abc", "high"));
    assert!(x.approved_current(), "Claude-Freigabe am Kopf zählt");
    assert!(x.claude_high() && !x.approved_by_human());
    assert_eq!(x.my_last_activity("me"), parse_iso("2026-11-17T13:00:00Z").unwrap(), "auf Draft gestellt = reagiert");
    let mut old = x.clone();
    old.head_oid = "newer".into();
    assert!(!old.approved_current(), "Freigabe eines alten Stands zählt nicht");
    assert_eq!(classification_of("<!-- claude-classification class=low risk=low -->"), "low");
    assert!(is_claude("claude[bot]") && !is_claude("claudette"));
}

#[test]
fn an_old_red_run_is_superseded_by_a_newer_run_of_the_same_check() {
    let mut n = node();
    n["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["contexts"]["nodes"] = json!([
        { "__typename": "CheckRun", "name": "lint", "conclusion": "FAILURE", "completedAt": "2026-11-17T10:00:00Z" },
        { "__typename": "CheckRun", "name": "lint", "conclusion": "SUCCESS", "completedAt": "2026-11-17T11:00:00Z" },
        { "__typename": "CheckRun", "name": "test", "conclusion": "SUCCESS", "completedAt": "2026-11-17T11:00:00Z" } ]);
    let x = parse(&n);
    assert_eq!(x.checks.len(), 2);
    assert!(x.checks.iter().all(|c| !c.failed));
}
