//! Die rechnenden Teile rund um Block und Ticket: wie wird aus einer Ticket-ID ein Slug,
//! ein Branch und ein Jira-Key. Die Persistenz der Bindung (bindings.json & Co.) steht
//! in `bind_store.rs`.

use std::sync::LazyLock;

use regex::Regex;

use crate::config::{JIRA_PROJECT_KEY, TICKET_BRANCH_PREFIX};

/// Ticket-ID → ordner- und branchtauglicher Slug: klein, nur Buchstaben und Ziffern,
/// alles andere zu einem `-` zusammengefasst, Ränder getrimmt.
/// `ABC-123: Login fix` → `abc-123-login-fix`.
pub fn ticket_slug(ticket: &str) -> String {
    let lower = ticket.trim().to_lowercase();
    let mut slug = String::with_capacity(lower.len());
    for ch in lower.chars() {
        let ch = if ch.is_alphanumeric() { ch } else { '-' };
        if !(ch == '-' && slug.ends_with('-')) {
            slug.push(ch);
        }
    }
    slug.trim_matches('-').to_string()
}

/// Branch aus Ticket-ID: Präfix + Slug (`ticket/abc-123`). Leerer Slug → leer.
pub fn ticket_branch(ticket: &str, prefix: Option<&str>) -> String {
    let slug = ticket_slug(ticket);
    if slug.is_empty() {
        return String::new();
    }
    format!("{}{}", prefix.unwrap_or(TICKET_BRANCH_PREFIX), slug)
}

static KEY_FORM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([A-Za-z][A-Za-z0-9]*)-(\d+)$").expect("KEY_FORM"));

/// Aus der eingegebenen Ticket-ID einen Jira-Key machen. Schon ein Key (`proj-42`) →
/// Projektteil groß; nur eine Nummer (auch mit `#`) → Projekt davor (ohne Projekt bleibt
/// sie roh); sonst unverändert. Leer → leer.
pub fn jira_key(ticket: &str, project: Option<&str>) -> String {
    let t = ticket.trim();
    if t.is_empty() {
        return String::new();
    }
    if let Some(c) = KEY_FORM.captures(t) {
        return format!("{}-{}", c[1].to_uppercase(), &c[2]);
    }
    let num = t.trim_start_matches('#').trim();
    let proj = project.unwrap_or(JIRA_PROJECT_KEY).trim().to_uppercase();
    if !proj.is_empty() && !num.is_empty() && num.chars().all(char::is_numeric) {
        return format!("{proj}-{num}");
    }
    t.to_string()
}
