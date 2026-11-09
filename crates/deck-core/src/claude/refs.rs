//! Ticket- und PR-Nummer aus einem Chat lesen: reine Regex, kein Modell.
//!
//! Das Problem sind nicht die Treffer, sondern die Falsch-Positiven: eine Zahl mit
//! Raute ist noch kein Pull Request, ein Wort mit Bindestrich noch kein Jira-Key. Jeder
//! Fund sammelt Punkte (Kontextwörter, URL-Form, Wiederholung) und muss eine Schwelle
//! reißen.
//!
//! Die Python-Muster benutzen Lookbehind und Lookahead, die die `regex`-Crate nicht
//! kennt. `find_with` prüft sie darum von Hand an den Rändern jedes Treffers und sucht
//! bei einem Fehlschlag ab dem nächsten Zeichen weiter, genau wie Pythons Engine. Das
//! ist für diese Muster gleichwertig: ob der Lookahead hält, hängt nur am Ziffernlauf
//! und nicht am gewählten Pfad.

use std::sync::LazyLock;

use regex::{Captures, Regex};

use crate::text::chars_before;

/// Ein Gesprächszug: (Rolle, Text). Rolle `user` zählt doppelt.
pub type Turn = (String, String);

static KEY_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([A-Z][A-Z0-9]{1,9})-(\d{1,6})").expect("KEY_RE"));

/// Technik-Kürzel, die wie ein Jira-Key aussehen (UTF-8, SHA-256, CVE-2021 …). Ein
/// Treffer auf das konfigurierte Projekt wird nie gefiltert.
const KEY_STOP: &[&str] = &[
    "UTF", "UTF8", "ISO", "RFC", "CVE", "CWE", "SHA", "MD5", "AES", "RSA", "TLS", "SSL", "HTTP", "HTTPS", "IP", "IPV",
    "EN", "DIN", "IEC", "IEEE", "ANSI", "ASCII", "CP", "WIN", "MACOS", "OSX", "GPT", "LLAMA", "NODE", "PY", "ES", "PEP",
    "PHP", "NET", "UUID", "URL", "URI", "SQL", "XML", "HTML", "CSS", "JSON", "YAML", "TOML", "CSV", "PDF", "PNG", "JPG",
    "JPEG", "SVG", "GIF", "MP3", "MP4", "RGB", "RGBA", "ARGB", "HSL", "HSV", "CMYK", "DPI", "PPI", "PX", "EM", "REM",
    "VW", "VH", "FPS", "HZ", "KHZ", "MHZ", "GHZ", "KB", "MB", "GB", "TB", "MS", "NS", "SEC", "MIN", "UTC", "GMT", "CET",
    "CEST", "CPU", "GPU", "RAM", "SSD", "HDD", "USB", "PCI", "SATA", "LTS", "EOL", "EOF", "LF", "CRLF", "BOM", "REV",
    "VER", "V", "N", "X", "Y", "Z", "K", "M", "PART", "STEP", "PHASE", "TIER", "TOP", "LEVEL", "COVID", "SARS", "RC",
];

/// Wörter, nach denen eine bloße Nummer als Ticket zählt bzw. die einen Key-Treffer
/// glaubwürdiger machen.
const CTX_WORDS: [&str; 7] = ["ticket", "issue", "jira", "bug", "story", "vorgang", "karte"];

static NUM_CTX_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"(?i)(?:{})s?\b[\s:#/-]*#?(\d{{2,6}})", CTX_WORDS.join("|"))).expect("NUM_CTX_RE")
});

static PR_URL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:github\.com/[^\s/]+/[^\s/]+/pull/|/-/merge_requests/)(\d{1,6})").expect("PR_URL_RE")
});
static PR_CTX_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:pull[\s_-]?requests?|merge[\s_-]?requests?|prs?|mrs?)\b[\s:#/_-]*#?(\d{1,6})")
        .expect("PR_CTX_RE")
});
static HASH_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"#(\d{1,6})").expect("HASH_RE"));

