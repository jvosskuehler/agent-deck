//! Reviewer der PR-Automatik per Namen statt per GitHub-Login (`pr_reviewer`).
//!
//! Wer das Deck einrichtet, tippt „Leo, Anna Schmidt“ oder eine Mailadresse; das Deck
//! gleicht das mit den Mitgliedern der GitHub-Organisation ab ([`reviewer_match`]) und
//! speichert die Logins. Gebraucht vom Installer (über `deck-setup resolve-reviewers`, als
//! JSON) und vom Einstellungsfenster (als [`ReviewerSave`]). Hier steht nur das Rechnen:
//! Eingabe zerlegen, Bericht bauen, Organisation wählen, Antwort von GitHub lesen. Den
//! Aufruf von `gh` macht `deck_services::reviewers`.
//!
//! [`reviewer_match`]: super::reviewer_match

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use ts_rs::TS;

pub use super::reviewer_match::{Match, Member, fold, resolve, valid_login};

/// Wie eine Eingabe ausgegangen ist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ReviewerStatus {
    /// Genau ein Mitglied passt.
    Unique,
    /// Mehrere passen: der Mensch wählt.
    Ambiguous,
    /// Keins passt (oder es gab keine Mitgliederliste).
    None,
}

/// Login und Name eines Mitglieds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReviewerPerson {
    pub login: String,
    pub name: String,
}

/// Ergebnis je Eingabe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReviewerResult {
    /// Was getippt wurde (ein Eintrag der Liste).
    pub input: String,
    pub status: ReviewerStatus,
    /// Eindeutig: der Login. Keiner gefunden: die Eingabe selbst, wenn sie als Login taugt
    /// (wird dann ungeprüft übernommen), sonst leer (wird übersprungen).
    pub login: String,
    /// Eindeutig: der Anzeigename (kann leer sein).
    pub name: String,
    /// Mehrdeutig: die Kandidaten.
    pub candidates: Vec<ReviewerPerson>,
}

/// Was bei der Suche fehlte. Ein fester Code, Texte bauen Installer und Oberfläche.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ReviewerNote {
    /// Alles da.
    None,
    /// Mehrere Organisationen und keine eingestellt: `orgs` zeigt die Wahl.
    ChooseOrg,
    /// gh kennt keine Organisation dieses Kontos.
    NoOrg,
    /// gh fehlt, ist nicht angemeldet oder antwortet nicht.
    GhUnavailable,
    /// Die Mitglieder der Organisation ließen sich nicht lesen.
    MembersFailed,
}

/// Der ganze Bericht (`deck-setup resolve-reviewers` druckt ihn als JSON).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReviewerReport {
    /// Organisation, gegen die verglichen wurde (eingestellt oder die eine des Kontos);
    /// leer = keine. Ist sie nicht eingestellt, speichert der Aufrufer sie als `github_org`.
    pub org: String,
    /// Bei [`ReviewerNote::ChooseOrg`]: die Organisationen zur Wahl.
    pub orgs: Vec<String>,
    pub note: ReviewerNote,
    /// Kurzer Fehlertext zu `note` (ohne Geheimnisse), sonst leer.
    pub detail: String,
    pub results: Vec<ReviewerResult>,
}

/// Antwort des Einstellungsfensters auf `set_reviewers`: gespeicherter Stand und Bericht.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReviewerSave {
    pub settings: crate::view::SettingsView,
    pub report: ReviewerReport,
}

/// Die Eingabe als Liste. Komma, Semikolon und Zeilenumbruch trennen (Leerzeichen nicht:
/// „Anna Schmidt“ ist ein Name). Ein führendes `@` fällt weg. Steht ein Eintrag als
/// „Name (login)“ da, so wie das Fenster ihn zeigt, zählt der Login in der Klammer.
pub fn split_input(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in raw.split([',', ';', '\n', '\r']) {
        let mut t = part.trim().trim_matches('"').trim();
        if let Some(open) = t.rfind('(')
            && t.ends_with(')')
        {
            let inner = t[open + 1..t.len() - 1].trim();
            if valid_login(inner.trim_start_matches('@')) {
                t = inner;
            }
        }
        let t = t.trim_start_matches('@').trim();
        if !t.is_empty() && !out.iter().any(|o| o.eq_ignore_ascii_case(t)) {
            out.push(t.to_string());
        }
    }
    out
}

