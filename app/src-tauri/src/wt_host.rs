//! Windows Terminal als Host für Agenten, das Gegenstück zum Broker
//! (`deck/ops/wt_host.py`). Für Windows Terminal gibt es keine Extension, also
//! beantwortet diese Klasse die Fragen des Decks selbst:
//!
//! | Frage / Kommando | Windows Terminal |
//! |---|---|
//! | welche Slots leben | `.wtpid`-Dateien mit lebender Shell |
//! | createAgent | `wt.exe -w deck-<Slot> new-tab`, danach kacheln |
//! | focusPane | Fenster mit dem Slot als Titel nach vorn |
//! | send / key | Eingabepuffer der Konsole (über `deck-console`) |
//! | closeAgent | Prozessbaum der Pane-Shell beenden |
//! | seen | Vordergrundfenster trägt den Slot-Titel |
//!
//! Welche Buchstaben in Windows Terminal leben, steht in `wt_hosts.json`
//! (`{Buchstabe: Ordner}`). Ein WT-Block gilt als verbunden, solange er dort steht: das
//! Deck kann sein Fenster jederzeit selbst öffnen. Alles, was Fenster oder Prozesse
//! anfasst, läuft hintereinander auf EINEM Arbeiter-Thread ([`Worker`]); ein Klick soll
//! den Takt nicht anhalten.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use deck_core::paths::{WT_HOSTS_FILE, load_json, save_json};
use deck_core::protocol::Command;
use deck_core::wt::{self, Step};
use deck_win::console_input::Step as ConsoleStep;
use deck_win::effects::{ConsoleInput, Desktop, Processes};
use serde_json::Value;

use crate::applog::note;
use crate::worker::Worker;
use crate::wt_tiles::{self, WtTiler};

/// So lange gilt ein angeforderter Slot-Name als vergeben (s).
const PENDING_TTL_S: f64 = 30.0;
/// `terminals()` wird je Takt dutzendfach gefragt (s).
const ALIVE_CACHE_S: f64 = 0.3;
/// So lange gilt die gemessene Fensterlage (s). Ein Blick kostet ein EnumWindows.
const SCREEN_CACHE_S: f64 = 0.5;

/// Startet `wt.exe`. Eigener Trait, damit Tests nie ein Terminal öffnen.
pub trait Launcher: Send + Sync {
    fn launch(&self, argv: &[String]) -> bool;
}

/// Die Buchstaben, die in Windows Terminal leben, geteilt mit dem Fokus-Rahmen.
pub type HostMap = Arc<RwLock<BTreeMap<String, String>>>;

/// Die Wirkungen nach außen, gebündelt.
#[derive(Clone)]
pub struct WtEffects {
    pub desktop: Arc<dyn Desktop>,
    pub procs: Arc<dyn Processes>,
    pub console: Arc<dyn ConsoleInput>,
    pub launcher: Arc<dyn Launcher>,
}

/// Programme und Pfade für das Öffnen eines Agenten.
#[derive(Debug, Clone)]
pub struct WtSetup {
    pub hosts_file: PathBuf,
    pub state_dir: PathBuf,
    pub script: PathBuf,
    pub wt_exe: String,
    pub shell: String,
    /// Verstecktes Profil „Agent Deck“, falls in den WT-Einstellungen vorhanden.
    pub profile: Option<String>,
}

pub struct WtHost {
    setup: WtSetup,
    fx: WtEffects,
    worker: Worker,
    hosts: HostMap,
    pending: HashMap<String, f64>,
    alive: BTreeMap<String, u32>,
    alive_ts: f64,
    last_fg: Option<String>,
    tiler: WtTiler,
    /// Zuletzt gemessene Fensterlage je Block (Zeitpunkt, Reihen, Spalten des Monitors).
    screen: HashMap<String, (f64, Vec<Vec<String>>, u32)>,
    clock: fn() -> f64,
    /// Fenster kacheln, wenn Agenten kommen oder gehen. Im Probebetrieb aus: dort
    /// kachelt noch das Python-Deck, und zwei Decks schöben dieselben Fenster.
    pub retile: bool,
}

/// Eingabe-Schritte aus dem Fachkern in die Form von `deck-console` (Pausen als Dauer).
fn to_console(steps: Vec<Step>) -> Vec<ConsoleStep> {
    steps
        .into_iter()
        .map(|s| match s {
            Step::Text(t) => ConsoleStep::Text(t),
            Step::Sleep(sec) => ConsoleStep::Sleep(std::time::Duration::from_secs_f64(sec.max(0.0))),
        })
        .collect()
}

