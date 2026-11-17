//! Tests zu `github.rs`, gegen eine Attrappe statt gh. Der echte Probelauf ist
//! `#[ignore]`: `cargo test -p deck-services dash::github -- --ignored --nocapture`.

use std::collections::VecDeque;
use std::sync::mpsc;

use super::*;

/// Liefert die vorbereiteten Antworten der Reihe nach und merkt sich die Abfragen.
#[derive(Default)]
struct FakeGh {
    answers: Mutex<VecDeque<Result<String, String>>>,
    queries: Mutex<Vec<String>>,
}

impl FakeGh {
    fn with(answers: Vec<Result<String, String>>) -> Arc<FakeGh> {
        Arc::new(FakeGh { answers: Mutex::new(answers.into()), ..FakeGh::default() })
    }
}

impl GhApi for FakeGh {
    fn graphql(&self, query: &str) -> Result<String, String> {
        lock(&self.queries).push(query.to_string());
        lock(&self.answers).pop_front().unwrap_or_else(|| Err("keine Antwort mehr".into()))
    }
}

fn today() -> (NaiveDate, i32) {
    (NaiveDate::from_ymd_opt(2026, 11, 15).unwrap(), 3600)
}

fn service(gh: Arc<FakeGh>, dir: &Path) -> GithubService {
    let log = LogFile::new(dir.join(LOG_FILE), 100_000, Rotate::Truncate);
    GithubService::new(gh, log, Arc::new(|| 1_000.0), Arc::new(today))
}

fn answer(open: usize, merged_total: u64) -> Result<String, String> {
    let nodes: Vec<Value> = (0..open).map(|i| serde_json::json!({ "number": i + 1, "repository": { "nameWithOwner": "Org/r" } })).collect();
    Ok(serde_json::json!({ "data": {
        "open": { "nodes": nodes },
        "createdToday": { "issueCount": 1 },
        "mergedToday": { "issueCount": 0 },
        "merged0": { "issueCount": merged_total, "nodes": [ { "repository": { "nameWithOwner": "Org/r" }, "headRefName": "b", "headRefOid": "abc" } ] }
    } })
    .to_string())
}

fn log_text(dir: &Path) -> String {
    std::fs::read_to_string(dir.join(LOG_FILE)).unwrap_or_default()
}

#[test]
fn query_asks_for_today_in_local_time_and_thirty_days_of_merged_prs_without_gaps() {
    let q = build_query(NaiveDate::from_ymd_opt(2026, 11, 15).unwrap(), 3600);
    assert!(q.contains("open: search(query:\"is:pr author:@me is:open archived:false\""));
    assert!(q.contains("createdToday: search(query:\"is:pr author:@me created:>=2026-11-15T00:00:00+01:00\""));
    assert!(q.contains("mergedToday: search(query:\"is:pr author:@me is:merged merged:>=2026-11-15T00:00:00+01:00\""));
    assert!(q.contains("merged0: search(query:\"is:pr author:@me is:merged merged:>=2026-11-11\""));
    assert!(q.contains("merged1: search(query:\"is:pr author:@me is:merged merged:2026-11-06..2026-11-10\""));
    assert!(q.contains("merged5: search(query:\"is:pr author:@me is:merged merged:2026-10-17..2026-10-21\""));
    assert!(!q.contains("merged6"));
    assert!(q.contains("headRefOid") && q.contains("reviewThreads(first:100)"));
    let west = build_query(NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(), -(4 * 3600 + 1800));
    assert!(west.contains("created:>=2026-07-01T00:00:00-04:30"));
}

#[test]
fn a_successful_fetch_replaces_the_snapshot_and_stamps_the_time() {
    let d = tempfile::tempdir().unwrap();
    let svc = service(FakeGh::with(vec![answer(2, 1)]), d.path());
    assert!(svc.fetch_once());
    let s = svc.snapshot();
    assert_eq!((s.open.len(), s.created_today, s.merged.len(), s.fetched_at, s.error), (2, Some(1), 1, 1_000.0, None));
    assert!(log_text(d.path()).contains("erster Abruf: 2 offen"));
}

