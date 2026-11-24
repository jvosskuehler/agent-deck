//! Agenten in WSL: die rechenbaren Teile.
//!
//! In der Team-Ausgabe läuft Claude Code ganz in einer WSL-Distro (Linux-`claude`, Repos
//! im Linux-Dateisystem oder unter `/mnt/c`), nur das Deck läuft unter Windows. Dafür
//! braucht es drei Rechnungen, die hier stehen und getestet sind:
//!
//! * **Pfade hin und her.** Claude meldet Linux-Pfade (`cwd`, `transcript_path`, der
//!   worktree-Marker), das Deck liest mit Windows-Mitteln. `/mnt/c/x` ist `C:\x`, alles
//!   andere liegt unter `\\wsl.localhost\<Distro>\…` ([`Wsl::to_windows`]). Umgekehrt
//!   braucht `wsl.exe --cd` den Linux-Pfad ([`Wsl::to_linux`]).
//! * **Der Start.** Im Fenster läuft `deck-wsl.exe` (Herkunftsmarke `.wtpid`, Umgebung),
//!   der `wsl.exe -d <Distro> --cd <Ordner> -e bash -li <Startskript> <claude-Argumente>`
//!   startet ([`wsl_argv`]). Die Umgebung für die Hooks reicht `WSLENV` durch
//!   ([`wslenv_with`]).
//! * **Was `wsl.exe` antwortet.** `wsl.exe -l` spricht UTF-16 ([`decode_wsl_output`]).
//!
//! Was die Einstellungen dazu sagen, steht in [`AgentEnv`].

use serde_json::{Map, Value};

use crate::deck_settings as ds;

/// Rechnername der WSL-Freigabe. Ältere Fassungen hießen `wsl$`; gelesen wird beides,
/// geschrieben nur dieser Name (so übersetzt auch WSL selbst das cwd eines Windows-Programms).
pub const UNC_HOST: &str = "wsl.localhost";
const UNC_HOST_OLD: &str = "wsl$";
/// Wo WSL die Windows-Laufwerke einhängt, wenn `/etc/wsl.conf` nichts anderes sagt.
pub const DEFAULT_MOUNT_ROOT: &str = "/mnt/";

/// Umgebung, die `deck-wsl.exe` setzt und über `WSLENV` in die Distro und von dort zurück
/// in `deck-report.exe`/`deck-statusline.exe` reicht.
pub const ENV_DISTRO: &str = "AGENT_DECK_DISTRO";
pub const ENV_MOUNT: &str = "AGENT_DECK_MNT";
/// Zustandsordner des Decks (Windows-Pfad). Die Hooks nehmen ihn vor `LOCALAPPDATA`: ein
/// Windows-Programm aus WSL heraus sieht die Windows-Umgebung nur, soweit WSL sie mitgibt.
pub const ENV_STATE: &str = "AGENT_DECK_STATE";
/// Claudes Ordner in der Distro; das Startskript setzt ihn mit `/p`, WSL übersetzt ihn auf
/// dem Rückweg in einen Windows-Pfad.
pub const ENV_CLAUDE_DIR: &str = "AGENT_DECK_CLAUDE_DIR";
/// Was `deck-wsl.exe` in beide Richtungen durchreicht. `AGENT_SLOT` ist der Slot (Vertrag 4),
/// `AGENT_DECK_SHELL` die Herkunftsmarke, `COLORTERM` sagt Claude, dass das Terminal 24 Bit kann.
pub const PASSED: [&str; 6] = ["AGENT_SLOT", "AGENT_DECK_SHELL", ENV_DISTRO, ENV_MOUNT, ENV_STATE, "COLORTERM"];

/// Eine Distro samt dem, was das Deck über sie wissen muss.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Wsl {
    pub distro: String,
    /// `$HOME` in der Distro (`/home/dev`); leer = unbekannt.
    pub home: String,
    /// Einhängepunkt der Laufwerke mit Schluss-`/` (`/mnt/`).
    pub mount_root: String,
}

