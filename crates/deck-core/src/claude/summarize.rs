//! Die Gesprächszüge eines Agenten-Chats aus dem Transcript: die reinen Teile.
//!
//! Datenweg: der report-Hook merkt sich je Slot `session_id` und `cwd`; daraus findet
//! das Deck das Transcript `~/.claude/projects/<kodiertes cwd>/<session>.jsonl`.
//! `extract_turns` zieht die echten Züge für die Ticket- und PR-Erkennung (`refs.rs`);
//! das Lagebild baut seinen Auszug in `briefing.rs` auf denselben Zügen.

use serde_json::Value;

use crate::claude::refs::Turn;

/// Von Claude Code selbst eingeschobene „User“-Zeilen: Skill-Texte und Hook-Ausgaben
/// (`isMeta`), die Zusammenfassung nach `/compact` (`isCompactSummary`, sie wiederholt
/// jedes Ticket des bisherigen Gesprächs), Nebenläufe von Subagenten (`isSidechain`) und
/// Tool-Ergebnisse (`toolUseResult`). Getippt hat davon nichts der Nutzer.
fn injected(rec: &Value) -> bool {
    ["isMeta", "isCompactSummary", "isSidechain"].iter().any(|k| rec.get(*k).and_then(Value::as_bool) == Some(true))
        || rec.get("toolUseResult").is_some()
}

/// Getippter Text einer User-Zeile: ein String, oder eine Liste aus Text- (und Bild-)
/// Blöcken, wenn etwas eingefügt wurde. Steckt ein `tool_result` darin, ist es ein
/// Tool-Ergebnis und kein getippter Text.
fn typed_text(content: &Value) -> Option<String> {
    let keep = |t: &str| !(t.is_empty() || t.starts_with('<') || t.starts_with("Caveat:"));
    if let Some(s) = content.as_str() {
        let t = s.trim();
        return keep(t).then(|| t.to_string());
    }
    let blocks = content.as_array()?;
    if blocks.iter().any(|b| b.get("type").and_then(Value::as_str) == Some("tool_result")) {
        return None;
    }
    let parts: Vec<&str> = blocks
        .iter()
        .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|b| b.get("text").and_then(Value::as_str))
        .map(str::trim)
        .filter(|t| keep(t))
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n"))
}

/// Ein Zug aus einem schon geparsten Transcript-Datensatz.
pub(crate) fn turn_of(rec: &Value) -> Option<Turn> {
    let msg = rec.get("message")?.as_object()?;
    let content = msg.get("content");
    if injected(rec) {
        return None;
    }
    match rec.get("type")?.as_str()? {
        "user" => Some(("user".into(), typed_text(content?)?)),
        "assistant" => {
            let parts: Vec<&str> = content?
                .as_array()?
                .iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .collect();
            let text = parts.join("\n").trim().to_string();
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
            let s = ln.as_ref().trim();
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

/// cwd → Ordnername unter `~/.claude/projects`: jedes Zeichen außer `[A-Za-z0-9]` wird
/// zu `-` (auch `:`, `\` und Umlaute; „Müßig“ → „M--ig“).
pub fn enc_cwd(cwd: &str) -> String {
    cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}
