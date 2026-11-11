//! Windows Terminal als Host der Agenten: die rechenbaren Teile.
//!
//! Ein Repo-Block in Windows Terminal ist eine Reihe von Fenstern: jeder Agent bekommt
//! ein EIGENES Fenster `deck-<Slot>`, gleich groß nebeneinander gelegt (Raster in
//! `arrange::tile_grid`). Keine geteilten Panes: Windows Terminal kann Panes nur
//! halbieren (gemessen 2026-11-06: 48 / 24 / 12 / 12 % der Fläche).
//!
//! Drei Verträge, die man nicht raten kann:
//! * Der Pane-TITEL ist der Slot-Name (`--title A3`, gegen Claude festgenagelt mit
//!   `--suppressApplicationTitle`). Daran findet die UI-Automation das Pane wieder.
//! * `AGENT_SLOT` setzt das Startskript im Pane, nicht wt.exe (das kann es nicht).
//! * Die PID-Datei `state\<slot>.wtpid` nennt die Shell des Panes samt Startzeit; die
//!   Startzeit schützt vor einer wiederverwendeten PID.

use serde_json::{Value, json};


pub const WINDOW_PREFIX: &str = "deck-";
/// Verstecktes Profil mit `closeOnExit: always` (sonst bleibt „[process exited …]“).
pub const PROFILE: &str = "Agent Deck";
pub const PID_SUFFIX: &str = ".wtpid";
/// Taskleisten-Kennung der Agentenfenster: eine eigene Gruppe neben den normalen
/// Terminals (`deck_win::taskbar`). Ohne Firmennamen, das Repo ist öffentlich.
pub const TASKBAR_APP_ID: &str = "AgentDeck.Agents";
/// Name dieser Gruppe in Sprungliste und Einstellungen.
pub const TASKBAR_NAME: &str = "Agent Deck";
/// Fensterklasse aller Windows-Terminal-Fenster.
pub const WINDOW_CLASS: &str = "CASCADIA_HOSTING_WINDOW_CLASS";
/// Grund der WT-Schemata `papier-ebenen` (hell) und `papier-ebenen-nacht` (dunkel).
/// Der Fensterrand der Agenten bekommt diese Farbe, damit gekachelte Fenster ohne
/// sichtbare Fuge aneinanderstoßen.
pub const PAPER_LIGHT: u32 = 0xFAF4ED;
pub const PAPER_DARK: u32 = 0x191724;

/// Randfarbe der Agentenfenster; ohne Auskunft über den Modus hell.
pub fn paper_for(light: Option<bool>) -> u32 {
    if light == Some(false) { PAPER_DARK } else { PAPER_LIGHT }
}
/// Pause vor dem eigenen Enter nach einer Paste (sonst verschluckt die TUI das Enter).
pub const SUBMIT_ENTER_DELAY_S: f64 = 0.45;
/// Pause zwischen wiederholten Tasten.
pub const KEY_REPEAT_DELAY_S: f64 = 0.09;
pub const PASTE_START: &str = "\x1b[200~";
pub const PASTE_END: &str = "\x1b[201~";

/// Tasten als Bytes für den Eingabepuffer der Konsole; Unbekanntes geht roh durch.
pub fn keymap(key: &str) -> Option<&'static str> {
    Some(match key {
        "enter" => "\r",
        "esc" => "\x1b",
        "ctrl-c" => "\x03",
        "up" => "\x1b[A",
        "down" => "\x1b[B",
        "right" => "\x1b[C",
        "left" => "\x1b[D",
        "tab" => "\t",
        "shift-tab" => "\x1b[Z",
        "1" => "1",
        "2" => "2",
        "3" => "3",
        "4" => "4",
        "5" => "5",
        _ => return None,
    })
}

/// Ein Eingabe-Schritt in eine fremde Konsole.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Text(String),
    Sleep(f64),
}

