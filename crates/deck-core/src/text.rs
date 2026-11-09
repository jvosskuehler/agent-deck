//! Texte in Zeichen zählen und schneiden, nicht in Bytes.
//!
//! Kürzungen („…“ nach 42 Zeichen) sollen bei Umlauten und Emoji genauso lang ausfallen
//! wie bei ASCII, und ein Schnitt in Bytes könnte mitten in einem Zeichen landen (Panik).

/// Länge in Zeichen.
pub fn char_len(s: &str) -> usize {
    s.chars().count()
}

/// Die ersten höchstens `n` Zeichen.
pub fn take_chars(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// Alles ab dem Zeichen `n`.
pub fn skip_chars(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[i..],
        None => "",
    }
}

/// Die letzten höchstens `n` Zeichen vor dem Byte-Index `pos`.
pub fn chars_before(s: &str, pos: usize, n: usize) -> &str {
    let head = &s[..pos];
    if n == 0 {
        return "";
    }
    match head.char_indices().rev().nth(n - 1) {
        Some((i, _)) => &head[i..],
        None => head,
    }
}

/// Leerraum glätten: jeder Lauf wird ein Leerzeichen, Ränder fallen weg.
pub fn squash_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Die erste Zeile, ohne Ränder.
pub fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("").trim()
}
