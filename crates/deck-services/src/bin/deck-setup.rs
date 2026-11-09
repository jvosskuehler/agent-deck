//! `deck-setup`: richtet die Claude-Code-Hooks für das neue Deck ein und nimmt sie wieder
//! heraus. Gebraucht von `tools/umstellen.ps1` und `tools/zurueck.ps1`.
//!
//! ```text
//! deck-setup install  --bin <ordner>   Hooks auf deck-report richten. Python-Einträge und die
//!                                      Python-statusLine bleiben stehen (beide schreiben dieselben
//!                                      Dateien). `hooks` und `statusLine` werden vorher gemerkt.
//! deck-setup check    --bin <ordner>   Befunde: fehlende Einträge, cmd /c, Pfade ins Leere …
//! deck-setup remove-legacy --bin <ordner>
//!                                      Python-Einträge heraus, statusLine auf deck-statusline
//!                                      (erst nach dem Beweis, dass deck-report schreibt).
//! deck-setup rollback                  Einträge dieser Fassung heraus, Python-Einträge und
//!                                      Python-statusLine aus dem gemerkten Stand zurück. Andere
//!                                      Hooks bleiben, auch später dazugekommene.
//! ```
//!
//! Ausgabe: eine Zeile je Befund (`ok|info|warn|fail: …`), am Ende `## fails=N warns=N`.
//! Exit-Code 1, wenn es einen Fehler gab.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use deck_core::claude::hook_setup::{self, Owner, owner};
use deck_core::claude::settings;
use deck_services::install;
use deck_services::report::{Grade, Report};
use serde_json::Value;

/// Hier liegen `hooks` und `statusLine`, wie sie vor der Umstellung galten. Der Rückweg
/// stellt genau diese beiden Einträge wieder her; alle anderen Einstellungen bleiben, wie
/// sie inzwischen sind.
fn snapshot_path() -> PathBuf {
    deck_core::paths::app_dir().join("settings.before-deck.json")
}

const KEYS: [&str; 2] = ["hooks", "statusLine"];

/// Zeigt die statusLine auf dieses Deck?
fn is_ours(v: &Value) -> bool {
    v.get("command").and_then(Value::as_str).is_some_and(|c| owner(c) == Owner::Deck)
}

/// Beim ersten Einrichten den Stand merken. Ein späterer Lauf überschreibt ihn nicht,
/// sonst wäre nach zwei Läufen der Stand „vorher“ schon der umgestellte.
fn remember(settings_file: &Path, r: &mut Report) {
    let path = snapshot_path();
    if path.exists() {
        r.push(Grade::Info, format!("Stand vor der Umstellung liegt schon vor: {}", path.display()));
        return;
    }
    let current = settings::load(settings_file);
    let mut snap = serde_json::Map::new();
    for key in KEYS {
        snap.insert(key.into(), current.get(key).cloned().unwrap_or(Value::Null));
    }
    match deck_core::paths::save_json(&path, &Value::Object(snap)) {
        Ok(()) => r.push(Grade::Info, format!("Stand vor der Umstellung gemerkt: {}", path.display())),
        Err(e) => r.push(Grade::Fail, format!("Stand nicht gemerkt ({e}), breche ab.")),
    }
}

/// Enthält eine Hook-Gruppe (`{matcher, hooks: [...]}`) einen Eintrag dieses Besitzers?
fn group_has(group: &Value, who: Owner) -> bool {
    group.get("hooks").and_then(Value::as_array).is_some_and(|hs| {
        hs.iter().any(|h| h.get("command").and_then(Value::as_str).is_some_and(|c| owner(c) == who))
    })
}

