//! `deck-statusline`: Claude-Code-statusLine, gibt die Zeile als UTF-8 aus und schreibt
//! `state\<slot>.live.json`. Endet auf jedem Pfad mit Exit 0.

use std::io::Write;

fn main() {
    deck_hooks::run_guarded(|| {
        let stdin = deck_hooks::read_stdin();
        let env = deck_hooks::env::HookEnv::from_process();
        if let Some(text) = deck_hooks::statusline::run(&env, stdin.as_deref()) {
            // Rohe UTF-8-Bytes, nie über eine Codepage; eine geschlossene Pipe ist kein Fehler.
            let mut out = std::io::stdout().lock();
            let _ = out.write_all(text.as_bytes());
            let _ = out.flush();
        }
    })
}
