//! Lagebild eines Agenten-Chats: die reinen Teile.
//!
//! Wer viele Agenten betreut, kennt das Thema eines Chats schon aus Titel und Ticket.
//! Was beim Zurückkommen fehlt, ist der Stand. Das Lagebild hat darum vier feste Zeilen:
//! Ziel, Stand, „Wartet auf dich“ und nächster Schritt. Die vier Felder sind das, worauf
//! Übergabe-Vorlagen für Agenten hinauslaufen; die eigene Zeile für die offene Frage
//! gibt es, weil genau sie entscheidet, ob man hin muss.
//!
//! Eingabe ist ein Auszug des Transcripts (`build_digest`): jede getippte Nachricht (sie
//! trägt die Absicht), die jüngsten Antworten ausführlich, ältere nur angerissen, die
//! Werkzeugaufrufe verdichtet. Dazu das bisherige Lagebild, damit das Modell
//! fortschreibt, statt jedes Mal neu zu raten. Wann gefragt wird, steht in
//! `summary_cache.rs`.

use serde_json::{Value, json};

use crate::claude::summarize::turn_of;
use crate::text::{char_len, squash_ws, take_chars};

/// Stand des Formats. Hochzählen, wenn sich Prompt oder Felder ändern: dann wird jedes
/// Lagebild einmal neu erzeugt (auch die Ein-Satz-Zusammenfassungen von vor Nov. 2026).
pub const BRIEF_VERSION: u64 = 1;

const INSTRUCTION_DE: &str = "Du bekommst den Verlauf einer Coding-Session zwischen einem Entwickler und einem KI-Agenten, eventuell dazu das bisherige Lagebild. Der Entwickler betreut viele Agenten gleichzeitig und kommt nach einer Pause zu diesem zurück. Schreib ihm auf Deutsch das Lagebild so, dass er ohne Nachlesen weitermachen kann.\n\nGenau vier Zeilen, jede beginnt mit ihrem Etikett, jede höchstens 30 Wörter:\nZiel: was der Entwickler erreichen will, in seinen Begriffen.\nStand: was erledigt ist. Sag dazu, ob es geprüft ist (Test, Build, Messung) oder nur behauptet.\nWartet auf dich: die offene Frage oder Entscheidung, so konkret, dass er antworten kann. Sonst das eine Wort: nichts\nNächster Schritt: was als Nächstes passiert und wer es tut.\n\nNenn Dateien, Tickets und Zahlen beim Namen. Gibt es ein bisheriges Lagebild, schreib es fort: übernimm, was noch stimmt, ersetze, was überholt ist. Keine Einleitung, keine Wertung, kein Markdown, nichts erfinden: was nicht im Verlauf steht, lässt du weg. Benutze KEINE Tools.\n\n";

const INSTRUCTION_EN: &str = "You get the history of a coding session between a developer and an AI agent, possibly with the previous briefing. The developer looks after many agents at once and returns to this one after a break. Write the briefing in English so that they can carry on without rereading.\n\nExactly four lines, each starting with its label, each at most 30 words:\nGoal: what the developer wants to achieve, in their terms.\nStatus: what is done. Say whether it is verified (test, build, measurement) or only claimed.\nWaiting on you: the open question or decision, concrete enough to answer. Otherwise the single word: nothing\nNext step: what happens next and who does it.\n\nName files, tickets and numbers. If there is a previous briefing, update it: keep what still holds, replace what is outdated. No preamble, no judgement, no Markdown, invent nothing: leave out what is not in the history. Do NOT use any tools.\n\n";

/// Trennzeile vor dem Auszug, in beiden Sprachen gleich.
pub const CHAT_MARK: &str = "--- Chat ---\n";

/// Etiketten der vier Zeilen je Sprache: (Ziel, Stand, Wartet auf dich, Nächster Schritt).
fn labels(lang: &str) -> [&'static str; 4] {
    if lang == "english" { ["Goal", "Status", "Waiting on you", "Next step"] } else { ["Ziel", "Stand", "Wartet auf dich", "Nächster Schritt"] }
}

/// Anweisung an das Modell; `english` → englisch, alles andere deutsch.
pub fn instruction(lang: &str) -> &'static str {
    if lang == "english" { INSTRUCTION_EN } else { INSTRUCTION_DE }
}

/// Das Lagebild, je Feld eine Zeile Text ohne Etikett.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Briefing {
    pub goal: String,
    pub state: String,
    pub waiting: String,
    pub next: String,
}