/// Vor einem bloßen `#42` heißt das: KEIN Pull Request.
const HASH_NOT_PR: [&str; 11] =
    ["issue", "ticket", "jira", "zeile", "line", "seite", "page", "kommentar", "comment", "spalte", "column"];

fn key_boundary(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// Alle Treffer von `re` in `text`, bei denen `before(zeichen davor)` und
/// `after(zeichen danach)` halten (`None` = Textrand). Ersetzt `(?<!…)` und `(?!…)`.
fn find_with<'t>(
    re: &Regex,
    text: &'t str,
    before: impl Fn(Option<char>) -> bool,
    after: impl Fn(Option<char>) -> bool,
) -> Vec<Captures<'t>> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos <= text.len() {
        let Some(c) = re.captures_at(text, pos) else { break };
        let m = c.get(0).expect("Gesamttreffer");
        let prev = text[..m.start()].chars().next_back();
        let next = text[m.end()..].chars().next();
        if before(prev) && after(next) {
            pos = m.end();
            out.push(c);
        } else {
            pos = m.start() + text[m.start()..].chars().next().map_or(1, char::len_utf8);
        }
    }
    out
}

static DIGIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d$").expect("DIGIT"));

/// `\d` in Python: Unicode-Dezimalziffer (nicht jede Zahl wie `²`).
fn is_decimal(c: char) -> bool {
    c.is_ascii_digit() || (!c.is_ascii() && DIGIT.is_match(c.encode_utf8(&mut [0; 4])))
}

/// `(?!\d)`.
fn no_digit(c: Option<char>) -> bool {
    !c.is_some_and(is_decimal)
}

fn lower_before(text: &str, pos: usize) -> String {
    chars_before(text, pos, 24).to_lowercase()
}

/// Steht kurz VOR der Fundstelle ein Wort wie „Ticket“? → Bonuspunkte.
fn ctx_bonus(text: &str, pos: usize) -> i64 {
    let before = lower_before(text, pos);
    if CTX_WORDS.iter().any(|w| before.contains(w)) { 2 } else { 0 }
}

/// Punkte je Kandidat in Einfüge-Reihenfolge: [punkte, letzter Zug-Index].
#[derive(Default)]
struct Hits(Vec<(String, i64, usize)>);

impl Hits {
    fn bump(&mut self, key: String, weight: i64, idx: usize) {
        match self.0.iter_mut().find(|(k, _, _)| *k == key) {
            Some(rec) => {
                rec.1 += weight;
                rec.2 = idx;
            }
            None => self.0.push((key, weight, idx)),
        }
    }

    /// Meiste Punkte, bei Gleichstand der zuletzt erwähnte (bei völligem Gleichstand
    /// der zuerst gefundene, wie Pythons `max`). Unter `min_score` → leer.
    fn best(self, min_score: i64) -> String {
        let mut best: Option<(String, i64, usize)> = None;
        for h in self.0 {
            if best.as_ref().is_none_or(|b| (h.1, h.2) > (b.1, b.2)) {
                best = Some(h);
            }
        }
        best.filter(|b| b.1 >= min_score).map(|b| b.0).unwrap_or_default()
    }
}

/// Züge mit Text; `user` wiegt 2, alles andere 1.
fn iter_turns(turns: &[Turn]) -> impl Iterator<Item = (&str, i64, usize)> {
    turns
        .iter()
        .enumerate()
        .filter(|(_, (_, t))| !t.is_empty())
        .map(|(i, (role, text))| (text.as_str(), if role == "user" { 2 } else { 1 }, i))
}

