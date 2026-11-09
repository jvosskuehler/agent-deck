//! Die rechnenden Teile der Fenster-Bindung: was ist ein bindbarer Workspace, wie fällt
//! sein Name aus dem Fenstertitel, und wie wird aus einer Ticket-ID ein Slug, ein
//! Branch und ein Jira-Key. Die Persistenz (bindings.json & Co.) steht in
//! `bind_store.rs`.

use std::sync::LazyLock;

use regex::Regex;

use crate::config::{JIRA_PROJECT_KEY, TICKET_BRANCH_PREFIX, VSCODE_MARKER};
use crate::pystr::strip;

/// „Kein echtes Projekt“ → nicht binden (sonst Phantom-Kachel): leer und das reservierte
/// `unknown`, das alte Extensions für ein Fenster ohne Ordner meldeten. Preis: ein
/// echter Ordner namens `unknown` wird nicht automatisch verfolgt.
pub fn is_placeholder_ws(ws: Option<&str>) -> bool {
    match ws {
        None => true,
        Some(s) => {
            let low = strip(s).to_lowercase();
            low.is_empty() || low == "unknown"
        }
    }
}

/// Remote-Anhängsel im Fenstertitel („agent-deck [SSH: 10.8.0.10]“). Bewusst ENG: nur
/// die bekannten Marker, damit ein Ordner „release [alt]“ seinen Namen behält.
static REMOTE_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\s*\[(?:SSH|WSL|Dev Container|Container|Codespaces|Remote|Tunnel|Attached Container)\b[^\]]*\]\n?$",
    )
    .expect("REMOTE_TAG")
});

/// `agent-deck [SSH: host]` → `agent-deck`. Ohne Marker unverändert.
pub fn strip_remote_tag(name: &str) -> String {
    strip(&REMOTE_TAG.replace(name, "")).to_string()
}

/// Aus `datei - repo - Visual Studio Code` den Namen ziehen, unter dem das Panel dieses
/// Fenster führt: Ordnername MIT Remote-Anhängsel, genau wie die Extension ihn meldet.
pub fn window_from_title(title: &str) -> String {
    let t = title.replace('●', " ");
    let t = strip(&t);
    let parts: Vec<&str> = t.split(" - ").map(strip).filter(|p| !p.is_empty()).collect();
    if let Some(i) = parts.iter().position(|p| *p == VSCODE_MARKER)
        && i >= 1
    {
        return parts[i - 1].to_string();
    }
    parts.last().map(|p| p.to_string()).unwrap_or_default()
}

/// Wie `window_from_title`, aber ohne Remote-Anhängsel: diese Form gehört in jeden
/// Abgleich Titel ↔ gebundener Name.
pub fn repo_from_title(title: &str) -> String {
    strip_remote_tag(&window_from_title(title))
}

/// Ticket-ID → ordner- und branchtauglicher Slug: klein, nur Buchstaben und Ziffern,
/// alles andere zu einem `-` zusammengefasst, Ränder getrimmt.
/// `ABC-123: Login fix` → `abc-123-login-fix`.
pub fn ticket_slug(ticket: &str) -> String {
    let lower = strip(ticket).to_lowercase();
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
    let t = strip(ticket);
    if t.is_empty() {
        return String::new();
    }
    if let Some(c) = KEY_FORM.captures(t) {
        return format!("{}-{}", c[1].to_uppercase(), &c[2]);
    }
    let num = strip(t.trim_start_matches('#'));
    let proj = strip(project.unwrap_or(JIRA_PROJECT_KEY)).to_uppercase();
    if !proj.is_empty() && !num.is_empty() && num.chars().all(char::is_numeric) {
        return format!("{proj}-{num}");
    }
    t.to_string()
}
