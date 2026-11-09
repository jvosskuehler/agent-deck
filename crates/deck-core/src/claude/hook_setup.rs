//! Unsere Einträge in Claudes settings.json: eintragen, prüfen, entfernen.
//!
//! Warum Code und keine Anleitung: sechs Einträge mit absolutem Pfad sind genau die
//! Stelle, an der drei Fallen sitzen, die das Projekt schon Stunden gekostet haben. Ein
//! fehlendes `|| exit 0` blockiert den Agenten (Exit ≠ 0 gilt bei UserPromptSubmit und
//! PreToolUse als Veto), ein `cmd /c` davor lässt die Kacheln STILL grau, und ein Pfad
//! aus einer früheren Installation zeigt ins Leere. Keine fällt am Exit-Code auf.
//!
//! CHIRURGISCH: wir ergänzen die Datei, statt sie zu schreiben. Fremde Hooks bleiben;
//! unsere werden am DATEINAMEN erkannt und ersetzt, ein zweiter Lauf ist ein
//! Nulldurchgang, ein verschobener Programmordner wird repariert statt verdoppelt.
//! „Unsere“ sind die Programme `deck-report.exe` und `deck-statusline.exe`; die Skripte
//! der Python-Fassung zählen als fremd und bleiben unangetastet.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value, json};

/// (Hook-Event, Status-Argument, Matcher). Bewusst kein idle_prompt: „wartet“ =
/// Notification, „fertig“ = Stop.
pub const HOOKS: [(&str, &str, Option<&str>); 6] = [
    ("SessionStart", "idle", None),
    ("UserPromptSubmit", "thinking", None),
    ("PreToolUse", "running", Some("*")),
    ("PostToolUse", "thinking", Some("*")),
    ("Notification", "waiting", None),
    ("Stop", "done", None),
];

pub const REPORT_EXE: &str = "deck-report.exe";
pub const STATUSLINE_EXE: &str = "deck-statusline.exe";
/// Claude Code ruft die Statuszeile zusätzlich alle so viele Sekunden auf
/// (`statusLine.refreshInterval`). Ohne den Takt stünde der Cache-Countdown genau dann
/// still, wenn er zählt: wenn im Chat nichts passiert.
pub const STATUSLINE_REFRESH_S: u64 = 15;

/// Die äußere Schale: fehlt die Datei, kommt das Fangnetz IM Programm nicht zum Zug,
/// dann urteilt die Shell, und ohne diesen Anhang gilt ihr Exit ≠ 0 als Veto.
pub const TAIL: &str = "|| exit 0";

/// Wem ein Kommando gehört.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    /// Die Programme dieser Fassung.
    Deck,
    Foreign,
}

static DECK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:^|[/"\s])(deck-report|deck-statusline)(?:\.exe)?(?:$|["\s])"#).expect("DECK_RE")
});

/// Wem gehört ein Kommando? Erkannt am DATEINAMEN als eigenes Wegstück (mit Grenze
/// davor und dahinter, ein fremdes `my-deck-report.exe` trägt nicht), nicht am ganzen Pfad:
/// so findet ein Lauf auch die Einträge eines verschobenen Ordners.
pub fn owner(command: &str) -> Owner {
    let c = command.replace('\\', "/").to_lowercase();
    if DECK_RE.is_match(&c) { Owner::Deck } else { Owner::Foreign }
}

fn with_slashes(dir: &str) -> String {
    dir.replace('\\', "/").trim_end_matches('/').to_string()
}

/// Kommando eines Status-Hooks: absoluter Pfad mit Vorwärts-Schrägstrichen (gilt in
/// bash, PowerShell und im JSON ohne Maskieren), kein `cmd /c`, äußere Schale.
pub fn hook_command(bin_dir: &str, status: &str) -> String {
    format!("\"{}/{REPORT_EXE}\" {status} {TAIL}", with_slashes(bin_dir))
}

/// Kommando der statusLine, ebenfalls mit äußerer Schale (jeder Eintrag endet darauf).
pub fn statusline_command(bin_dir: &str) -> String {
    format!("\"{}/{STATUSLINE_EXE}\" {TAIL}", with_slashes(bin_dir))
}