/// Die Ticket-ID, um die es im Gespräch geht (leer = keine). Gewertet nach Häufigkeit:
/// was DU sagst zählt doppelt, ein Treffer auf das konfigurierte Projekt dreifach, ein
/// „Ticket …“ davor gibt Bonus, bei Gleichstand gewinnt die zuletzt erwähnte.
/// `min_score` hält einmalige Nebenbei-Nennungen des Agenten draußen.
pub fn find_ticket(turns: &[Turn], project: Option<&str>, min_score: i64) -> String {
    let proj = project.unwrap_or("").trim().to_uppercase();
    let proj_re = (!proj.is_empty())
        .then(|| Regex::new(&format!(r"(?i){}-(\d{{1,6}})", regex::escape(&proj))).expect("proj_re"));
    let mut hits = Hits::default();
    for (text, w, idx) in iter_turns(turns) {
        for c in find_with(&KEY_RE, text, |p| !p.is_some_and(key_boundary), no_digit) {
            let (pre, num) = (&c[1], &c[2]);
            if pre == proj || KEY_STOP.contains(&pre) {
                continue;
            }
            let ctx = ctx_bonus(text, c.get(0).expect("Treffer").start());
            if num.chars().count() < 2 && ctx == 0 {
                continue; // einstellig nur mit „Ticket …“ davor (sonst UTF-8 & Co.)
            }
            hits.bump(format!("{pre}-{num}"), w + ctx, idx);
        }
        if let Some(re) = &proj_re {
            for c in find_with(re, text, |p| !p.is_some_and(key_boundary), no_digit) {
                hits.bump(format!("{proj}-{}", &c[1]), w * 3, idx);
            }
            for c in find_with(&NUM_CTX_RE, text, |_| true, no_digit) {
                hits.bump(format!("{proj}-{}", &c[1]), w, idx);
            }
        }
    }
    hits.best(min_score)
}

/// Die Pull-/Merge-Request-Nummer als reine Zahl (leer = keine). Härtere Schwelle als
/// beim Ticket: ein EINZELNES `#62` reicht nicht, „PR #62“ vom Nutzer oder eine PR-URL
/// schon.
pub fn find_pr(turns: &[Turn], min_score: i64) -> String {
    let mut hits = Hits::default();
    for (text, w, idx) in iter_turns(turns) {
        for c in find_with(&PR_URL_RE, text, |_| true, no_digit) {
            hits.bump(c[1].to_string(), w * 3, idx);
        }
        for c in find_with(&PR_CTX_RE, text, |_| true, no_digit) {
            hits.bump(c[1].to_string(), w * 2, idx);
        }
        let hash_before = |p: Option<char>| !p.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '#');
        let hash_after = |n: Option<char>| !n.is_some_and(|c| c.is_ascii_alphanumeric());
        for c in find_with(&HASH_RE, text, hash_before, hash_after) {
            let before = lower_before(text, c.get(0).expect("Treffer").start());
            if HASH_NOT_PR.iter().any(|w| before.contains(w)) {
                continue; // „Issue #42“ / „Zeile #42“ ist kein PR
            }
            hits.bump(c[1].to_string(), w, idx);
        }
    }
    hits.best(min_score)
}

/// Ticket UND PR in einem Durchgang. Beides kann gleichzeitig gelten.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Refs {
    pub ticket: String,
    pub pr: String,
}

pub fn find_refs(turns: &[Turn], project: Option<&str>) -> Refs {
    Refs { ticket: find_ticket(turns, project, 2), pr: find_pr(turns, 3) }
}

/// Bezugszeile für den Tooltip („Ticket: ABC-1 · PR #62“); ohne beides leer.
pub fn refs_label(refs: &Refs) -> String {
    let mut parts = Vec::new();
    if !refs.ticket.is_empty() {
        parts.push(format!("Ticket: {}", refs.ticket));
    }
    if !refs.pr.is_empty() {
        parts.push(format!("PR #{}", refs.pr));
    }
    parts.join(" · ")
}

/// Kompakte Fassung für die KARTE („PROJ-2691 #62“), nur solange beides in `max_chars`
/// passt; sonst gewinnt das Ticket, weil es das Dauerhaftere ist.
pub fn refs_card_label(refs: &Refs, max_chars: usize) -> String {
    let pr = if refs.pr.is_empty() { String::new() } else { format!("#{}", refs.pr) };
    let both = [refs.ticket.as_str(), pr.as_str()].iter().filter(|p| !p.is_empty()).copied().collect::<Vec<_>>().join(" ");
    if both.chars().count() <= max_chars {
        return both;
    }
    if !refs.ticket.is_empty() {
        return refs.ticket.clone();
    }
    format!("{}…", crate::text::take_chars(&both, max_chars.saturating_sub(1)))
}