/// Name des Agentenfensters (`wt -w deck-A3`); ein unbekannter Name öffnet ein neues.
pub fn window_name(slot: &str) -> String {
    format!("{WINDOW_PREFIX}{slot}")
}

/// `A3` → `("A", 3)`; alles andere → `None`.
pub fn split_slot(name: &str) -> Option<(String, u64)> {
    let mut chars = name.chars();
    let letter = chars.next().filter(char::is_ascii_alphabetic)?;
    let digits = chars.as_str();
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((letter.to_ascii_uppercase().to_string(), digits.parse().ok()?))
}

/// Die Slot-Namen dieses Buchstabens, numerisch sortiert (A2 vor A10).
pub fn slots_of<S: AsRef<str>>(win: &str, names: &[S]) -> Vec<String> {
    let win = win.to_uppercase();
    let mut hits: Vec<(u64, String)> = names
        .iter()
        .filter_map(|n| split_slot(n.as_ref()).filter(|(w, _)| *w == win).map(|(_, i)| (i, n.as_ref().to_string())))
        .collect();
    hits.sort();
    hits.into_iter().map(|(_, n)| n).collect()
}

/// Nächster freier Name: höchster vergebener Index dieses Buchstabens + 1 (also mit
/// Namensrecycling nach dem Schließen des höchsten).
pub fn next_slot<S: AsRef<str>>(win: &str, taken: &[S]) -> String {
    let win = win.to_uppercase();
    let max = taken
        .iter()
        .filter_map(|n| split_slot(n.as_ref()).filter(|(w, _)| *w == win).map(|(_, i)| i))
        .max()
        .unwrap_or(0);
    format!("{win}{}", max + 1)
}

/// Argumente für `claude`. `args` ist eine fertige Flag-Zeile aus
/// `agent_flags::flags` und hat Vorrang; sonst nur `--model <model>`. Zerlegt wie von
/// einer POSIX-Shell, damit die Anführungszeichen wirken. Fehler: offenes Zitat.
pub fn claude_argv(model: Option<&str>, args: Option<&str>) -> Result<Vec<String>, String> {
    if let Some(a) = args.filter(|a| !a.trim().is_empty()) {
        return shlex_split(a);
    }
    Ok(match model.filter(|m| !m.is_empty()) {
        Some(m) => vec!["--model".into(), m.into()],
        None => Vec::new(),
    })
}

