//! Was nie angeboten und nie beendet wird, und wie ein Prozess in der Zeile heißt.
//!
//! WICHTIGER als das Finden ist das NICHT-Finden: aus einem Agentenfenster laufen auch
//! Claude selbst, der Konsolenwirt und manchmal das Deck. Die Sperrliste nennt zu jedem
//! Eintrag den Grund ([`Protected`]); die Tests halten sie fest.

/// Warum ein Prozess geschützt ist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protected {
    /// Claude Code selbst (claude.exe oder node mit `@anthropic-ai/claude-code`).
    Claude,
    /// Ein Programm des Decks oder etwas aus seinem Programmordner.
    Deck,
    /// Docker Desktop, Engine, vpnkit, WSL-Dienst.
    Docker,
    /// Konsolenwirt (`conhost`, `OpenConsole`): endet mit seiner Konsole von selbst.
    Console,
    /// Windows Terminal und `wt.exe`.
    Terminal,
    /// Der Explorer (Start-Process ohne Fenster kann ihn als Eltern haben).
    Explorer,
    /// Windows-Kernprozesse.
    System,
    /// Sitzung 0: Dienste, dort startet kein Entwickler etwas.
    Session0,
}

/// Programme des Decks (Kleinschreibung, ohne `.exe`).
const DECK_EXES: [&str; 6] = ["agent-deck", "deck-console", "deck-report", "deck-statusline", "deck-watchdog", "deck-setup"];
/// Programmordner des Decks unter `%LOCALAPPDATA%`.
const DECK_DIR: &str = "\\claude-agent-deck\\";
const CONSOLE_EXES: [&str; 2] = ["conhost", "openconsole"];
const TERMINAL_EXES: [&str; 3] = ["windowsterminal", "wt", "windowsterminalshellext"];
const SYSTEM_EXES: [&str; 16] = [
    "system", "idle", "smss", "csrss", "wininit", "winlogon", "services", "lsass", "lsaiso", "svchost", "spoolsv", "dwm",
    "fontdrvhost", "sihost", "ctfmon", "taskhostw",
];
const DOCKER_PREFIXES: [&str; 5] = ["docker", "com.docker.", "vpnkit", "wslservice", "wslhost"];
/// Dateinamen, unter denen Claude Code startet (Exe, npm-Shim, Skript).
const CLAUDE_FILES: [&str; 5] = ["claude", "claude.exe", "claude.cmd", "claude.ps1", "claude.js"];

/// Exe-Name ohne Pfad und ohne `.exe`, klein.
pub fn exe_stem(exe: &str) -> String {
    let name = file_name(exe).to_ascii_lowercase();
    name.strip_suffix(".exe").map(str::to_string).unwrap_or(name)
}

fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// Ist dieser Prozess geschützt? `None` = darf angeboten und beendet werden.
pub fn protected(pid: u32, exe: &str, image: Option<&str>, cmdline: Option<&str>, session: Option<u32>) -> Option<Protected> {
    if pid <= 4 {
        return Some(Protected::System);
    }
    // Nur eine ECHTE 0 zählt: eine unbekannte Sitzung darf nicht alles schützen.
    if session == Some(0) {
        return Some(Protected::Session0);
    }
    let stem = exe_stem(exe);
    if is_claude(&stem, cmdline) {
        return Some(Protected::Claude);
    }
    let in_deck_dir = image.is_some_and(|i| i.to_ascii_lowercase().contains(DECK_DIR));
    if DECK_EXES.contains(&stem.as_str()) || in_deck_dir {
        return Some(Protected::Deck);
    }
    if DOCKER_PREFIXES.iter().any(|p| stem.starts_with(p)) || stem == "docker desktop" {
        return Some(Protected::Docker);
    }
    if CONSOLE_EXES.contains(&stem.as_str()) {
        return Some(Protected::Console);
    }
    if TERMINAL_EXES.contains(&stem.as_str()) {
        return Some(Protected::Terminal);
    }
    if stem == "explorer" {
        return Some(Protected::Explorer);
    }
    if SYSTEM_EXES.contains(&stem.as_str()) {
        return Some(Protected::System);
    }
    None
}