/// Länger wird kein Feld (Zeichen): das Modell soll bei 30 Wörtern bleiben, hält es sich
/// nicht daran, sprengt es wenigstens Tooltip und Tafel nicht.
const FIELD_MAX: usize = 400;

impl Briefing {
    /// „Wartet auf dich: nichts“ in allen Schreibweisen, die ein Modell dafür findet.
    pub fn waits(&self) -> bool {
        let w = self.waiting.trim().trim_end_matches(['.', '!']).to_lowercase();
        !matches!(w.as_str(), "" | "nichts" | "nothing" | "-" | "–" | "keine" | "none" | "n/a")
    }

    /// Kurzform für den Hover: das Ziel, darunter die offene Frage, wenn es eine gibt.
    pub fn short(&self, lang: &str) -> String {
        if self.waits() { format!("{}\n{}: {}", self.goal, labels(lang)[2], self.waiting) } else { self.goal.clone() }
    }

    /// Die vier Zeilen mit Etikett, wie das Modell sie schreibt (für das Fortschreiben).
    pub fn lines(&self, lang: &str) -> String {
        let l = labels(lang);
        format!("{}: {}\n{}: {}\n{}: {}\n{}: {}", l[0], self.goal, l[1], self.state, l[2], self.waiting, l[3], self.next)
    }

    pub fn to_json(&self) -> Value {
        json!({"goal": self.goal, "state": self.state, "waiting": self.waiting, "next": self.next})
    }

    pub fn from_json(v: &Value) -> Option<Briefing> {
        let f = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
        let b = Briefing { goal: f("goal"), state: f("state"), waiting: f("waiting"), next: f("next") };
        (!b.goal.is_empty() && !b.state.is_empty()).then_some(b)
    }
}

/// Etiketten beider Sprachen, kleingeschrieben, je Feld (auch ohne Umlaut).
const KNOWN: [&[&str]; 4] =
    [&["ziel", "goal"], &["stand", "status"], &["wartet auf dich", "waiting on you"], &["nächster schritt", "naechster schritt", "next step"]];

/// Antwort des Modells → Lagebild. Etiketten in beiden Sprachen, Aufzählungszeichen und
/// Fettdruck davor stören nicht; Zeilen ohne Etikett hängen am Feld darüber. `None`,
/// wenn Ziel oder Stand fehlen: dann hat das Modell das Format verfehlt, und der alte
/// Stand bleibt besser stehen.
pub fn parse(raw: &str) -> Option<Briefing> {
    let mut fields: [String; 4] = Default::default();
    let mut cur: Option<usize> = None;
    for line in raw.lines() {
        let clean = line.trim().trim_start_matches(['-', '*', '#', '•', ' ']);
        if clean.is_empty() {
            continue;
        }
        let lower = clean.to_lowercase();
        let hit = KNOWN.iter().enumerate().find_map(|(i, names)| {
            names.iter().find_map(|n| {
                let rest: String = lower.strip_prefix(n)?.trim_start_matches(['*', ' ']).to_string();
                rest.starts_with(':').then(|| (i, char_len(clean) - char_len(&rest) + 1))
            })
        });
        match hit {
            Some((i, skip)) => {
                let text: String = clean.chars().skip(skip).collect();
                fields[i] = text.trim().trim_start_matches(['*', ' ']).to_string();
                cur = Some(i);
            }
            None => {
                if let Some(i) = cur {
                    fields[i] = format!("{} {clean}", fields[i]).trim().to_string();
                }
            }
        }
    }
    let cap = |s: &String| {
        // Backticks um Dateinamen setzt das Modell trotz „kein Markdown“; gezeigt wird
        // reiner Text, dort wären sie nur Zeichenmüll.
        let s = squash_ws(&s.replace('`', ""));
        if char_len(&s) > FIELD_MAX { format!("{}…", take_chars(&s, FIELD_MAX).trim_end()) } else { s }
    };
    let b = Briefing { goal: cap(&fields[0]), state: cap(&fields[1]), waiting: cap(&fields[2]), next: cap(&fields[3]) };
    (!b.goal.is_empty() && !b.state.is_empty()).then_some(b)
}

/// Der ganze Prompt: Anweisung, bisheriges Lagebild (wenn es eines gibt), Auszug.
pub fn prompt(lang: &str, previous: Option<&Briefing>, digest: &str) -> String {
    let prev = match previous {
        Some(b) if lang == "english" => format!("Previous briefing:\n{}\n\n", b.lines(lang)),
        Some(b) => format!("Bisheriges Lagebild:\n{}\n\n", b.lines(lang)),
        None => String::new(),
    };
    format!("{}{prev}{CHAT_MARK}{digest}", instruction(lang))
}