/// `shlex.split(s, posix=True)`: Leerraum trennt, `'…'` wörtlich, in `"…"` maskiert
/// `\` nur `"` und `\`, außerhalb maskiert `\` das nächste Zeichen.
pub fn shlex_split(s: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_token = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            // shlex kennt nur diese vier als Trenner (`shlex.whitespace`).
            ' ' | '\t' | '\r' | '\n' => {
                if in_token {
                    out.push(std::mem::take(&mut cur));
                    in_token = false;
                }
            }
            '\'' => {
                in_token = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(x) => cur.push(x),
                        None => return Err("No closing quotation".into()),
                    }
                }
            }
            '"' => {
                in_token = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(x @ ('"' | '\\')) => cur.push(x),
                            Some(x) => {
                                cur.push('\\');
                                cur.push(x);
                            }
                            None => return Err("No closing quotation".into()),
                        },
                        Some(x) => cur.push(x),
                        None => return Err("No closing quotation".into()),
                    }
                }
            }
            '\\' => {
                in_token = true;
                match chars.next() {
                    Some(x) => cur.push(x),
                    None => return Err("No escaped character".into()),
                }
            }
            c => {
                in_token = true;
                cur.push(c);
            }
        }
    }
    if in_token {
        out.push(cur);
    }
    Ok(out)
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(B64[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let vals: Vec<u32> = s
        .bytes()
        .filter(|b| *b != b'=')
        .map(|b| B64.iter().position(|x| *x == b).map(|p| p as u32))
        .collect::<Option<_>>()?;
    let mut out = Vec::new();
    for chunk in vals.chunks(4) {
        let n = chunk.iter().enumerate().fold(0u32, |acc, (i, v)| acc | (v << (18 - 6 * i)));
        for i in 0..chunk.len().saturating_sub(1) {
            out.push((n >> (16 - 8 * i)) as u8);
        }
    }
    Some(out)
}

/// Was das Startskript im Pane wissen muss, als Base64-JSON: Slot, State-Ordner,
/// claude-Argumente. Base64, weil die Zeile durch zwei Parser muss (wt.exe, dann
/// PowerShell) und weder `;` noch Anführungszeichen beide unbeschadet überstehen.
pub fn launch_spec(slot: &str, state_dir: &str, argv: &[String]) -> String {
    let raw = json!({"slot": slot, "state": state_dir, "args": argv}).to_string();
    b64_encode(raw.as_bytes())
}

/// Umkehrung von `launch_spec` (Tests und Diagnose).
pub fn decode_spec(spec: &str) -> Option<Value> {
    let bytes = b64_decode(spec)?;
    serde_json::from_slice(&bytes).ok()
}

/// Rand eines Windows-Terminal-Fensters um das Zeichenraster (Titelleiste, Innenabstand,
/// Rollbalken), Pixel bei 100 %. Gemessen 11.11.2026 an WT 1.24: sichtbare Größe =
/// Zeichen × Zelle + dieser Rand, unabhängig von der Schrift.
pub const CHROME_DIP: (f64, f64) = (36.0, 58.0);
/// Zeichenzelle (Pixel bei 100 %), solange noch kein Fenster vermessen ist. Bewusst
/// groß: ein zu klein geöffnetes Fenster wächst unauffällig in seine Kachel, ein zu
/// großes läge kurz über dem Nachbarn.
pub const CELL_DIP_DEFAULT: (f64, f64) = (10.0, 22.0);

/// Wo ein neues Agentenfenster aufgehen soll (`wt --pos --size`): linke obere Ecke in
/// Bildschirmpixeln, Größe in Zeichen (Pixel nimmt wt.exe nicht).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spot {
    pub x: i64,
    pub y: i64,
    pub cols: i64,
    pub rows: i64,
}

fn sane_scale(scale: f64) -> f64 {
    if scale.is_finite() && scale > 0.1 { scale } else { 1.0 }
}

/// Die Kachel `(x, y, w, h)` in Pixeln als [`Spot`]: so viele ganze Zeichen, wie bei
/// Zellgröße `cell` (Pixel bei 100 %) und Skalierung `scale` hineinpassen.
pub fn spot((x, y, w, h): (i64, i64, i64, i64), scale: f64, cell: (f64, f64)) -> Spot {
    let s = sane_scale(scale);
    let fit = |px: i64, chrome: f64, cell: f64, min: i64| (((px as f64 / s - chrome) / cell.max(1.0) + 1e-6).floor() as i64).max(min);
    Spot { x, y, cols: fit(w, CHROME_DIP.0, cell.0, 20), rows: fit(h, CHROME_DIP.1, cell.1, 5) }
}

/// Zellgröße (Pixel bei 100 %) aus einem Fenster, das mit `spot` aufging und sichtbar
/// `(w, h)` Pixel groß wurde. `None`, wenn das Ergebnis keine Schrift sein kann: dann
/// hat jemand das Fenster schon gezogen, oder Windows Terminal hat es eingepasst.
pub fn learn_cell(spot: &Spot, (w, h): (i64, i64), scale: f64) -> Option<(f64, f64)> {
    let s = sane_scale(scale);
    let cw = (w as f64 / s - CHROME_DIP.0) / spot.cols.max(1) as f64;
    let ch = (h as f64 / s - CHROME_DIP.1) / spot.rows.max(1) as f64;
    ((4.0..=24.0).contains(&cw) && (8.0..=48.0).contains(&ch)).then_some((cw, ch))
}

