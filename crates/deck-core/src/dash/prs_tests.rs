//! Tests zu `prs.rs`: Zerlegen der GraphQL-Antwort und wer am Zug ist.

use super::*;
use serde_json::json;


/// Gekürzte echte Antwort (Form wie am 15.11.2026 abgefragt), Titel erfunden.
fn fixture() -> Value {
    json!({ "data": {
        "open": { "issueCount": 3, "nodes": [
            { "number": 213, "title": "fix(ORPH-4255): run tests pinned", "url": "https://github.com/o/orpheus-management-api/pull/213",
              "repository": { "nameWithOwner": "Org/orpheus-management-api" }, "isDraft": false, "createdAt": "2026-11-14T09:00:00Z",
              "mergeable": "MERGEABLE", "reviewDecision": "APPROVED", "headRefName": "fix/ORPH-4255",
              "commits": { "nodes": [ { "commit": { "statusCheckRollup": { "state": "SUCCESS" } } } ] },
              "reviewThreads": { "nodes": [ { "isResolved": true } ] } },
            { "number": 88, "title": "feat: neue Karte", "url": "u88",
              "repository": { "nameWithOwner": "Org/agent-deck" }, "isDraft": false,
              "mergeable": "CONFLICTING", "reviewDecision": "CHANGES_REQUESTED", "headRefName": "dash",
              "commits": { "nodes": [ { "commit": { "statusCheckRollup": { "state": "FAILURE" } } } ] },
              "reviewThreads": { "nodes": [ { "isResolved": false }, { "isResolved": false }, { "isResolved": true } ] } },
            { "number": 7, "title": "Entwurf", "isDraft": true, "commits": { "nodes": [ { "commit": { "statusCheckRollup": null } } ] } },
            {}
        ] },
        "createdToday": { "issueCount": 2 },
        "mergedToday": { "issueCount": 1 },
        "merged0": { "issueCount": 2, "nodes": [
            { "repository": { "nameWithOwner": "Org/agent-deck" }, "headRefName": "old", "headRefOid": "abc", "mergedAt": "2026-11-14T10:00:00Z" },
            { "repository": { "nameWithOwner": "Org/agent-deck" }, "headRefName": "", "headRefOid": "def" }
        ] },
        "merged1": { "issueCount": 1, "nodes": [
            { "repository": { "nameWithOwner": "Org/helm-charts" }, "headRefName": "x", "headRefOid": "123" }
        ] }
    } })
}

fn pr(number: u64) -> PrInfo {
    PrInfo { repo: "Org/orpheus-client-electron".into(), number, title: format!("PR {number}"), url: format!("u{number}"), ..PrInfo::default() }
}

fn ready(number: u64) -> PrInfo {
    PrInfo { ci: CiState::Success, review: ReviewState::Approved, mergeable: true, ..pr(number) }
}


#[test]
fn parser_reads_states_threads_counts_and_merged_windows() {
    let s = parse(&fixture()).unwrap();
    assert_eq!(s.open.len(), 3, "der leere Union-Knoten fällt weg");
    let a = &s.open[0];
    assert_eq!((a.number, a.repo.as_str(), a.head_ref.as_str()), (213, "Org/orpheus-management-api", "fix/ORPH-4255"));
    assert!(a.is_ready());
    assert_eq!(a.open_threads, 0);
    let b = &s.open[1];
    assert_eq!((b.ci, b.review, b.mergeable, b.open_threads), (CiState::Failure, ReviewState::ChangesRequested, false, 2));
    let c = &s.open[2];
    assert_eq!((c.ci, c.review, c.draft, c.repo.as_str()), (CiState::None, ReviewState::None, true, ""));
    assert_eq!((s.created_today, s.merged_today), (Some(2), Some(1)));
    let merged: Vec<(&str, &str)> = s.merged.iter().map(|m| (m.head_ref.as_str(), m.head_oid.as_str())).collect();
    assert_eq!(merged, [("old", "abc"), ("x", "123")], "ohne Branch kein Eintrag; alle merged-Fenster zählen");
}

#[test]
fn parser_treats_missing_counts_as_unknown_and_missing_data_as_error() {
    let s = parse(&json!({ "data": { "open": { "nodes": [] } } })).unwrap();
    assert_eq!((s.created_today, s.merged_today, s.open.len()), (None, None, 0));
    let e = parse(&json!({ "errors": [ { "message": "Bad credentials" } ] })).unwrap_err();
    assert_eq!(e, "Bad credentials");
    assert!(parse(&json!({})).is_err());
    // Teilfehler neben Daten werden hingenommen.
    assert!(parse(&json!({ "data": { "open": { "nodes": [] } }, "errors": [ { "message": "SAML" } ] })).is_ok());
}

