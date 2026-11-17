use super::*;
use crate::dash::cleanup::WtCandidate;
use crate::dash::prs::{PrInfo, ReviewRequest, ReviewState};
use crate::dash::titan::TitanRun;

const NOW: f64 = 1_794_760_000.0;

fn pr(number: u64, review: ReviewState, waiting: &[&str]) -> PrInfo {
    PrInfo {
        repo: "Org/orpheus-client-electron".into(),
        number,
        title: format!("feat(ORPH-{number}): do it"),
        url: format!("https://github.com/o/r/pull/{number}"),
        ci: CiState::Success,
        review,
        waiting_on: waiting.iter().map(|s| s.to_string()).collect(),
        ..PrInfo::default()
    }
}

#[test]
fn prs_sort_ready_first_and_count_whose_turn_it_is() {
    let snap = PrSnapshot {
        open: vec![
            pr(1, ReviewState::Required, &["reviewer1"]),
            pr(2, ReviewState::ChangesRequested, &["reviewer1"]),
            PrInfo { mergeable: true, ..pr(3, ReviewState::Approved, &[]) },
            PrInfo { changed_since_request: true, ..pr(4, ReviewState::ChangesRequested, &[]) },
        ],
        fetched_at: NOW - 60.0,
        ..PrSnapshot::default()
    };
    let v = prs(&snap, NOW, Lang::German);
    let order: Vec<u64> = v.rows.iter().map(|r| r.number).collect();
    assert_eq!(order, [3, 4, 2, 1]);
    assert_eq!((v.ready, v.mine, v.waiting), (1, 1, 2));
    assert_eq!(v.rows[2].status, "wartet auf Re-Review");
    assert_eq!(v.rows[2].meta, "bei reviewer1 · CI grün");
    assert_eq!(v.rows[1].status, "Reviewer anfragen");
    assert_eq!(v.rows[0].ticket, "ORPH-3");
    assert!(!v.stale);
}

#[test]
fn reviews_are_newest_first_with_age_and_size() {
    let r = |n: u64, days: f64| ReviewRequest { number: n, repo: "Org/helm-charts".into(), created_at: NOW - days * 86_400.0, additions: 15, deletions: 0, ..ReviewRequest::default() };
    let snap = PrSnapshot { review_requests: vec![r(1, 47.5), r(2, 0.2)], ..PrSnapshot::default() };
    let v = reviews(&snap, NOW, Lang::German);
    assert_eq!(v[0].number, 2);
    assert_eq!(v[0].meta, "seit heute · +15 −0");
    assert_eq!(v[1].meta, "seit 47 Tagen · +15 −0");
}

#[test]
fn hidden_reviews_move_down_and_are_forgotten_once_the_request_is_gone() {
    let r = |n: u64| ReviewRequest { number: n, url: format!("https://github.com/o/r/pull/{n}"), created_at: NOW - n as f64, ..ReviewRequest::default() };
    let mut snap = PrSnapshot { review_requests: vec![r(1), r(2)], fetched_at: NOW, ..PrSnapshot::default() };
    let mut hidden = vec!["https://github.com/o/r/pull/2".to_string(), "https://github.com/o/r/pull/9".to_string()];
    let (shown, gone) = split_reviews(reviews(&snap, NOW, Lang::German), &hidden);
    assert_eq!((shown.len(), gone.len()), (1, 1));
    assert_eq!((shown[0].number, gone[0].number), (1, 2));

    assert!(prune_hidden(&mut hidden, &snap), "#9 wartet nicht mehr auf mich");
    assert_eq!(hidden, ["https://github.com/o/r/pull/2"]);

    snap.review_requests.clear();
    snap.error = Some("gh: timeout".into());
    assert!(!prune_hidden(&mut hidden, &snap), "ein gescheiterter Abruf vergisst nichts");
    assert!(!prune_hidden(&mut hidden, &PrSnapshot::default()), "vor dem ersten Abruf auch nicht");
    assert_eq!(hidden.len(), 1);
}

#[test]
fn titan_shows_only_orphaned_runs_and_nothing_without_them() {
    let run = |name: &str, orphan: bool| TitanRun { name: name.into(), age_s: Some(150_000), orphan };
    let mut snap = TitanSnapshot { reachable: true, cores: 32, runs: vec![run("4258-w206-3c2272", true), run("4259-x-abcdef", false)], ..TitanSnapshot::default() };
    let v = titan(&snap, Lang::German).unwrap();
    assert_eq!(v.runs.len(), 1);
    assert_eq!((v.runs[0].label.as_str(), v.runs[0].age.as_str()), ("4258-w206", "seit 42 Std."));
    snap.runs.retain(|r| !r.orphan);
    assert_eq!(titan(&snap, Lang::German), None, "ein Lauf mit Aufrufer ist kein Problem");
}

#[test]
fn cleanup_shows_only_with_something_removable() {
    let c = |f: &str, state: WtState| WtCandidate { folder: f.into(), repo: "Org/helm-charts".into(), state, ..WtCandidate::default() };
    let none = CleanupSnapshot { candidates: vec![c("a", WtState::Changed), c("b", WtState::InUse)], ..CleanupSnapshot::default() };
    assert_eq!(cleanup(&none, Lang::German), None);
    let some = CleanupSnapshot { candidates: vec![c("a", WtState::Changed), c("b", WtState::Removable), c("x", WtState::InUse)], ..CleanupSnapshot::default() };
    let v = cleanup(&some, Lang::German).unwrap();
    assert_eq!((v.removable.len(), v.changed.len()), (1, 1), "in Benutzung erscheint nirgends");
}

#[test]
fn ui_texts_contain_no_dashes_except_the_minus_of_deletions() {
    let snap = PrSnapshot { open: vec![pr(1, ReviewState::ChangesRequested, &[])], error: Some("gh: Exit 1".into()), ..PrSnapshot::default() };
    for lang in [Lang::German, Lang::English] {
        let text = format!("{:?}", prs(&snap, NOW, lang));
        assert!(!text.contains('—') && !text.contains('–'), "{text}");
    }
}
