//! `deck-setup`: richtet die Claude-Code-Hooks für das Deck ein, prüft sie und nimmt sie
//! wieder heraus, und räumt die frühere VS-Code-Extension weg. Gebraucht von
//! `tools/install.ps1`.
//!
//! ```text
//! deck-setup install   --bin <ordner>   Hooks und statusLine auf die Programme in <ordner>
//!                                       richten. Fremde Einträge bleiben stehen.
//! deck-setup check     --bin <ordner>   Befunde: fehlende Einträge, cmd /c, Pfade ins Leere …
//! deck-setup uninstall                  Einträge dieser Fassung heraus, alles andere bleibt.
//! deck-setup remove-vscode-extension    Eintrag in ~/.vscode/extensions/extensions.json und
//!                                       Ordner agent-deck-bridge entfernen (mehrfach harmlos).
//! ```
//!
//! Ausgabe: eine Zeile je Befund (`ok|info|warn|fail: …`), am Ende `## fails=N warns=N`.
//! Exit-Code 1, wenn es einen Fehler gab.

use std::path::PathBuf;
use std::process::ExitCode;

use deck_services::{install, vscode_cleanup};
use deck_services::report::{Grade, Report};

fn arg_value(args: &[String], name: &str) -> Option<PathBuf> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(PathBuf::from)
}

fn print(r: &Report) {
    for line in &r.lines {
        let grade = match line.grade {
            Grade::Ok => "ok",
            Grade::Info => "info",
            Grade::Warn => "warn",
            Grade::Fail => "fail",
        };
        println!("{grade}: {}", line.text);
    }
    if let Some(b) = &r.backup {
        println!("info: Sicherung: {b}");
    }
    println!("## fails={} warns={}", r.fails(), r.warns());
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let settings_file = install::settings_path();
    let cmd = args.first().map(String::as_str);
    let bin = arg_value(&args, "--bin");
    let r = match (cmd, bin) {
        (Some("install"), Some(bin)) => install::install(&settings_file, &bin, true),
        (Some("check"), Some(bin)) => install::check(&settings_file, &bin),
        (Some("uninstall"), _) => install::uninstall(&settings_file),
        (Some("remove-vscode-extension"), _) => vscode_cleanup::remove_extension(&vscode_cleanup::extensions_dir()),
        _ => {
            eprintln!("deck-setup install|check --bin <ordner> | uninstall | remove-vscode-extension");
            return ExitCode::from(2);
        }
    };
    print(&r);
    if r.fails() > 0 { ExitCode::from(1) } else { ExitCode::SUCCESS }
}
