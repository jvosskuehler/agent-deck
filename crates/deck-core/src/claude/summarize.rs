//! KI-Kurzzusammenfassung eines Agenten-Chats für den Hover: die reinen Teile.
//!
//! Datenweg: der report-Hook merkt sich je Slot `session_id` und `cwd`; daraus findet
//! das Deck das Transcript `~/.claude/projects/<kodiertes cwd>/<session>.jsonl`,
//! `extract_turns` + `build_digest` ziehen einen kompakten Auszug (erster Zug = Thema,
//! jüngste Züge = aktueller Fokus), die App ruft `claude -p --safe-mode` und
//! `clean_summary` macht aus der Antwort eine Tooltip-Zeile. Cache und Entscheidungen,
//! wann neu erzeugt wird, stehen in `summary_cache.rs`.

use serde_json::Value;

use crate::claude::refs::Turn;
use crate::pystr::{char_len, rstrip, skip_chars, splitlines, squash_ws, strip, take_chars};

const INSTRUCTION_DE: &str = "Unten steht der Verlauf eines Coding-Chats zwischen einem Nutzer und einem KI-Agenten. Fasse in EINEM knappen deutschen Satz (hoechstens 12 Woerter) zusammen, WORUM es in dem Chat geht bzw. woran gerade gearbeitet wird - nur das Thema/die Aufgabe, keine Meta-Kommentare ueber den Chat selbst. Antworte ausschliesslich mit diesem einen Satz: ohne Anfuehrungszeichen, ohne Praefix, ohne Aufzaehlung, und benutze KEINE Tools.\n\n--- Chat ---\n";

const INSTRUCTION_EN: &str = "Below is the history of a coding chat between a user and an AI agent. Summarize in ONE concise English sentence (at most 12 words) WHAT the chat is about / what is currently being worked on - just the topic/task, no meta-commentary about the chat itself. Reply with only that single sentence: no quotation marks, no prefix, no bullet list, and do NOT use any tools.\n\n--- Chat ---\n";

/// Anweisung an das Modell; `english` → englisch, alles andere deutsch. Der Digest wird
/// direkt angehängt.
pub fn instruction(lang: &str) -> &'static str {
    if lang == "english" { INSTRUCTION_EN } else { INSTRUCTION_DE }
}

const QUOTES: &str = "\"'“”„»«‚‘’";

/// Ein Zug aus einem schon geparsten Transcript-Datensatz.
fn turn_of(rec: &Value) -> Option<Turn> {
    let msg = rec.get("message")?.as_object()?;
    let content = msg.get("content");
    match rec.get("type")?.as_str()? {
        "user" => {
            // Eine Liste ist ein Tool-Ergebnis, kein getippter Text.
            let text = strip(content?.as_str()?);
            if text.is_empty() || text.starts_with('<') || text.starts_with("Caveat:") {
                return None;
            }
            Some(("user".into(), text.into()))
        }
        "assistant" => {
            let parts: Vec<&str> = content?
                .as_array()?
                .iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .map(strip)
                .filter(|p| !p.is_empty())
                .collect();
            let text = strip(&parts.join("\n")).to_string();
            (!text.is_empty()).then(|| ("assistant".into(), text))
        }
        _ => None,
    }
}

/// Die echten Gesprächszüge aus den JSONL-Zeilen eines Transcripts: getippte
/// User-Nachrichten und die Text-Blöcke der Antworten. Tool-Aufrufe und -Ergebnisse,
/// Thinking, System-Zeilen und eingeschobene Slash-Kommando-/Caveat-Blöcke fallen weg.
pub fn extract_turns<S: AsRef<str>>(lines: &[S]) -> Vec<Turn> {
    lines
        .iter()
        .filter_map(|ln| {
            let s = strip(ln.as_ref());
            if s.is_empty() {
                return None;
            }
            serde_json::from_str::<Value>(s).ok()
        })
        .filter_map(|rec| turn_of(&rec))
        .collect()
}

/// Wie `extract_turns`, für schon geparste Datensätze.
pub fn extract_turns_from_values(records: &[Value]) -> Vec<Turn> {
    records.iter().filter_map(turn_of).collect()
}

/// Ein Transcript in Zeilen zerlegen wie Pythons Textmodus (`\r\n`, `\r`, `\n`).
pub fn transcript_lines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\n' || bytes[i] == b'\r' {
            out.push(&text[start..i]);
            if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
                i += 1;
            }
            start = i + 1;
        }
        i += 1;
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

fn fmt_turn(role: &str, text: &str, per_turn: usize) -> String {
    let mut text = squash_ws(text);
    if char_len(&text) > per_turn {
        text = format!("{}…", rstrip(take_chars(&text, per_turn)));
    }
    let role = match role {
        "user" => "User",
        "assistant" => "Assistant",
        r => r,
    };
    format!("{role}: {text}")
}

/// Kompakter Auszug: der ERSTE Zug (setzt das Thema) plus die JÜNGSTEN, bis das
/// Zeichenbudget voll ist. Eine Lücke dazwischen steht als `…` da.
pub fn build_digest(turns: &[Turn], max_chars: usize, per_turn: usize) -> String {
    let Some((first_role, first_text)) = turns.first() else { return String::new() };
    let first = fmt_turn(first_role, first_text, per_turn);
    let mut total = char_len(&first);
    let mut picked = Vec::new();
    for (role, text) in turns[1..].iter().rev() {
        let line = fmt_turn(role, text, per_turn);
        let n = char_len(&line);
        if total + n + 1 > max_chars {
            break;
        }
        picked.push(line);
        total += n + 1;
    }
    picked.reverse();
    let mut lines = vec![first];
    if picked.len() < turns.len() - 1 {
        lines.push("…".into());
    }
    lines.extend(picked);
    lines.join("\n")
}

/// Modell-Ausgabe zu einer Tooltip-Zeile: erster nicht-leerer Absatz, umschließende
/// Anführungszeichen und ein Präfix („Zusammenfassung:“ …) weg, Leerraum geglättet,
/// hart auf `max_len` gekürzt.
pub fn clean_summary(raw: &str, max_len: usize) -> String {
    let Some(first) = splitlines(raw).into_iter().map(strip).find(|l| !l.is_empty()) else {
        return String::new();
    };
    let mut text = first.to_string();
    let (head, tail) = (text.chars().next(), text.chars().next_back());
    if char_len(&text) >= 2 && head.is_some_and(|c| QUOTES.contains(c)) && tail.is_some_and(|c| QUOTES.contains(c)) {
        let inner: String = text.chars().skip(1).take(char_len(&text) - 2).collect();
        text = strip(&inner).to_string();
    }
    for pre in ["zusammenfassung:", "thema:", "summary:", "worum es geht:", "topic:"] {
        let n = pre.chars().count();
        if take_chars(&text, n).to_lowercase() == pre {
            text = strip(skip_chars(&text, n)).to_string();
            break;
        }
    }
    let mut text = squash_ws(&text);
    if char_len(&text) > max_len {
        text = format!("{}…", rstrip(take_chars(&text, max_len)));
    }
    text
}

/// cwd → Ordnername unter `~/.claude/projects`: jedes Zeichen außer `[A-Za-z0-9]` wird
/// zu `-` (auch `:`, `\` und Umlaute; „Müßig“ → „M--ig“).
pub fn enc_cwd(cwd: &str) -> String {
    cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}