fn person(m: &Member) -> ReviewerPerson {
    ReviewerPerson { login: m.login.clone(), name: m.name.clone() }
}

fn unresolved(input: &str) -> ReviewerResult {
    let login = if valid_login(input) { input.to_string() } else { String::new() };
    ReviewerResult { input: input.into(), status: ReviewerStatus::None, login, name: String::new(), candidates: Vec::new() }
}

fn result(input: &str, m: Match) -> ReviewerResult {
    match m {
        Match::Unique(m) => ReviewerResult {
            input: input.into(),
            status: ReviewerStatus::Unique,
            login: m.login.clone(),
            name: m.name.clone(),
            candidates: Vec::new(),
        },
        Match::Ambiguous(ms) => ReviewerResult {
            input: input.into(),
            status: ReviewerStatus::Ambiguous,
            login: String::new(),
            name: String::new(),
            candidates: ms.iter().map(person).collect(),
        },
        Match::None => unresolved(input),
    }
}

/// Ergebnisse für die ganze Eingabe. Ohne Mitgliederliste (`None`) bleibt jeder Eintrag
/// unaufgelöst. Ein Eintrag mit Leerzeichen, der als Ganzes nicht passt, dessen Wörter
/// aber je genau ein Mitglied treffen („leo anna“, alte Schreibweise der Login-Liste),
/// wird in diese Wörter geteilt.
pub fn resolve_all(members: Option<&[Member]>, raw: &str) -> Vec<ReviewerResult> {
    let Some(members) = members else {
        return split_input(raw).iter().map(|t| unresolved(t)).collect();
    };
    let mut out = Vec::new();
    for token in split_input(raw) {
        let m = resolve(members, &token);
        if m == Match::None && token.contains(char::is_whitespace) {
            let parts: Vec<(&str, Match)> = token.split_whitespace().map(|w| (w, resolve(members, w))).collect();
            if parts.iter().all(|(_, m)| matches!(m, Match::Unique(_))) {
                out.extend(parts.into_iter().map(|(w, m)| result(w, m)));
                continue;
            }
        }
        out.push(result(&token, m));
    }
    out
}

/// Wie die Organisation zustande kommt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrgPick {
    /// Gegen diese vergleichen.
    Use(String),
    /// Mehrere zur Wahl, keine eingestellt.
    Choose(Vec<String>),
    /// Keine Suche möglich.
    Without(ReviewerNote, String),
}

/// Eingestellt geht vor; sonst die Organisationen des Kontos: genau eine wird genommen,
/// mehrere muss der Mensch wählen, keine oder ein Fehler heißt: ohne Suche.
pub fn pick_org(setting: &str, mine: impl FnOnce() -> Result<Vec<String>, String>) -> OrgPick {
    let setting = setting.trim();
    if !setting.is_empty() {
        return OrgPick::Use(setting.to_string());
    }
    match mine() {
        Err(e) => OrgPick::Without(ReviewerNote::GhUnavailable, e),
        Ok(orgs) => match orgs.as_slice() {
            [] => OrgPick::Without(ReviewerNote::NoOrg, String::new()),
            [one] => OrgPick::Use(one.clone()),
            _ => OrgPick::Choose(orgs),
        },
    }
}

/// Die Logins, die gespeichert werden: eindeutige und unaufgelöste, die als Login taugen;
/// doppelte (ohne Groß/klein) einmal.
pub fn logins(results: &[ReviewerResult]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for r in results.iter().filter(|r| r.status != ReviewerStatus::Ambiguous && !r.login.is_empty()) {
        if !out.iter().any(|o| o.eq_ignore_ascii_case(&r.login)) {
            out.push(r.login.clone());
        }
    }
    out
}

