//! Tests zu `reviewers.rs` und `reviewer_match.rs`: Namen, Mailadressen und Logins
//! einem Mitglied der Organisation zuordnen.

use serde_json::json;

use super::*;

fn m(login: &str, name: &str) -> Member {
    Member { login: login.into(), name: name.into(), emails: Vec::new() }
}

fn org() -> Vec<Member> {
    vec![
        m("leo-mxyz", "Leo Müller"),
        m("lkrause", "Leon Krause"),
        m("anna-s", "Anna Schmidt"),
        m("aberger", "Anna-Lena Berger"),
        m("jvk", "Jorrit Voßkühler"),
        m("brandt-dev", "Tim Brandt"),
        m("noname42", ""),
        Member { login: "xy-dev".into(), name: "Xaver Yilmaz".into(), emails: vec!["x.yilmaz@firma.de".into()] },
    ]
}

fn login_of(r: Match) -> String {
    match r {
        Match::Unique(m) => m.login,
        other => panic!("nicht eindeutig: {other:?}"),
    }
}

fn candidates(r: Match) -> Vec<String> {
    match r {
        Match::Ambiguous(ms) => ms.into_iter().map(|m| m.login).collect(),
        other => panic!("nicht mehrdeutig: {other:?}"),
    }
}

#[test]
fn a_login_stays_that_login_even_with_at_and_other_case() {
    assert_eq!(login_of(resolve(&org(), "LEO-MXYZ")), "leo-mxyz");
    assert_eq!(login_of(resolve(&org(), "@noname42")), "noname42");
}

#[test]
fn an_exact_first_name_beats_a_longer_name_with_the_same_start() {
    // „Leo“ ist Leo Müllers Vorname genau, bei Leon nur der Anfang.
    assert_eq!(login_of(resolve(&org(), "Leo")), "leo-mxyz");
    assert_eq!(login_of(resolve(&org(), "leon")), "lkrause");
}

#[test]
fn umlauts_and_their_spellings_count_as_the_same_letter() {
    for q in ["Müller", "mueller", "MULLER", "Leo Mueller"] {
        assert_eq!(login_of(resolve(&org(), q)), "leo-mxyz", "{q}");
    }
    assert_eq!(login_of(resolve(&org(), "Vosskuehler")), "jvk");
    assert_eq!(login_of(resolve(&org(), "voßkühler")), "jvk");
}

#[test]
fn several_people_with_the_same_first_name_are_offered_for_choice() {
    assert_eq!(candidates(resolve(&org(), "Anna")), ["anna-s", "aberger"]);
    assert_eq!(login_of(resolve(&org(), "Anna Schmidt")), "anna-s");
    assert_eq!(login_of(resolve(&org(), "anna lena")), "aberger");
}

#[test]
fn a_word_start_finds_a_name_when_nothing_matches_exactly() {
    assert_eq!(login_of(resolve(&org(), "Krau")), "lkrause");
    assert_eq!(login_of(resolve(&org(), "Jor")), "jvk");
}

#[test]
fn nobody_matching_gives_none() {
    assert_eq!(resolve(&org(), "Zacharias"), Match::None);
    assert_eq!(resolve(&org(), "  "), Match::None);
}

#[test]
fn a_known_company_email_finds_its_member() {
    assert_eq!(login_of(resolve(&org(), "X.Yilmaz@firma.de")), "xy-dev");
}

#[test]
fn an_unknown_email_is_matched_by_its_local_part() {
    // Nachname allein, Initiale + Nachname mit und ohne Punkt, Vorname.Nachname.
    assert_eq!(login_of(resolve(&org(), "brandt@firma.de")), "brandt-dev");
    assert_eq!(login_of(resolve(&org(), "a.schmidt@firma.de")), "jvk");
    assert_eq!(login_of(resolve(&org(), "aschmidt@firma.de")), "jvk");
    assert_eq!(login_of(resolve(&org(), "anna.schmidt@firma.de")), "anna-s");
    assert_eq!(login_of(resolve(&org(), "lkrause@firma.de")), "lkrause");
    assert_eq!(resolve(&org(), "unbekannt@firma.de"), Match::None);
}

#[test]
fn logins_follow_githubs_rules() {
    assert!(valid_login("leo-mxyz"));
    assert!(valid_login("dependabot[bot]"));
    assert!(!valid_login("name@firma.de"));
    assert!(!valid_login("Anna Schmidt"));
    assert!(!valid_login("-leo"));
    assert!(!valid_login(&"a".repeat(40)));
}

#[test]
fn input_splits_on_commas_but_keeps_full_names_and_reads_shown_labels() {
    assert_eq!(split_input(" Leo, Anna Schmidt;@bob\n Leo Müller (leo-mxyz) ,, leo "), ["Leo", "Anna Schmidt", "bob", "leo-mxyz"]);
    // Eine Klammer, die kein Login ist, bleibt Teil des Namens.
    assert_eq!(split_input("Anna (Team Nord)"), ["Anna (Team Nord)"]);
}