/// Claude Code am Namen, am Programm in den ersten beiden Argumenten (`node …\claude.js`,
/// npm-Shim) oder am npm-Paket. Bewusst NICHT an einem Ordner `claude` irgendwo im Pfad:
/// Claude legt Ausgabedateien unter `%TEMP%\claude\…` ab, und ein Hintergrundjob mit so
/// einem Pfad in der Kommandozeile ist ein Dev-Server, nicht Claude.
fn is_claude(stem: &str, cmdline: Option<&str>) -> bool {
    if stem == "claude" {
        return true;
    }
    let Some(cmd) = cmdline else { return false };
    let lower = cmd.to_ascii_lowercase();
    if lower.contains("@anthropic-ai/claude-code") || lower.contains("@anthropic-ai\\claude-code") {
        return true;
    }
    split_cmdline(cmd).iter().take(2).any(|a| CLAUDE_FILES.contains(&file_name(a).to_ascii_lowercase().as_str()))
}

/// Kommandozeile zerlegen wie `CommandLineToArgvW`, vereinfacht: Leerraum trennt, `"…"`
/// hält zusammen, `\"` ist ein Anführungszeichen. Für die Anzeige genügt das.
pub fn split_cmdline(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut quoted, mut in_token) = (false, false);
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'"') => {
                chars.next();
                cur.push('"');
                in_token = true;
            }
            '"' => {
                quoted = !quoted;
                in_token = true;
            }
            c if c.is_whitespace() && !quoted => {
                if in_token {
                    out.push(std::mem::take(&mut cur));
                    in_token = false;
                }
            }
            c => {
                cur.push(c);
                in_token = true;
            }
        }
    }
    if in_token {
        out.push(cur);
    }
    out
}

/// Laufzeitumgebungen, deren erstes Argument das eigentliche Programm ist.
const INTERPRETERS: [&str; 7] = ["node", "bun", "deno", "python", "pythonw", "py", "dotnet"];
const SCRIPT_EXTS: [&str; 5] = [".js", ".cjs", ".mjs", ".ts", ".py"];

/// Ein Pfad (absolut, UNC, mit `\` oder relativ mit `./`), kein Stück Code (`node -e …`)
/// und keine Adresse (`http://…`).
fn looks_like_path(a: &str) -> bool {
    if a.contains(char::is_whitespace) || a.contains(['(', ')', ';', '\'', '=', ',']) {
        return false;
    }
    let b = a.as_bytes();
    let drive = b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/');
    drive || a.contains('\\') || a.starts_with('/') || a.starts_with("./") || a.starts_with("../")
}

/// Kommandozeile für die Zeile: ohne das Programm selbst, Pfade auf ihren Dateinamen
/// gekürzt, bei node & Co. das Skript ohne Endung (`node …\pnpm.cjs nx serve` →
/// `pnpm nx serve`). Leer → Programmname.
pub fn display_command(cmdline: Option<&str>, exe: &str) -> String {
    let stem = exe_stem(exe);
    let args = cmdline.map(split_cmdline).unwrap_or_default();
    let mut parts: Vec<String> = Vec::new();
    for (i, a) in args.iter().enumerate().skip(1) {
        let mut shown = if looks_like_path(a) && !a.starts_with('-') { file_name(a).to_string() } else { a.clone() };
        if i == 1 && INTERPRETERS.contains(&stem.as_str()) {
            let lower = shown.to_ascii_lowercase();
            if let Some(ext) = SCRIPT_EXTS.iter().find(|e| lower.ends_with(**e)) {
                shown.truncate(shown.len() - ext.len());
            }
        }
        if !shown.is_empty() {
            parts.push(shown);
        }
    }
    let text = parts.join(" ");
    if text.is_empty() { stem } else { text }
}
