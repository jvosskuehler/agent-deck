//! Was `deck-wsl.exe` im Fenster eines WSL-Agenten tut, als reine Rechnung.
//!
//! Das Fenster eines Windows-Agenten startet PowerShell mit `wt_agent.ps1`; die Shell
//! schreibt die Herkunftsmarke `<slot>.wtpid` mit IHRER PID. Ein WSL-Agent hat keine
//! Windows-Shell, und `wsl.exe` kann seine PID nicht nennen (der Linux-Prozess sieht sie
//! nicht). Darum steht `deck-wsl.exe` im Fenster: es schreibt die Marke mit seiner eigenen
//! PID, setzt die Umgebung für die Hooks und wartet auf `wsl.exe`. Solange der Agent
//! läuft, lebt es; das Deck sieht daran Leben und Tod des Slots wie bei PowerShell, und
//! Beenden des Prozessbaums nimmt `wsl.exe` mit.
//!
//! Die Angaben kommen als Base64-JSON (`wt::encode_spec`), weil die Zeile durch wt.exe
//! muss, das `;` als Trenner liest.

use serde_json::{Value, json};

use crate::wsl::{self, wsl_argv};

/// Was das Deck dem Starter mitgibt.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WslSpec {
    pub slot: String,
    /// Zustandsordner des Decks (Windows-Pfad).
    pub state: String,
    pub distro: String,
    /// Einhängepunkt der Laufwerke (`/mnt/`).
    pub mount: String,
    /// Arbeitsordner als Linux-Pfad.
    pub cwd: String,
    /// Startskript als Linux-Pfad (`/mnt/c/…/wt_agent.sh`).
    pub script: String,
    /// Argumente für `claude`.
    pub args: Vec<String>,
}

impl WslSpec {
    pub fn to_json(&self) -> Value {
        json!({"slot": self.slot, "state": self.state, "distro": self.distro, "mount": self.mount,
               "cwd": self.cwd, "script": self.script, "args": self.args})
    }

    /// Aus dem JSON; ohne Slot, Zustandsordner, Distro, Ordner oder Skript: `None`.
    pub fn from_json(v: &Value) -> Option<WslSpec> {
        let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty()).map(String::from);
        let args = v.get("args").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect()).unwrap_or_default();
        Some(WslSpec {
            slot: s("slot")?,
            state: s("state")?,
            distro: s("distro")?,
            mount: s("mount").unwrap_or_else(|| wsl::DEFAULT_MOUNT_ROOT.into()),
            cwd: s("cwd")?,
            script: s("script")?,
            args,
        })
    }
}

/// Variablen, die das Deck womöglich aus einer umgebenden Claude-Session erbt. In WSL
/// kämen sie zwar nur über `WSLENV` an, aber die Hooks laufen als Windows-Programme und
/// sähen sie (`CLAUDE_CODE_CHILD_SESSION` schaltet das Transcript ab, `NO_COLOR` die
/// Farben); dieselbe Liste wie in `wt_agent.ps1`.
pub const DROPPED_ENV: [&str; 9] = [
    "CLAUDECODE",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "NO_COLOR",
];

/// Was der Starter ausführt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    /// `state\<slot>.wtpid` und ihr Inhalt (wie bei `wt_agent.ps1`).
    pub pidfile: String,
    pub pidfile_body: String,
    pub set_env: Vec<(String, String)>,
    pub remove_env: Vec<&'static str>,
    /// Argumente für `wsl.exe`.
    pub argv: Vec<String>,
}

/// Den Start rechnen: `pid`/`created` des Starters (FILETIME), `wslenv` = das schon
/// gesetzte `WSLENV`.
pub fn plan(spec: &WslSpec, pid: u32, created: u64, wslenv: Option<&str>) -> LaunchPlan {
    let sep = if spec.state.ends_with(['\\', '/']) { "" } else { "\\" };
    let pidfile = format!("{}{sep}{}{}", spec.state, spec.slot, crate::wt::PID_SUFFIX);
    let pidfile_body = json!({"pid": pid, "created": created, "slot": spec.slot}).to_string();
    let set_env = vec![
        ("AGENT_SLOT".to_string(), spec.slot.clone()),
        ("AGENT_DECK_SHELL".to_string(), format!("{pid}.{created}")),
        (wsl::ENV_DISTRO.to_string(), spec.distro.clone()),
        (wsl::ENV_MOUNT.to_string(), spec.mount.clone()),
        (wsl::ENV_STATE.to_string(), spec.state.clone()),
        ("COLORTERM".to_string(), "truecolor".to_string()),
        ("WSLENV".to_string(), wsl::wslenv_with(wslenv, &wsl::PASSED)),
    ];
    LaunchPlan {
        pidfile,
        pidfile_body,
        set_env,
        remove_env: DROPPED_ENV.to_vec(),
        argv: wsl_argv(&spec.distro, &spec.cwd, &spec.script, &spec.args),
    }
}