impl WtHost {
    pub fn new(setup: WtSetup, fx: WtEffects, worker: Worker, clock: fn() -> f64) -> WtHost {
        let hosts: BTreeMap<String, String> = match load_json(&setup.hosts_file) {
            Some(Value::Object(m)) => m.into_iter().filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_string()))).collect(),
            _ => BTreeMap::new(),
        };
        WtHost {
            setup,
            fx,
            worker,
            hosts: Arc::new(RwLock::new(hosts)),
            pending: HashMap::new(),
            alive: BTreeMap::new(),
            alive_ts: -1.0,
            last_fg: None,
            tiler: WtTiler::default(),
            screen: HashMap::new(),
            clock,
            retile: true,
        }
    }

    pub fn host_map(&self) -> HostMap {
        self.hosts.clone()
    }

    fn hosts(&self) -> BTreeMap<String, String> {
        self.hosts.read().map(|h| h.clone()).unwrap_or_default()
    }

    fn save(&self) {
        let m: serde_json::Map<String, Value> = self.hosts().into_iter().map(|(k, v)| (k, Value::from(v))).collect();
        let _ = save_json(&self.setup.hosts_file, &Value::Object(m));
    }

    pub fn has(&self, win: &str) -> bool {
        self.hosts.read().is_ok_and(|h| h.contains_key(win))
    }

    /// Ordner eines WT-Blocks.
    pub fn folder(&self, win: &str) -> Option<String> {
        self.hosts.read().ok().and_then(|h| h.get(win).cloned())
    }

    /// `(Buchstabe, Ordner)` aller WT-Blöcke.
    pub fn blocks(&self) -> Vec<(String, String)> {
        self.hosts().into_iter().collect()
    }

    /// Fenstertitel → Slot, wenn das Fenster ein Agent eines WT-Blocks ist.
    pub fn slot_of_title(hosts: &HostMap, title: &str) -> Option<String> {
        let slot = wt::slot_from_title(Some(title))?;
        let letter: String = slot.chars().take(1).collect();
        hosts.read().ok().filter(|h| h.contains_key(&letter)).map(|_| slot)
    }

    /// `{slot: pid}` jedes Panes, dessen Shell noch läuft. Tote PID-Dateien fliegen
    /// raus: eine liegengebliebene soll nicht Jahre später auf eine fremde PID zeigen.
    pub fn alive_slots(&mut self) -> BTreeMap<String, u32> {
        let now = (self.clock)();
        if (0.0..ALIVE_CACHE_S).contains(&(now - self.alive_ts)) {
            return self.alive.clone();
        }
        let mut out = BTreeMap::new();
        let names = fs::read_dir(&self.setup.state_dir)
            .map(|rd| rd.flatten().filter_map(|e| e.file_name().into_string().ok()).collect::<Vec<_>>())
            .unwrap_or_default();
        for name in names {
            let Some(slot) = name.strip_suffix(wt::PID_SUFFIX) else { continue };
            let path = self.setup.state_dir.join(&name);
            let Some((pid, created)) = load_json(&path).as_ref().and_then(wt::parse_pidfile) else { continue };
            match self.fx.procs.process_start(pid) {
                Some(start) if wt::same_start(start as i64, created) => {
                    out.insert(slot.to_string(), pid);
                }
                _ => {
                    let _ = fs::remove_file(&path);
                }
            }
        }
        self.alive = out.clone();
        self.alive_ts = now;
        self.pending.retain(|n, t| !out.contains_key(n) && now - *t <= PENDING_TTL_S);
        // Neu kacheln, sobald ein Agent dazukommt oder geht. Die PID-Datei schreibt die
        // Shell IM Fenster: es steht also schon, wenn der Slot hier auftaucht.
        let names: Vec<String> = out.keys().cloned().collect();
        for win in self.hosts().into_keys() {
            let slots = wt::slots_of(&win, &names);
            if self.tiler.changed(&win, &slots) && self.retile {
                // Wer schon liegt, behält seinen Platz in der Reihenfolge (zuletzt
                // gemessene Lage), Neue kommen hinten dazu. Sonst sortierte jedes neue
                // Fenster den Bildschirm nach Slot-Namen um.
                let seen: Vec<String> = self.screen.remove(&win).map(|(_, r, _)| r.concat()).unwrap_or_default();
                let mut slots: Vec<String> = seen.into_iter().filter(|s| slots.contains(s)).collect::<Vec<_>>();
                slots.extend(wt::slots_of(&win, &names).into_iter().filter(|s| !slots.contains(s)).collect::<Vec<_>>());
                let d = self.fx.desktop.clone();
                self.worker.run(move || {
                    wt_tiles::tile(d.as_ref(), &slots);
                });
            }
        }
        out
    }

    pub fn connected(&self, win: &str) -> bool {
        self.has(win)
    }

    pub fn terminals(&mut self, win: &str) -> Vec<String> {
        if !self.has(win) {
            return Vec::new();
        }
        let names: Vec<String> = self.alive_slots().into_keys().collect();
        wt::slots_of(win, &names)
    }

    /// Wie die Fenster der Agenten dieses Blocks auf dem Bildschirm liegen, als Reihen
    /// ([`wt_tiles::screen_rows`]), dazu wie viele auf dem Monitor nebeneinander passen
    /// ([`wt_tiles::screen_cols`]). Danach legt das Deck seine Karten und die ＋-Karte.
    pub fn screen_rows(&mut self, win: &str) -> (Vec<Vec<String>>, u32) {
        let now = (self.clock)();
        if let Some((ts, rows, cols)) = self.screen.get(win)
            && (0.0..SCREEN_CACHE_S).contains(&(now - ts))
        {
            return (rows.clone(), *cols);
        }
        let slots = self.terminals(win);
        let rows = if slots.is_empty() { Vec::new() } else { wt_tiles::screen_rows(self.fx.desktop.as_ref(), &slots) };
        let cols = wt_tiles::screen_cols(self.fx.desktop.as_ref(), &rows);
        self.screen.insert(win.to_string(), (now, rows.clone(), cols));
        (rows, cols)
    }

    /// Nach dem Umsortieren im Deck: die Fenster in der neuen Reihenfolge neu kacheln,
    /// damit der Bildschirm der Karte folgt (und nicht die Karte zurückspringt).
    pub fn retile_in(&mut self, win: &str, order: &[String]) {
        let live = self.terminals(win);
        let slots: Vec<String> = order.iter().filter(|s| live.contains(s)).cloned().collect();
        if slots.is_empty() || !self.retile {
            return;
        }
        self.screen.remove(win);
        let d = self.fx.desktop.clone();
        self.worker.run(move || {
            wt_tiles::tile(d.as_ref(), &slots);
        });
    }

    /// Den Block vergessen. Laufende Panes bleiben offen: „vergessen“ heißt wie bei VS
    /// Code, das Deck schaut nicht mehr hin, es schließt nichts.
    pub fn forget(&mut self, win: &str) -> bool {
        let gone = self.hosts.write().map(|mut h| h.remove(win).is_some()).unwrap_or(false);
        if gone {
            self.save();
        }
        gone
    }

    /// Der Agent, dessen Fenster gerade vorne ist.
    pub fn slot_in_front(&self) -> Option<String> {
        let fg = self.fx.desktop.foreground();
        fg.filter(|w| w.class == wt::WINDOW_CLASS).and_then(|w| Self::slot_of_title(&self.hosts, &w.title))
    }

    /// Den Slot melden, dessen Fenster gerade vorne ist, einmal je Wechsel.
    pub fn drain_seen(&mut self) -> BTreeSet<String> {
        let slot = self.slot_in_front();
        if slot == self.last_fg {
            return BTreeSet::new();
        }
        self.last_fg = slot.clone();
        slot.into_iter().collect()
    }

    /// Einen Buchstaben als Windows-Terminal-Block für diesen Ordner anlegen.
    pub fn open_repo(&mut self, win: &str, folder: &str) {
        if let Ok(mut h) = self.hosts.write() {
            h.insert(win.to_string(), folder.to_string());
        }
        self.save();
    }

    /// Ein Kommando wie die Extension ausführen. `true` = angenommen.
    pub fn send(&mut self, win: &str, cmd: &Command) -> bool {
        if !self.has(win) {
            return false;
        }
        match cmd {
            Command::FocusPane { slot } => self.focus(slot),
            Command::Send { slot, text, execute, submit } => {
                let steps = wt::send_steps(text, *execute && !*submit, *submit);
                self.input(slot, steps)
            }
            Command::Key { slot, key, repeat } => self.input(slot, wt::key_steps(key, *repeat)),
            Command::CreateAgent { model, args, cwd, slot } => {
                self.create(win, model.as_deref(), slot.as_deref(), args.as_deref(), cwd.as_deref())
            }
            Command::CloseAgent { slot } => self.close(slot),
            Command::CloseWindow => {
                for s in self.terminals(win) {
                    self.close(&s);
                }
                true
            }
            // Gibt es in Windows Terminal nicht: nichts zu tun.
            Command::Reload | Command::SaveAll => true,
            Command::Assign { .. } | Command::Unassign => false,
        }
    }

    /// Einen Agenten in einem eigenen Fenster `deck-<Slot>` öffnen.
    fn create(&mut self, win: &str, model: Option<&str>, slot: Option<&str>, args: Option<&str>, cwd: Option<&str>) -> bool {
        let Some(folder) = self.folder(win) else { return false };
        let mut taken = self.terminals(win);
        taken.extend(self.pending.keys().cloned());
        let name = wt::wanted_or_next(win, slot, &taken);
        let argv = match wt::claude_argv(model, args) {
            Ok(a) => a,
            Err(e) => {
                note(format!("wt_host.create {name}: Argumente unbrauchbar ({e})"));
                return false;
            }
        };
        self.pending.insert(name.clone(), (self.clock)());
        let s = &self.setup;
        let spec = wt::launch_spec(&name, &s.state_dir.to_string_lossy(), &argv);
        let work_dir = cwd.filter(|c| !c.is_empty()).map_or(folder, String::from);
        let line = wt::wt_argv(&s.wt_exe, &name, &work_dir, &s.shell, &s.script.to_string_lossy(), &spec, s.profile.as_deref());
        let launcher = self.fx.launcher.clone();
        self.worker.run(move || {
            launcher.launch(&line);
        });
        true
    }

    fn focus(&mut self, slot: &str) -> bool {
        if !self.alive_slots().contains_key(slot) {
            return false;
        }
        let (d, slot) = (self.fx.desktop.clone(), slot.to_string());
        self.worker.run(move || {
            if let Some(h) = wt_tiles::hwnd_of(d.as_ref(), &slot) {
                let _ = d.focus(h);
            }
        });
        true
    }

    /// Klick auf den Repo-Namen: alle Fenster des Blocks nach vorn, das des ersten
    /// Agenten zuletzt und damit obenauf.
    pub fn raise_window(&mut self, win: &str) -> bool {
        let slots = self.terminals(win);
        if slots.is_empty() {
            return false;
        }
        let d = self.fx.desktop.clone();
        self.worker.run(move || {
            let mut hwnds = Vec::new();
            for s in &slots {
                if let Some(h) = wt_tiles::hwnd_of(d.as_ref(), s)
                    && !hwnds.contains(&h)
                {
                    hwnds.push(h);
                }
            }
            for h in hwnds.into_iter().rev() {
                let _ = d.focus(h);
            }
        });
        true
    }

    /// Taste „alle nach vorn“ ([`wt_tiles::bring_front`]) für die lebenden unter `slots`.
    /// `false` = keiner lebt.
    pub fn front_all(&mut self, slots: &[String], target: Option<String>, cursor: Option<(i32, i32)>) -> bool {
        let alive = self.alive_slots();
        let slots: Vec<String> = slots.iter().filter(|s| alive.contains_key(*s)).cloned().collect();
        if slots.is_empty() {
            return false;
        }
        let d = self.fx.desktop.clone();
        self.worker.run(move || wt_tiles::bring_front(d.as_ref(), &slots, target.as_deref(), cursor));
        true
    }

    fn input(&mut self, slot: &str, steps: Vec<Step>) -> bool {
        let Some(pid) = self.alive_slots().get(slot).copied() else { return false };
        let (c, steps) = (self.fx.console.clone(), to_console(steps));
        self.worker.run(move || {
            if let Err(e) = c.send(pid, &steps) {
                note(format!("wt_host.input pid={pid}: {e}"));
            }
        });
        true
    }

    /// Den Agenten beenden: die Shell des Panes samt claude (Profil „Agent Deck“ macht
    /// das Fenster dann selbst zu).
    fn close(&mut self, slot: &str) -> bool {
        let Some(pid) = self.alive_slots().get(slot).copied() else { return false };
        let path = self.setup.state_dir.join(format!("{slot}{}", wt::PID_SUFFIX));
        let p = self.fx.procs.clone();
        self.worker.run(move || match p.kill_tree(pid) {
            Ok(()) => {
                let _ = fs::remove_file(&path);
            }
            Err(e) => note(format!("wt_host.close pid={pid}: {e}")),
        });
        self.alive_ts = -1.0;
        true
    }
}