fn mount_root_norm(root: &str) -> String {
    let r = root.trim();
    if r.is_empty() || !r.starts_with('/') {
        return DEFAULT_MOUNT_ROOT.to_string();
    }
    format!("{}/", r.trim_end_matches('/'))
}

/// `X:` am Anfang (Laufwerksbuchstabe), sonst `None`.
fn drive_of(p: &str) -> Option<char> {
    let mut c = p.chars();
    let d = c.next().filter(char::is_ascii_alphabetic)?;
    (c.next() == Some(':')).then_some(d)
}

impl Wsl {
    pub fn new(distro: &str, home: &str, mount_root: &str) -> Wsl {
        Wsl { distro: distro.trim().to_string(), home: home.trim().trim_end_matches('/').to_string(), mount_root: mount_root_norm(mount_root) }
    }

    /// `\\wsl.localhost\<Distro>`: die Wurzel der Distro für Windows.
    pub fn unc_root(&self) -> String {
        format!(r"\\{UNC_HOST}\{}", self.distro)
    }

    /// Linux-Pfad → Windows-Pfad. `/mnt/c/x` → `C:\x`, `/home/x` →
    /// `\\wsl.localhost\<Distro>\home\x`. Was kein absoluter Linux-Pfad ist (schon ein
    /// Windows-Pfad, relativ, leer), bleibt, wie es ist.
    pub fn to_windows(&self, p: &str) -> String {
        if !p.starts_with('/') {
            return p.to_string();
        }
        if let Some(rest) = p.strip_prefix(self.mount_root.as_str()) {
            let mut it = rest.splitn(2, '/');
            let first = it.next().unwrap_or("");
            let mut chars = first.chars();
            if let (Some(d), None) = (chars.next().filter(char::is_ascii_alphabetic), chars.next()) {
                let tail = it.next().unwrap_or("").replace('/', "\\");
                return format!("{}:\\{tail}", d.to_ascii_uppercase());
            }
        }
        let tail = p.trim_start_matches('/').replace('/', "\\");
        if tail.is_empty() { format!("{}\\", self.unc_root()) } else { format!("{}\\{tail}", self.unc_root()) }
    }

    /// Windows-Pfad → Linux-Pfad, für `wsl.exe --cd` und Prompts an den Agenten. Ein
    /// Linux-Pfad bleibt. `None`: liegt in einer anderen Distro oder ist kein absoluter Pfad.
    pub fn to_linux(&self, p: &str) -> Option<String> {
        let p = p.trim();
        if p.starts_with('/') && !p.starts_with("//") {
            return Some(p.to_string());
        }
        let s = p.replace('\\', "/");
        if let Some(d) = drive_of(&s) {
            let rest = s[2..].trim_start_matches('/').trim_end_matches('/');
            let base = format!("{}{}", self.mount_root, d.to_ascii_lowercase());
            return Some(if rest.is_empty() { base } else { format!("{base}/{rest}") });
        }
        let unc = s.strip_prefix("//")?;
        let (host, rest) = unc.split_once('/')?;
        if !host.eq_ignore_ascii_case(UNC_HOST) && !host.eq_ignore_ascii_case(UNC_HOST_OLD) {
            return None;
        }
        let (distro, rest) = rest.split_once('/').unwrap_or((rest, ""));
        if !distro.eq_ignore_ascii_case(&self.distro) {
            return None;
        }
        let rest = rest.trim_end_matches('/');
        Some(format!("/{rest}"))
    }

    /// Claudes Ordner in der Distro als Linux-Pfad (`$HOME/.claude`); `None` ohne Home.
    pub fn claude_dir_linux(&self) -> Option<String> {
        (!self.home.is_empty()).then(|| format!("{}/.claude", self.home))
    }

