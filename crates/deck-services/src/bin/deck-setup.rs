//! `deck-setup`: richtet das Deck auf diesem Rechner ein und nimmt es wieder heraus.
//! Gebraucht von `tools/install.ps1` und `tools/uninstall.ps1`.
//!
//! ```text
//! deck-setup install   --bin <ordner> [--statusline keep|replace]
//!                         Hooks und statusLine in Claudes settings.json auf die Programme
//!                         in <ordner> richten. Fremde Hooks bleiben. Eine fremde statusLine
//!                         bleibt bei keep (Vorgabe); replace sichert sie nach
//!                         %LOCALAPPDATA%\claude-agent-deck\statusline.before.json.
//! deck-setup check     --bin <ordner>   Befunde: fehlende Einträge, cmd /c, Pfade ins Leere …
//! deck-setup uninstall                  Einträge dieser Fassung heraus, gesicherte statusLine
//!                                       zurück, alles andere bleibt.
//! deck-setup shortcut  [--bin <ordner>] [--remove]
//!                                       Startmenü-Verknüpfung „Agent Deck“ (mit App-Kennung).
//! deck-setup wt-profile [--remove]      Windows-Terminal-Fragment mit dem Profil „Agent Deck“
//!                                       und den Farbschemata papier-ebenen(-nacht).
//! deck-setup remove-vscode-extension    Eintrag in ~/.vscode/extensions/extensions.json und
//!                                       Ordner agent-deck-bridge entfernen (mehrfach harmlos).
//! deck-setup resolve-reviewers --names "<text>" [--org <org>]
//!                                       Reviewer-Namen, Mailadressen oder Logins gegen die
//!                                       Mitglieder der GitHub-Organisation auflösen (nur
//!                                       lesend, über gh). Druckt eine JSON-Zeile
//!                                       (`deck_core::dash::reviewers::ReviewerReport`) für den
//!                                       Installer; scheitert nie an gh, `note` sagt warum.
//! ```
//!
//! Claudes settings.json liegt in `CLAUDE_CONFIG_DIR`, sonst in `~/.claude`.
//! Ausgabe: eine Zeile je Befund (`ok|info|warn|fail: …`), am Ende `## fails=N warns=N`.
//! Exit-Code 1, wenn es einen Fehler gab, 2 bei falschem Aufruf. `resolve-reviewers` druckt
//! nur das JSON und endet mit 0.

use std::path::PathBuf;
use std::process::ExitCode;

use deck_services::install::{self, StatusLine};
use deck_services::report::{Grade, Report};
use deck_services::{shortcut, statusline_backup, vscode_cleanup, wt_fragment};

const USAGE: &str = "deck-setup install --bin <ordner> [--statusline keep|replace] | check --bin <ordner> | uninstall \
                     | shortcut [--bin <ordner>] [--remove] | wt-profile [--remove] | remove-vscode-extension                      | resolve-reviewers --names <text> [--org <org>]";

fn arg_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str)
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

/// `claude.exe` als Symbol des Terminal-Profils: auf dem PATH, sonst dort, wo der
/// Installer von Claude Code es ablegt. Ein npm-`claude.cmd` taugt nicht als Symbol.
fn claude_icon() -> Option<String> {
    let local = deck_core::paths::home_dir().join(".local").join("bin").join("claude.exe");
    deck_win::which::which("claude.exe")
        .or_else(|| local.is_file().then_some(local))
        .map(|p| p.to_string_lossy().into_owned())
}

fn default_bin() -> PathBuf {
    deck_core::paths::app_dir().join("bin")
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let settings_file = install::settings_path();
    let cmd = args.first().map(String::as_str);
    let bin = arg_value(&args, "--bin").map(PathBuf::from);
    let remove = args.iter().any(|a| a == "--remove");
    if cmd == Some("resolve-reviewers") {
        let Some(names) = arg_value(&args, "--names") else {
            eprintln!("--names fehlt
{USAGE}");
            return ExitCode::from(2);
        };
        let org = arg_value(&args, "--org").unwrap_or("");
        let report = deck_services::reviewers::ReviewerResolver::system().resolve(org, names);
        match serde_json::to_string(&report) {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("JSON: {e}");
                return ExitCode::from(1);
            }
        }
        return ExitCode::SUCCESS;
    }
    let r = match (cmd, bin) {
        (Some("install"), Some(bin)) => {
            let Some(mode) = StatusLine::parse(arg_value(&args, "--statusline").unwrap_or("keep")) else {
                eprintln!("--statusline: keep oder replace\n{USAGE}");
                return ExitCode::from(2);
            };
            install::install(&settings_file, &bin, mode, &statusline_backup::path())
        }
        (Some("check"), Some(bin)) => install::check(&settings_file, &bin),
        (Some("uninstall"), _) => install::uninstall(&settings_file, &statusline_backup::path()),
        (Some("shortcut"), _) if remove => shortcut::remove(&shortcut::link_path()),
        (Some("shortcut"), bin) => {
            let exe = bin.unwrap_or_else(default_bin).join("agent-deck.exe");
            shortcut::create(&shortcut::link_path(), &exe, shortcut::APP_ID)
        }
        (Some("wt-profile"), _) if remove => wt_fragment::remove(&wt_fragment::fragment_path()),
        (Some("wt-profile"), _) => {
            wt_fragment::install(&wt_fragment::fragment_path(), &wt_fragment::wt_settings_path(), claude_icon().as_deref())
        }
        (Some("remove-vscode-extension"), _) => vscode_cleanup::remove_extension(&vscode_cleanup::extensions_dir()),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    print(&r);
    if r.fails() > 0 { ExitCode::from(1) } else { ExitCode::SUCCESS }
}