/// Die gemerkten Anzeigenamen (`pr_reviewer_names`, Login → Name) für die neue Liste:
/// bekannte Namen der verbliebenen Logins bleiben, neue eindeutige kommen dazu.
pub fn names_after(old: Option<&Value>, results: &[ReviewerResult]) -> Map<String, Value> {
    let keep = logins(results);
    let mut out = Map::new();
    for login in &keep {
        let fresh = results.iter().find(|r| r.status == ReviewerStatus::Unique && r.login.eq_ignore_ascii_case(login) && !r.name.is_empty());
        let known = old.and_then(Value::as_object).and_then(|o| o.get(login)).and_then(Value::as_str).filter(|n| !n.is_empty());
        if let Some(name) = fresh.map(|r| r.name.as_str()).or(known) {
            out.insert(login.clone(), Value::String(name.to_string()));
        }
    }
    out
}

/// So zeigt das Fenster die Liste: „Leo Müller (leo-mxyz), anna“. Ohne bekannten Namen
/// steht nur der Login da.
pub fn label(logins_raw: &str, names: Option<&Value>) -> String {
    let names = names.and_then(Value::as_object);
    split_input(logins_raw)
        .iter()
        .map(|login| match names.and_then(|n| n.get(login)).and_then(Value::as_str).filter(|n| !n.trim().is_empty()) {
            Some(name) => format!("{} ({login})", name.trim()),
            None => login.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// GraphQL: eine Seite der Mitglieder. `full` mit den Adressen; die Firmenadressen
/// (`organizationVerifiedDomainEmails`) zeigt GitHub nicht jedem, darum gibt es eine
/// schlanke Fassung als Rückfall.
pub fn members_query(full: bool) -> &'static str {
    if full {
        "query($org:String!,$cursor:String){organization(login:$org){membersWithRole(first:100,after:$cursor){nodes{login name email organizationVerifiedDomainEmails(login:$org)} pageInfo{hasNextPage endCursor}}}}"
    } else {
        "query($org:String!,$cursor:String){organization(login:$org){membersWithRole(first:100,after:$cursor){nodes{login name} pageInfo{hasNextPage endCursor}}}}"
    }
}

/// Eine Seite lesen: Mitglieder und der Zeiger auf die nächste (`None` = letzte Seite).
/// Fehlt die Liste ganz, ist das ein Fehler mit GitHubs erster Meldung.
pub fn parse_members_page(v: &Value) -> Result<(Vec<Member>, Option<String>), String> {
    let Some(list) = v.pointer("/data/organization/membersWithRole") else {
        let msg = v.pointer("/errors/0/message").and_then(Value::as_str).unwrap_or("keine Mitgliederliste");
        return Err(msg.to_string());
    };
    let text = |n: &Value, k: &str| n.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
    let members = list
        .get("nodes")
        .and_then(Value::as_array)
        .map(|nodes| {
            nodes
                .iter()
                .filter(|n| !text(n, "login").is_empty())
                .map(|n| {
                    let mut emails: Vec<String> = n
                        .get("organizationVerifiedDomainEmails")
                        .and_then(Value::as_array)
                        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_lowercase).collect())
                        .unwrap_or_default();
                    let public = text(n, "email").to_lowercase();
                    if !public.is_empty() {
                        emails.push(public);
                    }
                    Member { login: text(n, "login"), name: text(n, "name"), emails }
                })
                .collect()
        })
        .unwrap_or_default();
    let next = list
        .pointer("/pageInfo/hasNextPage")
        .and_then(Value::as_bool)
        .filter(|more| *more)
        .and_then(|_| list.pointer("/pageInfo/endCursor").and_then(Value::as_str))
        .map(String::from);
    Ok((members, next))
}

#[cfg(test)]
#[path = "reviewers_tests.rs"]
mod tests;