/// Programme suchen wie `shutil.which`: erst der PATH, dann die Vorgabe.
pub fn which(name: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(name)).find(|p| p.is_file()).map(|p| p.to_string_lossy().into_owned())
}

/// Die echte Einrichtung: `wt.exe`, PowerShell 7 (sonst Windows PowerShell) und das
/// Profil „Agent Deck“, wenn es in den Einstellungen von Windows Terminal steht.
pub fn real_setup(app_dir: &std::path::Path, state_dir: &std::path::Path, script: PathBuf) -> WtSetup {
    let local = deck_core::paths::local_app_data();
    let wt_settings = local.join("Packages").join("Microsoft.WindowsTerminal_8wekyb3d8bbwe").join("LocalState").join("settings.json");
    let profile = fs::read_to_string(wt_settings).ok().filter(|t| t.contains(&format!("\"{}\"", wt::PROFILE))).map(|_| wt::PROFILE.to_string());
    WtSetup {
        hosts_file: app_dir.join(WT_HOSTS_FILE),
        state_dir: state_dir.to_path_buf(),
        script,
        wt_exe: which("wt.exe").unwrap_or_else(|| "wt.exe".into()),
        shell: which("pwsh.exe").unwrap_or_else(|| "powershell.exe".into()),
        profile,
    }
}
