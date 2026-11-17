//! Tests zu `reviewers.rs` mit einer Attrappe statt gh.

use std::sync::atomic::{AtomicUsize, Ordering};

use deck_core::dash::reviewers::ReviewerStatus;

use super::*;

struct FakeDir {
    orgs: Result<Vec<String>, String>,
    members: Result<Vec<Member>, String>,
    member_calls: AtomicUsize,
}

impl OrgDirectory for FakeDir {
    fn my_orgs(&self) -> Result<Vec<String>, String> {
        self.orgs.clone()
    }
    fn members(&self, _org: &str) -> Result<Vec<Member>, String> {
        self.member_calls.fetch_add(1, Ordering::SeqCst);
        self.members.clone()
    }
}

fn dir(orgs: Result<Vec<String>, String>, members: Result<Vec<Member>, String>) -> Arc<FakeDir> {
    Arc::new(FakeDir { orgs, members, member_calls: AtomicUsize::new(0) })
}

fn leo() -> Member {
    Member { login: "leo-mxyz".into(), name: "Leo Müller".into(), emails: vec![] }
}

#[test]
fn the_single_org_of_the_account_is_used_and_reported() {
    let d = dir(Ok(vec!["acme".into()]), Ok(vec![leo()]));
    let r = ReviewerResolver::new(d, CACHE_FOR).resolve("", "Leo");
    assert_eq!((r.org.as_str(), r.note), ("acme", ReviewerNote::None));
    assert_eq!((r.results[0].status, r.results[0].login.as_str()), (ReviewerStatus::Unique, "leo-mxyz"));
}

#[test]
fn several_orgs_without_a_setting_ask_for_a_choice_and_resolve_nothing() {
    let d = dir(Ok(vec!["a".into(), "b".into()]), Ok(vec![leo()]));
    let r = ReviewerResolver::new(d.clone(), CACHE_FOR).resolve("", "Leo, bob");
    assert_eq!((r.note, r.orgs.len()), (ReviewerNote::ChooseOrg, 2));
    assert_eq!(r.results.iter().map(|x| x.login.as_str()).collect::<Vec<_>>(), ["Leo", "bob"]);
    assert_eq!(d.member_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn without_gh_every_entry_stays_unresolved_and_the_reason_is_given() {
    let d = dir(Err("gh nicht startbar: NotFound".into()), Ok(vec![]));
    let r = ReviewerResolver::new(d, CACHE_FOR).resolve("", "leo, name@firma.de");
    assert_eq!(r.note, ReviewerNote::GhUnavailable);
    assert_eq!(r.results.iter().map(|x| x.login.as_str()).collect::<Vec<_>>(), ["leo", ""]);
}

#[test]
fn a_failing_member_list_is_reported_not_raised() {
    let d = dir(Ok(vec![]), Err("gh: HTTP 401: Bad credentials".into()));
    let r = ReviewerResolver::new(d, CACHE_FOR).resolve("acme", "Leo");
    assert_eq!((r.note, r.org.as_str(), r.detail.as_str()), (ReviewerNote::MembersFailed, "acme", "gh: HTTP 401: Bad credentials"));
    assert_eq!(r.results[0].status, ReviewerStatus::None);
}

#[test]
fn the_member_list_is_read_once_while_fresh() {
    let d = dir(Ok(vec![]), Ok(vec![leo()]));
    let res = ReviewerResolver::new(d.clone(), CACHE_FOR);
    res.resolve("acme", "Leo");
    res.resolve("ACME", "leo-mxyz");
    assert_eq!(d.member_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn empty_input_asks_github_nothing() {
    let d = dir(Err("darf nicht gefragt werden".into()), Err("nein".into()));
    let r = ReviewerResolver::new(d.clone(), CACHE_FOR).resolve("", " , ");
    assert_eq!((r.note, r.results.len()), (ReviewerNote::None, 0));
    assert_eq!(d.member_calls.load(Ordering::SeqCst), 0);
}