    /// Dasselbe als Windows-Pfad (`\\wsl.localhost\<Distro>\home\dev\.claude`).
    pub fn claude_dir(&self) -> Option<String> {
        self.claude_dir_linux().map(|d| self.to_windows(&d))
    }

    /// `~/.claude.json` der Distro (Konto der CLI) als Windows-Pfad.
    pub fn claude_json(&self) -> Option<String> {
        (!self.home.is_empty()).then(|| self.to_windows(&format!("{}/.claude.json", self.home)))
    }

    /// Die Pfadfelder eines Hook-Eingangs (`cwd`, `transcript_path`, `workspace.*`) in
    /// Windows-Pfade umrechnen, damit Zustandsdateien und Deck nur Windows-Pfade sehen.
    pub fn localize_hook_input(&self, data: &mut Map<String, Value>) {
        for key in ["cwd", "transcript_path", "agent_transcript_path"] {
            self.localize_field(data, key);
        }
        if let Some(Value::Object(ws)) = data.get_mut("workspace") {
            for key in ["current_dir", "project_dir"] {
                self.localize_field(ws, key);
            }
        }
    }

    fn localize_field(&self, m: &mut Map<String, Value>, key: &str) {
        if let Some(Value::String(s)) = m.get(key) {
            let w = self.to_windows(s);
            m.insert(key.to_string(), Value::String(w));
        }
    }
}

/// Ein Linux-Pfad aus einer Datei im Repo (etwa `gitdir:` eines worktree, den git in WSL
/// angelegt hat), gelesen über einen Windows-Pfad `base`: liegt `base` in einer Distro, gilt
/// der Linux-Pfad in derselben; `/mnt/c/…` ist immer das Laufwerk. `None`, wenn `linux`
/// kein absoluter Linux-Pfad ist oder sich die Distro nicht aus `base` ergibt.
pub fn beside(base: &str, linux: &str) -> Option<String> {
    if !linux.starts_with('/') {
        return None;
    }
    let s = base.replace('\\', "/");
    let distro = s.strip_prefix("//").and_then(|u| {
        let (host, rest) = u.split_once('/')?;
        (host.eq_ignore_ascii_case(UNC_HOST) || host.eq_ignore_ascii_case(UNC_HOST_OLD)).then(|| rest.split('/').next().unwrap_or("").to_string())
    });
    let wsl = Wsl::new(distro.as_deref().unwrap_or(""), "", DEFAULT_MOUNT_ROOT);
    let w = wsl.to_windows(linux);
    (drive_of(&w).is_some() || distro.is_some_and(|d| !d.is_empty())).then_some(w)
}

/// `WSLENV` um `names` ergänzen (vorne steht, was schon da war; doppelte Namen fallen weg).
/// Ohne Anhängsel gilt eine Variable in beide Richtungen, also auch für die Hooks, die
/// `claude` in der Distro als Windows-Programme startet.
pub fn wslenv_with(existing: Option<&str>, names: &[&str]) -> String {
    let mut parts: Vec<String> = existing.unwrap_or("").split(':').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect();
    for n in names {
        let taken = parts.iter().any(|p| p.split('/').next() == Some(*n));
        if !taken {
            parts.push((*n).to_string());
        }
    }
    parts.join(":")
}

/// Argumente für `wsl.exe` (ohne das Programm): Distro, Ordner, dann das Startskript in
/// einer Login-Shell (`bash -li` lädt `.profile` und `.bashrc`, also auch nvm und
/// `~/.local/bin`, wie ein normales Terminal) mit den Argumenten für `claude`. `-e` statt
/// `--`: ohne Shell dazwischen kommen die Argumente unzerlegt an.
pub fn wsl_argv(distro: &str, cwd: &str, script: &str, args: &[String]) -> Vec<String> {
    let mut v: Vec<String> = vec!["-d".into(), distro.into(), "--cd".into(), cwd.into(), "-e".into(), "bash".into(), "-li".into(), script.into()];
    v.extend(args.iter().cloned());
    v
}