/// Kommandozeile für einen neuen Agenten in einem eigenen Fenster. Mit `spot` geht es
/// gleich an seinem Platz im Raster auf, statt irgendwo und dann dorthin zu springen.
#[allow(clippy::too_many_arguments)]
pub fn wt_argv(
    wt_exe: &str,
    slot: &str,
    cwd: &str,
    shell: &str,
    script: &str,
    spec: &str,
    profile: Option<&str>,
    spot: Option<&Spot>,
) -> Vec<String> {
    let mut argv: Vec<String> = vec![wt_exe.into(), "-w".into(), window_name(slot)];
    if let Some(s) = spot {
        // Mit `=`: eine negative Koordinate (Monitor links der Hauptanzeige) läse wt.exe
        // sonst als Schalter.
        argv.push(format!("--pos={},{}", s.x, s.y));
        argv.push(format!("--size={},{}", s.cols, s.rows));
    }
    argv.push("new-tab".into());
    if let Some(p) = profile.filter(|p| !p.is_empty()) {
        argv.extend(["-p".into(), p.into()]);
    }
    for a in ["--title", slot, "--suppressApplicationTitle", "-d", cwd, shell, "-NoLogo", "-NoExit",
        "-ExecutionPolicy", "Bypass", "-File", script, "-Spec", spec]
    {
        argv.push(a.into());
    }
    argv
}

/// Fenstertitel eines Windows-Terminal-Fensters → Slot des aktiven Panes, oder `None`
/// (fremdes Pane, Einstellungen …). Der Titel IST der Slot-Name.
pub fn slot_from_title(title: Option<&str>) -> Option<String> {
    let (w, i) = split_slot(title.unwrap_or("").trim())?;
    Some(format!("{w}{i}"))
}

/// Inhalt einer `.wtpid`-Datei → `(pid, startzeit als FILETIME)`; nur vollständig.
pub fn parse_pidfile(data: &Value) -> Option<(u32, i64)> {
    let int = |v: Option<&Value>| -> Option<i64> {
        match v? {
            Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f.trunc() as i64)),
            Value::String(s) => s.trim().parse().ok(),
            _ => None,
        }
    };
    let pid = int(data.get("pid"))?;
    let created = int(data.get("created"))?;
    (pid > 0).then(|| (u32::try_from(pid).ok(), created)).and_then(|(p, c)| p.map(|p| (p, c)))
}

/// Zwei FILETIME-Startzeiten (100-ns-Schritte) gelten als gleich, wenn sie weniger als
/// eine Sekunde auseinanderliegen: PowerShell rechnet sie über DateTime um.
pub fn same_start(a: i64, b: i64) -> bool {
    (a - b).abs() < 10_000_000
}

/// Eingabe-Schritte für `send`: `submit` → Text als Bracketed Paste, Pause, dann ein
/// EIGENES Enter (ein mitgeschicktes ginge in der Paste unter); sonst Text plus Enter.
pub fn send_steps(text: &str, execute: bool, submit: bool) -> Vec<Step> {
    if submit {
        return vec![
            Step::Text(format!("{PASTE_START}{text}{PASTE_END}")),
            Step::Sleep(SUBMIT_ENTER_DELAY_S),
            Step::Text("\r".into()),
        ];
    }
    vec![Step::Text(format!("{text}{}", if execute { "\r" } else { "" }))]
}

/// Eingabe-Schritte für `key`: bekannte Namen über `keymap`, sonst roh.
pub fn key_steps(key: &str, repeat: u32) -> Vec<Step> {
    let seq = keymap(key).unwrap_or(key);
    let mut steps = Vec::new();
    for i in 0..repeat.max(1) {
        if i > 0 {
            steps.push(Step::Sleep(KEY_REPEAT_DELAY_S));
        }
        steps.push(Step::Text(seq.into()));
    }
    steps
}
