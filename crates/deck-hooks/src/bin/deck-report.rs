//! `deck-report <status>`: Claude-Code-Hook, schreibt `state\<slot>.json`.
//! Endet auf jedem Pfad mit Exit 0 (siehe `deck_hooks::run_guarded`).

fn main() {
    deck_hooks::run_guarded(|| {
        let status = std::env::args_os()
            .nth(1)
            .map_or_else(|| "thinking".to_string(), |a| a.to_string_lossy().into_owned());
        let env = deck_hooks::env::HookEnv::from_process();
        let _ = deck_hooks::report::run(&status, &env, deck_hooks::read_stdin);
    })
}