/// Ausgabe von `wsl.exe` als Text: ohne Umleitung des Kodierens spricht es UTF-16LE
/// (mit oder ohne BOM), mit `WSL_UTF8=1` UTF-8. Beides geht.
pub fn decode_wsl_output(raw: &[u8]) -> String {
    let utf16 = raw.len() >= 2 && (raw.starts_with(&[0xFF, 0xFE]) || raw.iter().skip(1).step_by(2).take(8).all(|b| *b == 0));
    let text = if utf16 {
        let units: Vec<u16> = raw.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(raw).into_owned()
    };
    text.trim_start_matches('\u{FEFF}').replace('\0', "")
}

/// Distros zur Wahl, aus `wsl.exe -l -v`: (Name, Standard?). Die internen Distros von
/// Docker Desktop zählen nicht, in ihnen arbeitet niemand.
pub fn parse_distros(list_verbose: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    for (i, line) in list_verbose.lines().enumerate() {
        let line = line.trim_end();
        if i == 0 || line.trim().is_empty() {
            continue; // Kopfzeile „NAME STATE VERSION“ (übersetzt, darum nicht am Text erkannt)
        }
        let default = line.trim_start().starts_with('*');
        let name = line.trim_start().trim_start_matches('*').split_whitespace().next().unwrap_or("");
        if name.is_empty() || name.starts_with("docker-desktop") {
            continue;
        }
        out.push((name.to_string(), default));
    }
    out
}

/// Einhängepunkt der Laufwerke aus `/etc/wsl.conf` (`[automount]` `root = …`), sonst
/// [`DEFAULT_MOUNT_ROOT`].
pub fn mount_root_from_conf(conf: &str) -> String {
    let mut section = String::new();
    for line in conf.lines() {
        let l = line.trim();
        if l.starts_with('[') && l.ends_with(']') {
            section = l[1..l.len() - 1].trim().to_lowercase();
        } else if section == "automount"
            && let Some((k, v)) = l.split_once('=')
            && k.trim().eq_ignore_ascii_case("root")
        {
            return mount_root_norm(v.trim().trim_matches('"'));
        }
    }
    DEFAULT_MOUNT_ROOT.to_string()
}

/// Wo die Agenten laufen.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AgentEnv {
    /// PowerShell und Windows-`claude` (`wt_agent.ps1`).
    #[default]
    Windows,
    Wsl(Wsl),
}

impl AgentEnv {
    /// Aus `deck_settings.json`: `agent_env` = `wsl` mit `wsl_distro` (sonst Windows),
    /// dazu `wsl_home` und `wsl_mount_root`, soweit schon ermittelt.
    pub fn from_settings(s: &Map<String, Value>) -> AgentEnv {
        let distro = ds::text(s, ds::WSL_DISTRO);
        if ds::text(s, ds::AGENT_ENV) != "wsl" || distro.is_empty() {
            return AgentEnv::Windows;
        }
        AgentEnv::Wsl(Wsl::new(&distro, &ds::text(s, ds::WSL_HOME), &ds::text(s, ds::WSL_MOUNT_ROOT)))
    }

    pub fn wsl(&self) -> Option<&Wsl> {
        match self {
            AgentEnv::Wsl(w) => Some(w),
            AgentEnv::Windows => None,
        }
    }

    /// Ein Pfad, wie ihn ein Agent meldet, als Windows-Pfad.
    pub fn to_windows(&self, p: &str) -> String {
        self.wsl().map_or_else(|| p.to_string(), |w| w.to_windows(p))
    }

    /// Ein Windows-Pfad so, wie der Agent ihn versteht (in WSL der Linux-Pfad; geht das
    /// nicht, bleibt er).
    pub fn for_agent(&self, p: &str) -> String {
        self.wsl().and_then(|w| w.to_linux(p)).unwrap_or_else(|| p.to_string())
    }
}