#[test]
fn ci_states_map_pending_to_running_and_error_to_failure() {
    let n = |state: &str| json!({ "data": { "open": { "nodes": [ { "number": 1, "commits": { "nodes": [ { "commit": { "statusCheckRollup": { "state": state } } } ] } } ] } } });
    let ci = |state: &str| parse(&n(state)).unwrap().open[0].ci;
    assert_eq!(ci("PENDING"), CiState::Running);
    assert_eq!(ci("EXPECTED"), CiState::Running);
    assert_eq!(ci("ERROR"), CiState::Failure);
    assert_eq!(ci("WHATEVER"), CiState::None);
}

#[test]
fn titles_lose_their_conventional_commit_prefix_and_start_upper_case() {
    assert_eq!(short_title("fix(ORPH-4255): run tests pinned"), "Run tests pinned");
    assert_eq!(short_title("feat!: drop api"), "Drop api");
    assert_eq!(short_title("Kein Präfix hier"), "Kein Präfix hier");
    assert_eq!(short_title("chore:"), "Chore:");
    assert_eq!(short_repo("Org/orpheus-management-api"), "api");
    assert_eq!(short_repo("Org/orpheus-client-electron"), "client");
    assert_eq!(short_repo("Org/orpheus-keycloak"), "keycloak");
    assert_eq!(short_repo("Org/agent-deck"), "agent-deck");
}


fn changes(waiting: &[&str], changed: bool) -> PrInfo {
    PrInfo {
        review: ReviewState::ChangesRequested,
        ci: CiState::Success,
        waiting_on: waiting.iter().map(|s| s.to_string()).collect(),
        changed_since_request: changed,
        ..pr(1)
    }
}

#[test]
fn changes_requested_with_the_reviewer_asked_again_waits_on_the_reviewer() {
    assert_eq!(changes(&["reviewer1"], true).ball(), PrBall::ReReview);
    assert!(!PrBall::ReReview.is_mine());
}

#[test]
fn changes_done_but_reviewer_not_asked_again_is_mine_to_nudge() {
    assert_eq!(changes(&[], true).ball(), PrBall::AskReviewer);
    assert_eq!(changes(&[], false).ball(), PrBall::ChangesOpen);
    assert!(PrBall::AskReviewer.is_mine() && PrBall::ChangesOpen.is_mine());
}

#[test]
fn draft_beats_everything_and_red_ci_beats_review_state() {
    assert_eq!(PrInfo { draft: true, ci: CiState::Failure, ..ready(1) }.ball(), PrBall::Draft);
    assert_eq!(PrInfo { ci: CiState::Failure, ..changes(&["x"], true) }.ball(), PrBall::CiRed);
    assert_eq!(ready(1).ball(), PrBall::Ready);
    assert_eq!(pr(1).ball(), PrBall::Review);
}

#[test]
fn review_requests_bots_and_commits_after_the_request_are_read() {
    let v = json!({ "data": {
        "open": { "nodes": [ { "number": 5, "reviewDecision": "CHANGES_REQUESTED",
            "reviewRequests": { "nodes": [ { "requestedReviewer": { "login": "reviewer1" } }, { "requestedReviewer": { "login": "claude" } }, { "requestedReviewer": { "name": "backend" } } ] },
            "timelineItems": { "nodes": [
                { "state": "CHANGES_REQUESTED", "submittedAt": "2026-11-14T00:37:00Z" },
                { "commit": { "committedDate": "2026-11-14T00:55:00Z" } },
                { "state": "COMMENTED", "submittedAt": "2026-11-14T01:24:00Z" } ] } } ] },
        "reviews": { "nodes": [ { "number": 77, "title": "docs: x", "url": "https://github.com/o/r/pull/77", "repository": { "nameWithOwner": "Org/idm-homepage" },
            "createdAt": "2026-11-10T08:00:00Z", "additions": 3, "deletions": 1, "commits": { "nodes": [ { "commit": { "statusCheckRollup": { "state": "FAILURE" } } } ] } } ] }
    } });
    let s = parse(&v).unwrap();
    assert_eq!(s.open[0].waiting_on, ["reviewer1", "backend"], "Bots zählen nicht");
    assert!(s.open[0].changed_since_request);
    assert_eq!(s.review_requests.len(), 1);
    assert_eq!((s.review_requests[0].number, s.review_requests[0].ci), (77, CiState::Failure));
}