/// Chirurgisch zurück: nur die Einträge dieser Fassung heraus und die Python-Einträge
/// aus dem gemerkten Stand wieder hinein. Alles andere in `hooks` bleibt, wie es jetzt
/// ist, auch Hooks, die nach der Umstellung dazugekommen sind (z. B. ein
/// PermissionRequest-Hook). Die statusLine kommt nur zurück, wenn gerade unsere oder
/// keine eingetragen ist.
fn restore(settings_file: &Path, r: &mut Report) {
    let path = snapshot_path();
    let Some(Value::Object(snap)) = deck_core::paths::load_json(&path) else {
        r.push(Grade::Fail, format!("Kein gemerkter Stand unter {}, nichts zurückgestellt.", path.display()));
        return;
    };
    let (mut s, _) = hook_setup::remove(&settings::load(settings_file));

    if let Some(Value::Object(old_hooks)) = snap.get("hooks") {
        let hooks = s.entry("hooks").or_insert_with(|| Value::Object(serde_json::Map::new()));
        if let Value::Object(hooks) = hooks {
            for (event, groups) in old_hooks {
                let Some(groups) = groups.as_array() else { continue };
                let list = hooks.entry(event.clone()).or_insert_with(|| Value::Array(Vec::new()));
                let Value::Array(list) = list else { continue };
                if list.iter().any(|g| group_has(g, Owner::Legacy)) {
                    continue; // Python-Eintrag steht noch, nicht doppeln.
                }
                for g in groups.iter().filter(|g| group_has(g, Owner::Legacy)) {
                    list.push(g.clone());
                    r.push(Grade::Info, format!("{event}: Python-Eintrag zurück"));
                }
            }
        }
    }

    let ours_or_none = s.get("statusLine").is_none_or(is_ours);
    match snap.get("statusLine") {
        Some(old) if !old.is_null() && ours_or_none => {
            s.insert("statusLine".into(), old.clone());
            r.push(Grade::Info, "statusLine wie vor der Umstellung");
        }
        _ if s.get("statusLine").is_some_and(is_ours) => {
            s.remove("statusLine");
        }
        _ => {}
    }

    match settings::save(&s, settings_file) {
        Ok(()) => {
            r.changed = true;
            r.push(Grade::Ok, "hooks und statusLine wie vor der Umstellung.");
            // Danach wieder frei für eine neue Umstellung.
            let _ = std::fs::remove_file(&path);
        }
        Err(e) => r.push(Grade::Fail, format!("settings.json nicht zurückgeschrieben: {e}")),
    }
}

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
        println!("info: Sicherung der settings.json: {b}");
    }
    println!("## fails={} warns={}", r.fails(), r.warns());
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let settings_file = install::settings_path();
    let mut r = Report::default();
    match args.first().map(String::as_str) {
        Some("install") => {
            let Some(bin) = arg_value(&args, "--bin") else {
                eprintln!("deck-setup install --bin <ordner>");
                return ExitCode::from(2);
            };
            remember(&settings_file, &mut r);
            if r.fails() == 0 {
                r.append(install::install(&settings_file, &bin, true));
            }
        }
        Some("remove-legacy") => {
            let Some(bin) = arg_value(&args, "--bin") else {
                eprintln!("deck-setup remove-legacy --bin <ordner>");
                return ExitCode::from(2);
            };
            // Mit den Python-Einträgen geht auch die Python-statusLine; danach die eigene
            // eintragen, sonst stünde gar keine mehr da.
            r.append(install::remove_legacy(&settings_file));
            if r.fails() == 0 {
                r.append(install::install(&settings_file, &bin, true));
            }
        }
        Some("check") => {
            let Some(bin) = arg_value(&args, "--bin") else {
                eprintln!("deck-setup check --bin <ordner>");
                return ExitCode::from(2);
            };
            r.append(install::check(&settings_file, &bin));
        }
        Some("rollback") => restore(&settings_file, &mut r),
        _ => {
            eprintln!("deck-setup install|check|remove-legacy --bin <ordner> | rollback");
            return ExitCode::from(2);
        }
    }
    print(&r);
    if r.fails() > 0 { ExitCode::from(1) } else { ExitCode::SUCCESS }
}