#[test]
fn a_failed_fetch_keeps_old_data_and_logs_only_the_first_failure_and_the_recovery() {
    let d = tempfile::tempdir().unwrap();
    let gh = FakeGh::with(vec![answer(3, 1), Err("gh: HTTP 502".into()), Err("gh: HTTP 502".into()), Ok("{\"message\":\"Bad credentials\"}".into()), answer(1, 1)]);
    let svc = service(gh, d.path());
    assert!(svc.fetch_once());
    assert!(!svc.fetch_once());
    let s = svc.snapshot();
    assert_eq!((s.open.len(), s.error.as_deref()), (3, Some("gh: HTTP 502")), "alte Daten bleiben");
    assert!(!svc.fetch_once());
    assert!(!svc.fetch_once());
    assert_eq!(svc.snapshot().error.as_deref(), Some("Bad credentials"));
    assert!(svc.fetch_once());
    assert_eq!(svc.snapshot().error, None);
    let log = log_text(d.path());
    assert_eq!(log.matches("Abruf gescheitert").count(), 1, "{log}");
    assert!(log.contains("klappt wieder nach 3 Fehlversuchen: 1 offen"), "{log}");
}

#[test]
fn garbage_output_is_an_error_not_a_panic() {
    let d = tempfile::tempdir().unwrap();
    let svc = service(FakeGh::with(vec![Ok("{kaputt".into())]), d.path());
    assert!(!svc.fetch_once());
    assert!(svc.snapshot().error.unwrap().starts_with("Antwort kein JSON"));
}

#[test]
fn truncated_merged_windows_are_logged_when_that_changes() {
    let d = tempfile::tempdir().unwrap();
    let svc = service(FakeGh::with(vec![answer(0, 150), answer(0, 150), answer(0, 1)]), d.path());
    for _ in 0..3 {
        svc.fetch_once();
    }
    let log = log_text(d.path());
    assert_eq!(log.matches("1 von 6 Zeitfenstern abgeschnitten").count(), 1, "{log}");
    assert_eq!(log.matches("0 von 6 Zeitfenstern abgeschnitten").count(), 1, "{log}");
}

#[test]
fn the_thread_fetches_at_once_wakes_the_app_and_refresh_fetches_again() {
    let d = tempfile::tempdir().unwrap();
    let gh = FakeGh::with(vec![answer(1, 1), answer(2, 1)]);
    let svc = service(gh.clone(), d.path());
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    svc.start(Arc::new(move || {
        let _ = lock(&tx).send(());
    }));
    rx.recv_timeout(Duration::from_secs(5)).expect("erster Abruf sofort");
    assert_eq!(svc.snapshot().open.len(), 1);
    svc.refresh();
    rx.recv_timeout(Duration::from_secs(5)).expect("Abruf auf Anstoß");
    assert_eq!(svc.snapshot().open.len(), 2);
    svc.stop();
    assert_eq!(lock(&gh.queries).len(), 2);
}

/// Echter Probelauf, nur lesend: `cargo test -p deck-services dash::github -- --ignored --nocapture`.
#[test]
#[ignore]
fn real_query_runs_against_github() {
    let d = tempfile::tempdir().unwrap();
    let log = LogFile::new(d.path().join(LOG_FILE), 100_000, Rotate::Truncate);
    let svc = GithubService::new(Arc::new(GhCli::default()), log, Arc::new(now_unix), Arc::new(local_today));
    let ok = svc.fetch_once();
    let s = svc.snapshot();
    println!(
        "ok={ok} offen={} heute erstellt={:?} heute gemergt={:?} gemergt 30 Tg.={} Fehler={:?}",
        s.open.len(),
        s.created_today,
        s.merged_today,
        s.merged.len(),
        s.error
    );
    println!("{}", log_text(d.path()));
    assert!(ok);
}
