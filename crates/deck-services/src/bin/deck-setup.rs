//! `deck-setup`: richtet das Deck auf diesem Rechner ein und nimmt es wieder heraus.
//! Gebraucht von `tools/install.ps1` und `tools/uninstall.ps1`.
//!
//! ```text
//! deck-setup install   --bin <ordner> [--statusline replace|keep]
//!                         Hooks und Infozeile (statusLine) in Claudes settings.json auf die
//!                         Programme in <ordner> richten, fehlende Vorgaben (Modus, Effort)
//!                         eintragen. Fremde Hooks bleiben. Eine fremde Infozeile wird nach
//!                         %LOCALAPPDATA%\claude-agent-deck\statusline.before.json gesichert und
//!                         läuft hinter der des Decks weiter; `keep` lässt sie allein stehen.
//! deck-setup check     --bin <ordner>   Befunde: fehlende Einträge, cmd /c, Pfade ins Leere …
//! deck-setup look      --from <papier> [--remove]
//!                                       Papier-Look: Schrift IBM Plex Mono (pro Benutzer),
//!                                       Claude-Theme, Einstellungen der Deck-Agenten, Shader.
//! deck-setup shortcut  [--bin <ordner>] [--remove]
//!                                       Startmenü-Verknüpfung „Agent Deck“ (mit App-Kennung).
//! deck-setup wt-profile [--remove]      Windows-Terminal-Fragment mit dem Profil „Agent Deck“
//!                                       und den Farbschemata papier-ebenen(-nacht).
//! deck-setup autostart [--bin <ordner>] [--remove]
//!                                       HKCU\…\Run\AgentDeck.
//! deck-setup apps-entry --bin <ordner> --version <v> [--remove]
//!                                       Eintrag unter „Apps“ (HKCU\…\Uninstall\AgentDeck).
//! deck-setup uninstall                  Alles heraus, was die Einrichtung hinzugefügt hat
//!                                       (laut installed.json und am Namen), gesicherte
//!                                       Infozeile zurück. Programme und Daten bleiben.
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
//! Was eingerichtet wurde, verzeichnet jeder Befehl in
//! `%LOCALAPPDATA%\claude-agent-deck\installed.json` (`deck_services::installed`).
//! Claudes settings.json liegt in `CLAUDE_CONFIG_DIR`, sonst in `~/.claude`.
//! Ausgabe: eine Zeile je Befund (`ok|info|warn|fail: …`), dann je Merker für den
//! Installer `## <merker>`, am Ende `## fails=N warns=N`. Exit-Code 1, wenn es einen
//! Fehler gab, 2 bei falschem Aufruf. `resolve-reviewers` druckt nur das JSON und endet
//! mit 0.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use deck_services::install::{self, StatusLine};
use deck_services::installed::{self, Installed};
use deck_services::look::{self, LookPaths};
use deck_services::report::{Grade, Report};
use deck_services::user_system::RealSystem;
use deck_services::{apps_entry, autostart, shortcut, statusline_backup, teardown, vscode_cleanup, wt_fragment, wt_theme};

const USAGE: &str = "deck-setup install --bin <ordner> [--statusline replace|keep] | check --bin <ordner> | uninstall \
                     | look --from <ordner> [--remove] | shortcut [--bin <ordner>] [--remove] | wt-profile [--remove] \
                     | autostart [--bin <ordner>] [--remove] | apps-entry --bin <ordner> --version <v> [--remove] \
                     | remove-vscode-extension | resolve-reviewers --names <text> [--org <org>]";

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
    for t in &r.tags {
        println!("## {t}");
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
            eprintln!("--names fehlt\n{USAGE}");
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
    let manifest_file = installed::path();
    let mut m = Installed::load(&manifest_file);
    let sys = RealSystem;
    let mut r = match (cmd, bin) {
        (Some("install"), Some(bin)) => {
            let Some(mode) = StatusLine::parse(arg_value(&args, "--statusline").unwrap_or("replace")) else {
                eprintln!("--statusline: replace oder keep\n{USAGE}");
                return ExitCode::from(2);
            };
            install::install(&settings_file, &bin, mode, &statusline_backup::path(), &mut m)
        }
        (Some("check"), Some(bin)) => install::check(&settings_file, &bin),
        (Some("uninstall"), _) => teardown::uninstall_all(&teardown::Targets::real(), &sys, &mut m),
        (Some("look"), _) if remove => look::remove(&LookPaths::real(), &sys, &mut m),
        (Some("look"), _) => {
            let Some(from) = arg_value(&args, "--from") else {
                eprintln!("--from fehlt\n{USAGE}");
                return ExitCode::from(2);
            };
            look::install(Path::new(from), &LookPaths::real(), &sys, &mut m)
        }
        (Some("shortcut"), _) if remove => {
            m.shortcut = None;
            shortcut::remove(&shortcut::link_path())
        }
        (Some("shortcut"), bin) => {
            let exe = bin.unwrap_or_else(default_bin).join("agent-deck.exe");
            let r = shortcut::create(&shortcut::link_path(), &exe, shortcut::APP_ID);
            if r.is_ok() {
                m.shortcut = Some(shortcut::link_path().to_string_lossy().into_owned());
            }
            r
        }
        (Some("wt-profile"), _) if remove => {
            let mut r = wt_fragment::remove(&wt_fragment::fragment_path(), &mut m);
            r.append(wt_theme::remove(&wt_fragment::wt_settings_path(), &mut m));
            r
        }
        (Some("wt-profile"), _) => {
            let shader = LookPaths::real().shader();
            let shader = shader.is_file().then(|| shader.to_string_lossy().into_owned());
            let shell = wt_fragment::shell_commandline(deck_win::which::which("pwsh.exe").is_some());
            let profile = wt_fragment::fragment(claude_icon().as_deref(), shell, shader.as_deref());
            let mut r = wt_fragment::install(&wt_fragment::fragment_path(), &wt_fragment::wt_settings_path(), &profile, &mut m);
            r.append(wt_theme::install(&wt_fragment::wt_settings_path(), &mut m));
            r
        }
        (Some("autostart"), _) if remove => autostart::remove(&sys, &mut m),
        (Some("autostart"), bin) => autostart::set(&bin.unwrap_or_else(default_bin), &sys, &mut m),
        (Some("apps-entry"), _) if remove => apps_entry::remove(&sys, &mut m),
        (Some("apps-entry"), Some(bin)) => {
            let version = arg_value(&args, "--version").unwrap_or("unbekannt");
            apps_entry::set(&bin, version, &sys, &mut m)
        }
        (Some("remove-vscode-extension"), _) => vscode_cleanup::remove_extension(&vscode_cleanup::extensions_dir()),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    if let Err(e) = m.save(&manifest_file) {
        r.push(Grade::Warn, format!("Liste der Änderungen ({}) nicht gespeichert: {e}", manifest_file.display()));
    }
    print(&r);
    if r.fails() > 0 { ExitCode::from(1) } else { ExitCode::SUCCESS }
}