static QUOTED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""([^"]+\.(?:exe|py))""#).expect("QUOTED"));
static BARE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\S+\.(?:exe|py))").expect("BARE"));

/// Den Programm- bzw. Skriptpfad aus einem Kommando ziehen (gequotet oder nicht).
pub fn command_path(command: &str) -> Option<String> {
    QUOTED.captures(command).or_else(|| BARE.captures(command)).map(|c| c[1].to_string())
}

/// Pfad vergleichbar machen wie `normcase(normpath(…))` unter Windows.
pub fn norm_path(p: &str) -> String {
    let p = p.replace('/', "\\").to_lowercase();
    let mut parts: Vec<&str> = Vec::new();
    for seg in p.split('\\') {
        match seg {
            "" | "." if !parts.is_empty() => {}
            ".." if parts.len() > 1 => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("\\")
}

fn parent_of(p: &str) -> String {
    let n = norm_path(p);
    n.rsplit_once('\\').map(|(d, _)| d.to_string()).unwrap_or_default()
}

/// Zeigt ein Kommando schon auf `leaf` IN DIESEM Ordner? Toleranter als ein
/// Stringvergleich: ein eigenes Anhängsel an der statusLine wird nicht jedes Mal neu
/// geschrieben. Bei den sechs Hooks gilt das nicht, dort ist die exakte Form der Zweck.
fn points_here(command: Option<&str>, bin_dir: &str, leaf: &str) -> bool {
    let Some(p) = command.and_then(command_path) else { return false };
    norm_path(&p) == norm_path(&format!("{bin_dir}/{leaf}"))
}

fn cmd_of(h: &Value) -> Option<&str> {
    h.get("command").and_then(Value::as_str)
}

/// Einträge, für die `strip(owner)` gilt, aus den Gruppen EINES Events entfernen:
/// (verbleibende Gruppen, entfernte Kommandos). Eine dadurch leere Gruppe fällt weg,
/// eine schon vorher leere bleibt (fremde Struktur räumen wir nicht auf).
fn strip_where(groups: &[Value], strip: impl Fn(Owner) -> bool) -> (Vec<Value>, Vec<String>) {
    let mut rest = Vec::new();
    let mut removed = Vec::new();
    for g in groups {
        let Some(inner) = g.get("hooks").and_then(Value::as_array) else {
            rest.push(g.clone());
            continue;
        };
        let mut keep = Vec::new();
        for h in inner {
            match cmd_of(h) {
                Some(c) if strip(owner(c)) => removed.push(c.to_string()),
                _ => keep.push(h.clone()),
            }
        }
        if keep.len() == inner.len() {
            rest.push(g.clone());
        } else if !keep.is_empty() {
            let mut ng = g.clone();
            ng["hooks"] = Value::Array(keep);
            rest.push(ng);
        }
    }
    (rest, removed)
}

/// Optionen für `merge`.
#[derive(Debug, Clone, Copy, Default)]
pub struct MergeOptions {
    /// Auch eine FREMDE statusLine ersetzen.
    pub force_statusline: bool,
}

/// Eine Meldung: (Grad, Text). Grad `info` = getan, `warn` = bewusst ausgelassen.
pub type Note = (&'static str, String);

fn hooks_map(settings: &Map<String, Value>) -> Map<String, Value> {
    settings.get("hooks").and_then(Value::as_object).cloned().unwrap_or_default()
}

/// Hooks und statusLine in ein GELESENES settings-Objekt mergen. Rein, kein IO. Ob
/// geschrieben werden muss, entscheidet der Aufrufer am Vergleich `neu != alt`.
pub fn merge(settings: &Map<String, Value>, bin_dir: &str, opts: MergeOptions) -> (Map<String, Value>, Vec<Note>) {
    let mut out = settings.clone();
    let mut notes = Vec::new();
    let mut hooks = hooks_map(settings);
    let strip = |o: Owner| o == Owner::Deck;
    for (event, status, matcher) in HOOKS {
        let before: Vec<Value> = hooks.get(event).and_then(Value::as_array).cloned().unwrap_or_default();
        let (mut after, removed) = strip_where(&before, strip);
        let mut entry = Map::new();
        if let Some(m) = matcher {
            entry.insert("matcher".into(), m.into());
        }
        entry.insert("hooks".into(), json!([{"type": "command", "command": hook_command(bin_dir, status)}]));
        after.push(Value::Object(entry));
        if after != before {
            hooks.insert(event.into(), Value::Array(after));
            let mut text = format!("{event} -> {REPORT_EXE} {status}");
            if !removed.is_empty() {
                text += &format!(" (ersetzt: {} alter Eintrag)", removed.len());
            }
            notes.push(("info", text));
        }
    }
    if !hooks.is_empty() {
        out.insert("hooks".into(), Value::Object(hooks));
    }

    let cur = settings.get("statusLine").and_then(|s| s.get("command")).and_then(Value::as_str);
    let cur_owner = cur.filter(|c| !crate::pystr::strip(c).is_empty()).map(owner);
    if cur_owner == Some(Owner::Foreign) && !opts.force_statusline {
        notes.push(("warn", "statusLine gehoert einem anderen Werkzeug und bleibt stehen — Modell, Effort und \
                            Kontext-% bleiben auf den Kacheln leer (mit --force ersetzen)".into()));
    } else if !points_here(cur, bin_dir, STATUSLINE_EXE) {
        let sl = json!({"type": "command", "command": statusline_command(bin_dir), "refreshInterval": STATUSLINE_REFRESH_S});
        out.insert("statusLine".into(), sl);
        notes.push(("info", format!("statusLine -> {STATUSLINE_EXE}")));
    } else if let Some(sl) = settings.get("statusLine").and_then(Value::as_object)
        && !sl.contains_key("refreshInterval")
    {
        // Ohne Takt zeichnet Claude Code die Zeile nur bei Ereignissen neu: der Cache-
        // Countdown stünde im Leerlauf still, und `live.json` würde alt, sodass das Deck
        // nie kompaktiert. Ein selbst gesetzter Wert bleibt.
        let mut sl = sl.clone();
        sl.insert("refreshInterval".into(), STATUSLINE_REFRESH_S.into());
        out.insert("statusLine".into(), Value::Object(sl));
        notes.push(("info", format!("statusLine: alle {STATUSLINE_REFRESH_S} s neu zeichnen")));
    }
    (out, notes)
}

fn remove_where(settings: &Map<String, Value>, who: Owner) -> (Map<String, Value>, Vec<Note>) {
    let mut out = settings.clone();
    let mut notes = Vec::new();
    let mut hooks = hooks_map(settings);
    let events: Vec<String> = hooks.keys().cloned().collect();
    for event in events {
        let Some(groups) = hooks.get(&event).and_then(Value::as_array).cloned() else { continue };
        let (rest, removed) = strip_where(&groups, |o| o == who);
        if removed.is_empty() {
            continue;
        }
        notes.push(("info", format!("{event}: {} Eintrag entfernt", removed.len())));
        if rest.is_empty() {
            hooks.shift_remove(&event);
        } else {
            hooks.insert(event, Value::Array(rest));
        }
    }
    if !hooks.is_empty() {
        out.insert("hooks".into(), Value::Object(hooks));
    } else {
        out.shift_remove("hooks");
    }
    let cur = settings.get("statusLine").and_then(|s| s.get("command")).and_then(Value::as_str);
    if cur.is_some_and(|c| owner(c) == who) {
        out.shift_remove("statusLine");
        notes.push(("info", "statusLine entfernt".into()));
    }
    (out, notes)
}

/// Die Einträge der neuen Programme herausnehmen; alles andere bleibt.
pub fn remove(settings: &Map<String, Value>) -> (Map<String, Value>, Vec<Note>) {
    remove_where(settings, Owner::Deck)
}

/// Kommandos eines Events, die `who` gehören.
pub fn commands_of(settings: &Map<String, Value>, event: &str, who: Owner) -> Vec<String> {
    let Some(groups) = settings.get("hooks").and_then(|h| h.get(event)).and_then(Value::as_array) else {
        return Vec::new();
    };
    groups
        .iter()
        .filter_map(|g| g.get("hooks").and_then(Value::as_array))
        .flatten()
        .filter_map(cmd_of)
        .filter(|c| owner(c) == who)
        .map(String::from)
        .collect()
}

/// Ein Urteil: (Grad, Text), Grad in `ok`, `warn`, `fail`.
pub type Finding = (&'static str, String);

/// Urteile über die Einträge. Rein bis auf zwei Dateisystem-Fragen, die als Funktionen
/// hereinkommen: so ist jede Regel ohne Dateisystem prüfbar. Geprüft wird, was am
/// Exit-Code NICHT auffällt: die drei Fallen und ein Pfad in einen anderen Ordner.
///
/// `resolve` löst Windows-Kurznamen auf (`C:\Users\JORRIT~1\…`): `norm_path` allein
/// macht aus Kurz- und Langnamen nicht denselben Text.
pub fn audit(
    settings: &Map<String, Value>,
    bin_dir: &str,
    exists: &dyn Fn(&str) -> bool,
    resolve: &dyn Fn(&str) -> String,
) -> Vec<Finding> {
    static CMD_C: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bcmd\b\s*/c").expect("CMD_C"));
    let mut out: Vec<Finding> = Vec::new();
    let want_dir = norm_path(&resolve(bin_dir));
    for (event, status, _) in HOOKS {
        let cmds = commands_of(settings, event, Owner::Deck);
        let Some(cmd) = cmds.first() else {
            out.push(("fail", format!("{event}: kein Hook eingetragen — Kacheln melden kein '{status}'")));
            continue;
        };
        if cmds.len() > 1 {
            out.push(("warn", format!("{event}: {} Eintraege zeigen auf {REPORT_EXE} — zwei Melder schreiben gegeneinander", cmds.len())));
        }
        if CMD_C.is_match(cmd) {
            out.push(("fail", format!("{event}: 'cmd /c' im Kommando — die POSIX-Shell macht aus /c den Pfad C:\\, \
                                       das Programm wird nie aufgerufen (Exit 0, aber kein Status)")));
        }
        if !cmd.trim_end().ends_with(TAIL) {
            out.push(("fail", format!("{event}: '{TAIL}' fehlt — ein Hook, der nicht startet, blockiert Prompt bzw. Tool-Aufruf")));
        }
        if !format!(" {cmd} ").contains(&format!(" {status} ")) {
            out.push(("warn", format!("{event}: meldet nicht '{status}'")));
        }
        match command_path(cmd) {
            None => out.push(("fail", format!("{event}: kein Programmpfad im Kommando erkennbar"))),
            Some(p) if !exists(&p) => out.push(("fail", format!("{event}: {p} gibt es nicht"))),
            Some(p) if parent_of(&resolve(&p)) != want_dir => {
                out.push(("warn", format!("{event}: zeigt auf einen anderen Ordner ({p})")));
            }
            Some(_) => {}
        }
    }
    let cur = settings.get("statusLine").and_then(|s| s.get("command")).and_then(Value::as_str);
    match cur.filter(|c| !crate::pystr::strip(c).is_empty()) {
        None => out.push(("warn", "statusLine fehlt — Modell, Effort und Kontext-% bleiben auf den Kacheln leer".into())),
        Some(c) => match owner(c) {
            Owner::Foreign => out.push(("warn", "statusLine gehoert einem anderen Werkzeug — Modell, Effort und Kontext-% bleiben leer".into())),
            Owner::Deck => {
                if let Some(p) = command_path(c).filter(|p| !exists(p)) {
                    out.push(("fail", format!("statusLine: {p} gibt es nicht")));
                }
            }
        },
    }
    if out.is_empty() {
        out.push(("ok", format!("{} Hooks und die statusLine zeigen auf {bin_dir}", HOOKS.len())));
    }
    out
}
