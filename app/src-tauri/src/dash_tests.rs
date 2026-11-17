//! Tests zu `dash.rs`.

use super::*;

#[test]
fn switched_off_there_is_no_view() {
    let mut d = Dash::inert();
    let input = DashInput { now: crate::now(), lang: Lang::German, enabled: false, in_use: Vec::new(), jira_prefix: "ORPH", feed: Default::default() };
    assert!(d.view(&input).is_none());
}

/// Probelauf gegen die echten Quellen (gh, ssh, git, Transcripts; nur lesend, nichts
/// wird entfernt oder beendet): schreibt die Ansicht als JSON nach `DASH_DUMP`, damit der
/// Browser-Mock sie zeigen kann (`?dash=real`).
/// `DASH_DUMP=app/dash-real.json cargo test -p agent-deck dash::tests::real -- --ignored --nocapture`
#[test]
#[ignore]
fn real_dash_dump() {
    let paths = Paths::from_env();
    let src = Sources::system(&paths, Arc::new(|| {}));
    let now = crate::now();
    src.github.fetch_once();
    src.costs.set_prefix("ORPH");
    src.costs.scan_now(now);
    src.titan.poll_once();
    src.cleanup.set_merged(src.github.snapshot().merged);
    src.cleanup.scan_now();
    let prs = src.github.snapshot();
    // Feed nur gebaut, nie eine Runde (die würde mergen): Gedächtnis „seit zwei Tagen“,
    // Ursachen ohne Job-Logs.
    let st = deck_core::dash::feed_state::FeedState::fresh(now - 2.0 * 86_400.0);
    let causes = prs.open.iter().map(|p| (p.key(), deck_core::dash::ci_cause::cause(&p.extra.checks, &p.extra.files, &Default::default()))).collect();
    let input = deck_core::dash::feed::FeedInput { snap: &prs, st: &st, causes: &causes, waiting: &[], now, lang: Lang::German };
    let (feed, popup) = deck_core::dash::feed::build(&input);
    let view = DashView {
        costs: src.costs.view(),
        prs: board::prs(&prs, now, Lang::German),
        reviews: board::reviews(&prs, now, Lang::German),
        hidden_reviews: Vec::new(),
        titan: board::titan(&src.titan.snapshot(), Lang::German),
        cleanup: board::cleanup(&src.cleanup.snapshot(), Lang::German),
        notice: None,
        feed: Some(feed),
        popup,
    };
    let json = serde_json::to_string_pretty(&view).unwrap();
    match std::env::var("DASH_DUMP") {
        Ok(path) => std::fs::write(path, json).unwrap(),
        Err(_) => println!("{json}"),
    }
}

#[test]
fn hiding_a_review_twice_changes_nothing_and_showing_brings_it_back() {
    let mut d = Dash::inert();
    let url = "https://github.com/o/r/pull/7";
    assert!(d.set_hidden(url, true));
    assert!(!d.set_hidden(url, true));
    assert_eq!(d.hidden, [url]);
    assert!(d.set_hidden(url, false));
    assert!(d.hidden.is_empty());
}

#[test]
fn hidden_reviews_survive_a_restart_and_a_broken_file_means_none() {
    let dir = std::env::temp_dir().join(format!("deck-hidden-{}", std::process::id()));
    let path = dir.join(HIDDEN_FILE);
    let mut d = Dash { hidden_path: Some(path.clone()), ..Dash::inert() };
    d.set_hidden("https://github.com/o/r/pull/7", true);
    assert_eq!(load_hidden(&path), ["https://github.com/o/r/pull/7"]);
    std::fs::write(&path, b"{kaputt").unwrap();
    assert!(load_hidden(&path).is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_notice_shows_for_a_while_then_goes() {
    let mut d = Dash::inert();
    let now = 1000.0;
    d.say("Approve gesendet".into(), now);
    let input = |now| DashInput { now, lang: Lang::German, enabled: true, in_use: Vec::new(), jira_prefix: "ORPH", feed: Default::default() };
    assert_eq!(d.view(&input(now + 1.0)).unwrap().notice.as_deref(), Some("Approve gesendet"));
    assert_eq!(d.view(&input(now + NOTICE_S + 1.0)).unwrap().notice, None);
}