enum Item {
    User(String),
    Agent(String),
    Tools(Vec<String>),
}

/// So lang darf eine getippte Nachricht im Auszug sein (Zeichen).
const USER_MAX: usize = 1500;
/// Die jüngsten Antworten ausführlich, ältere nur angerissen.
const AGENT_RECENT: usize = 6;
const AGENT_RECENT_MAX: usize = 2500;
const AGENT_OLD_MAX: usize = 240;
/// Werkzeugaufrufe zählen nur am Ende des Verlaufs: was vor zwanzig Zügen lief, steht
/// längst in einer Antwort.
const TOOLS_WITHIN: usize = 14;
const TOOLS_SHOWN: usize = 12;

/// Ein Werkzeugaufruf als Stichwort: Name und das Sprechendste aus seiner Eingabe.
fn tool_note(block: &Value) -> Option<String> {
    let name = block.get("name")?.as_str()?;
    let input = block.get("input");
    let pick = |k: &str| input.and_then(|i| i.get(k)).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
    let detail = match pick("file_path").or_else(|| pick("path")) {
        Some(p) => p.rsplit(['\\', '/']).next().unwrap_or(p).to_string(),
        None => ["description", "command", "pattern", "url", "prompt"]
            .iter()
            .find_map(|k| pick(k))
            .map(|d| take_chars(d.lines().next().unwrap_or(""), 70).to_string())
            .unwrap_or_default(),
    };
    Some(if detail.is_empty() { name.to_string() } else { format!("{name} {detail}") })
}

fn items(records: &[Value]) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    for rec in records {
        if rec.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        match turn_of(rec) {
            Some((role, text)) if role == "user" => out.push(Item::User(text)),
            Some((_, text)) => out.push(Item::Agent(text)),
            None => {}
        }
        if rec.get("type").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let blocks = rec.get("message").and_then(|m| m.get("content")).and_then(Value::as_array);
        let notes: Vec<String> = blocks
            .into_iter()
            .flatten()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_use"))
            .filter_map(tool_note)
            .collect();
        if notes.is_empty() {
            continue;
        }
        match out.last_mut() {
            Some(Item::Tools(t)) => t.extend(notes),
            _ => out.push(Item::Tools(notes)),
        }
    }
    out
}

fn capped(prefix: &str, text: &str, max: usize) -> String {
    let t = squash_ws(text);
    if char_len(&t) > max { format!("{prefix}{}…", take_chars(&t, max).trim_end()) } else { format!("{prefix}{t}") }
}

/// Auszug des Transcripts für das Lagebild. Die erste getippte Nachricht bleibt immer
/// (sie setzt das Ziel), danach wird vom Ende her gefüllt, bis `max_chars` voll ist; eine
/// Lücke steht als `…` da. Leer, wenn noch niemand etwas gesagt hat.
pub fn build_digest(records: &[Value], max_chars: usize) -> String {
    let items = items(records);
    let n = items.len();
    let mut agents_after = 0usize;
    // Von hinten: jede Zeile mit ihrer Position, damit „jüngste“ bekannt ist.
    let mut lines: Vec<(usize, String)> = Vec::new();
    for (i, item) in items.iter().enumerate().rev() {
        let line = match item {
            Item::User(t) => capped("User: ", t, USER_MAX),
            Item::Agent(t) => {
                agents_after += 1;
                capped("Assistant: ", t, if agents_after <= AGENT_RECENT { AGENT_RECENT_MAX } else { AGENT_OLD_MAX })
            }
            Item::Tools(t) if n - i <= TOOLS_WITHIN => {
                let more = t.len().saturating_sub(TOOLS_SHOWN);
                let shown = t[more..].join("; ");
                if more > 0 { format!("Tools: (+{more}) {shown}") } else { format!("Tools: {shown}") }
            }
            Item::Tools(_) => continue,
        };
        lines.push((i, line));
    }
    lines.reverse();
    let Some(first) = lines.iter().position(|(i, _)| matches!(items[*i], Item::User(_) | Item::Agent(_))) else {
        return String::new();
    };
    let head = lines[first].1.clone();
    let mut total = char_len(&head);
    let mut tail: Vec<&str> = Vec::new();
    for (_, line) in lines[first + 1..].iter().rev() {
        let len = char_len(line) + 1;
        if total + len > max_chars {
            break;
        }
        tail.push(line);
        total += len;
    }
    tail.reverse();
    let mut out = vec![head.as_str()];
    if tail.len() < lines.len() - first - 1 {
        out.push("…");
    }
    out.extend(tail);
    out.join("\n")
}