#[test]
fn without_a_member_list_valid_logins_are_kept_and_the_rest_is_skipped() {
    let r = resolve_all(None, "leo, name@firma.de");
    assert_eq!(r.iter().map(|r| (r.status, r.login.as_str())).collect::<Vec<_>>(), [(ReviewerStatus::None, "leo"), (ReviewerStatus::None, "")]);
    assert_eq!(logins(&r), ["leo"]);
}

#[test]
fn a_space_separated_login_list_is_split_when_each_word_is_one_person() {
    let r = resolve_all(Some(&org()), "leo-mxyz lkrause");
    assert_eq!(logins(&r), ["leo-mxyz", "lkrause"]);
    // Ein echter Name bleibt ein Eintrag.
    assert_eq!(resolve_all(Some(&org()), "Tim Brandt").len(), 1);
}

#[test]
fn ambiguous_entries_are_not_saved_until_chosen() {
    let r = resolve_all(Some(&org()), "Anna, Leo, zacharias, nobody@y.de");
    assert_eq!(r[0].status, ReviewerStatus::Ambiguous);
    assert_eq!(r[0].candidates[1], ReviewerPerson { login: "aberger".into(), name: "Anna-Lena Berger".into() });
    assert_eq!(logins(&r), ["leo-mxyz", "zacharias"]);
}

#[test]
fn the_org_setting_wins_then_exactly_one_org_of_the_account() {
    let never = || -> Result<Vec<String>, String> { panic!("gh darf nicht gefragt werden") };
    assert_eq!(pick_org(" acme ", never), OrgPick::Use("acme".into()));
    assert_eq!(pick_org("", || Ok(vec!["acme".into()])), OrgPick::Use("acme".into()));
    assert_eq!(pick_org("", || Ok(vec!["a".into(), "b".into()])), OrgPick::Choose(vec!["a".into(), "b".into()]));
    assert_eq!(pick_org("", || Ok(vec![])), OrgPick::Without(ReviewerNote::NoOrg, String::new()));
    assert_eq!(pick_org("", || Err("gh: not logged in".into())), OrgPick::Without(ReviewerNote::GhUnavailable, "gh: not logged in".into()));
}

#[test]
fn names_are_kept_for_remaining_logins_and_shown_as_name_and_login() {
    let old = json!({"leo-mxyz": "Leo Müller", "gone": "Weg"});
    // leo-mxyz kommt diesmal ungeprüft (gh offline): der bekannte Name bleibt.
    let r = resolve_all(None, "leo-mxyz");
    let names = Value::Object(names_after(Some(&old), &r));
    assert_eq!(names, json!({"leo-mxyz": "Leo Müller"}));
    let r = resolve_all(Some(&org()), "leo-mxyz, Anna Schmidt, newbie");
    let names = Value::Object(names_after(Some(&old), &r));
    assert_eq!(names, json!({"leo-mxyz": "Leo Müller", "anna-s": "Anna Schmidt"}));
    assert_eq!(label("leo-mxyz, anna-s, newbie", Some(&names)), "Leo Müller (leo-mxyz), Anna Schmidt (anna-s), newbie");
    assert_eq!(label("", None), "");
}

#[test]
fn a_member_page_yields_logins_names_emails_and_the_next_cursor() {
    let v = json!({"data": {"organization": {"membersWithRole": {
        "nodes": [
            {"login": "leo-mxyz", "name": "Leo Müller", "email": "Leo@Example.org", "organizationVerifiedDomainEmails": ["leo@firma.de"]},
            {"login": "ghost", "name": null},
            {"login": ""}
        ],
        "pageInfo": {"hasNextPage": true, "endCursor": "Y3Vy"}
    }}}});
    let (members, next) = parse_members_page(&v).unwrap();
    assert_eq!(members, [
        Member { login: "leo-mxyz".into(), name: "Leo Müller".into(), emails: vec!["leo@firma.de".into(), "leo@example.org".into()] },
        m("ghost", ""),
    ]);
    assert_eq!(next.as_deref(), Some("Y3Vy"));
    let last = json!({"data": {"organization": {"membersWithRole": {"nodes": [], "pageInfo": {"hasNextPage": false, "endCursor": "x"}}}}});
    assert_eq!(parse_members_page(&last).unwrap().1, None);
}

#[test]
fn a_page_without_members_is_an_error_with_githubs_message() {
    let v = json!({"data": {"organization": null}, "errors": [{"message": "Could not resolve to an Organization"}]});
    assert_eq!(parse_members_page(&v).unwrap_err(), "Could not resolve to an Organization");
}

#[test]
fn the_report_serializes_in_the_shape_the_installer_reads() {
    let report = ReviewerReport {
        org: "acme".into(),
        orgs: vec![],
        note: ReviewerNote::None,
        detail: String::new(),
        results: resolve_all(Some(&org()), "Leo"),
    };
    let v = serde_json::to_value(&report).unwrap();
    assert_eq!(v["results"][0], json!({"input": "Leo", "status": "unique", "login": "leo-mxyz", "name": "Leo Müller", "candidates": []}));
    assert_eq!(v["note"], json!("none"));
}
