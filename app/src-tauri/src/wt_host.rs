//! Windows Terminal als Host der Agenten (`deck/ops/wt_host.py`). Jede Frage und jedes
//! Kommando des Decks landet hier:
//!
//! | Frage / Kommando | Windows Terminal |
//! |---|---|
//! | welche Slots leben | `.wtpid`-Dateien mit lebender Shell |
//! | [`WtHost::create_agent`] | `wt.exe -w deck-<Slot> new-tab` an seinem Platz im Raster, danach kacheln |
//! | [`WtHost::focus_pane`] | Fenster mit dem Slot als Titel nach vorn |
//! | [`WtHost::send_text`] / [`WtHost::key`] | Eingabepuffer der Konsole (über `deck-console`) |
//! | [`WtHost::close_agent`] | Prozessbaum der Pane-Shell beenden |
//! | Reserve-Agent (nur auf Wunsch einer Erweiterung) | versteckt vorgewärmtes Fenster, das [`WtHost::create_agent`] übernimmt (`wt_reserve.rs`) |
//! | gesehen | Vordergrundfenster trägt den Slot-Titel |
//!
//! Welche Buchstaben in Windows Terminal leben, steht in `wt_hosts.json`
//! (`{Buchstabe: Ordner}`). Ein WT-Block gilt als verbunden, solange er dort steht: das
//! Deck kann sein Fenster jederzeit selbst öffnen. Alles, was Fenster oder Prozesse
//! anfasst, läuft hintereinander auf EINEM Arbeiter-Thread ([`Worker`]); ein Klick soll
//! den Takt nicht anhalten.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use deck_core::crash_resume::Ended;
use deck_core::paths::{load_json, save_json};
use deck_core::wt::{self, Step};
use deck_win::Rect;
use deck_win::effects::Desktop;
use deck_win::console_input::Step as ConsoleStep;
use serde_json::Value;

use crate::applog::note;
use crate::worker::Worker;
use crate::wt_setup::{WtEffects, WtSetup};
use crate::wt_tiles::{self, WtTiler};

#[path = "wt_reserve.rs"]
mod reserve;
pub use reserve::ReserveState;

/// So lange gilt ein angeforderter Slot-Name als vergeben (s).
const PENDING_TTL_S: f64 = 30.0;
/// `terminals()` wird je Takt dutzendfach gefragt (s).
const ALIVE_CACHE_S: f64 = 0.3;
/// Nach einem Wechsel im Raster wird so oft (s danach) noch einmal nachgelegt. Ein
/// frisches Fenster setzt seine Größe manchmal selbst noch einmal (Anfangsgröße in ganzen
/// Zeichen, DPI-Wechsel, Rand weg): ohne Nachlegen bliebe es dann ein paar Pixel zu klein.
const SETTLE_AFTER_S: [f64; 2] = [1.0, 3.0];
/// So lange wartet der Vorhang höchstens auf das Fenster eines neuen Agenten.
const OPEN_WAIT: Duration = Duration::from_secs(5);
/// Legen unter dem Vorhang: höchstens so viele Züge, je mit [`OPEN_PAINT`] Zeit zum
/// Nachziehen und Zeichnen; Schluss, sobald nach einem Zug alle Fenster auf ihrer Kachel
/// liegen ([`place_until_still`]). Früher immer drei bzw. zwei Züge, also bis zu 360 ms
/// Standbild ohne Grund.
const OPEN_ROUNDS: usize = 3;
const OPEN_PAINT: Duration = Duration::from_millis(120);
/// So lange wartet der Vorhang beim Schließen höchstens, bis das Fenster weg ist.
const CLOSE_WAIT: Duration = Duration::from_secs(2);

/// Unter dem Vorhang ([`Desktop::curtain`]) über dem Monitor von `around` arbeiten:
/// `work` legt die Fenster, danach folgen Ringe, Schatten und Glow, dann geht der Vorhang
/// weg. Ohne Vorhang (geht nicht, Attrappe) bekommt `work` `None` statt der Wartefunktion
/// und macht es so schlicht wie möglich.
fn under_curtain(d: &dyn Desktop, around: Option<Rect>, hold: &FrameHold, work: impl FnOnce(Option<&dyn Fn(Duration)>)) {
    let curtain = around.and_then(|r| wt_tiles::monitor_of(d, r)).and_then(|m| d.curtain(&[m]));
    let Some(curtain) = curtain else {
        work(None);
        return;
    };
    let holding = Holding::start(hold);
    work(Some(&|t| curtain.wait(t)));
    // Ringe, Schatten und Glow folgen den Fenstern, sobald der Halt weg ist; gelüftet wird,
    // wenn sie nachgezogen haben (zwei Durchgänge, höchstens 400 ms), nicht nach fester Zeit.
    crate::group_shadow::catch_up(hold, &|t| curtain.wait(t));
    drop(holding);
    curtain.lift();
}

/// Den Block in der Reihenfolge `slots` kacheln; muss dabei ein Fenster sichtbar umziehen,
/// unter dem Vorhang (zweimal gelegt: ein Terminal setzt seine Größe gern noch einmal).
fn retile_veiled(d: &dyn Desktop, slots: &[String], hold: &FrameHold) {
    let plan = wt_tiles::tile_plan(d, slots, 0);
    if !wt_tiles::needs_moving(d, &plan) {
        wt_tiles::tile(d, slots, 0);
        return;
    }
    let around = plan.first().and_then(|(h, _)| d.window_rect(*h));
    under_curtain(d, around, hold, |wait| match wait {
        Some(wait) => place_until_still(d, slots, wait, 2),
        None => {
            wt_tiles::tile(d, slots, 0);
        }
    });
}

/// Unter dem Vorhang kacheln, bis nach einem Zug (und [`OPEN_PAINT`] zum Zeichnen) jedes
/// Fenster auf seiner Kachel liegt, höchstens `rounds` Züge. Ein Terminal setzt seine
/// Größe gern noch einmal selbst (ganze Zeichen, Rand weg): dann gibt es den nächsten Zug.
fn place_until_still(d: &dyn Desktop, slots: &[String], wait: &dyn Fn(Duration), rounds: usize) {
    for _ in 0..rounds {
        wt_tiles::tile(d, slots, 0);
        wait(OPEN_PAINT);
        if !wt_tiles::needs_moving(d, &wt_tiles::tile_plan(d, slots, 0)) {
            return;
        }
    }
}

/// So lange gilt die gemessene Fensterlage (s). Ein Blick kostet ein EnumWindows.
const SCREEN_CACHE_S: f64 = 0.5;
/// Hier merkt sich das Deck die vermessene Zeichenzelle der Agentenfenster (neben
/// `wt_hosts.json`), damit auch das erste Fenster nach einem Neustart passt.
const CELL_FILE: &str = "wt_cell.json";

/// Die Buchstaben, die in Windows Terminal leben, geteilt mit dem Fokus-Rahmen.
pub type HostMap = Arc<RwLock<BTreeMap<String, String>>>;

/// Gesetzt, solange das Deck mehrere Agentenfenster hintereinander anfasst (alle holen,
/// alle verstecken, Block nach vorn). Der Fokus-Rahmen wartet so lange: sonst spränge er
/// sichtbar von Fenster zu Fenster, bis das letzte vorn liegt.
pub type FrameHold = Arc<AtomicBool>;

/// Hält [`FrameHold`] bis zum Ende des Auftrags, auch wenn der abstürzt.
struct Holding(FrameHold);

impl Holding {
    fn start(hold: &FrameHold) -> Holding {
        hold.store(true, Ordering::SeqCst);
        Holding(hold.clone())
    }
}

impl Drop for Holding {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

pub struct WtHost {
    setup: WtSetup,
    fx: WtEffects,
    worker: Worker,
    hosts: HostMap,
    pending: HashMap<String, f64>,
    alive: BTreeMap<String, u32>,
    /// Startzeit der Shell je lebendem Slot, Unix-Sekunden.
    started: BTreeMap<String, f64>,
    alive_ts: f64,
    last_fg: Option<String>,
    tiler: WtTiler,
    /// Zuletzt gemessene Fensterlage je Block (Zeitpunkt, Reihen, Spalten des Monitors).
    screen: HashMap<String, (f64, Vec<Vec<String>>, u32)>,
    frame_hold: FrameHold,
    /// Angeforderte Fenster, die mit Vorgabe aufgehen: daran wird die Zelle vermessen.
    spots: HashMap<String, wt::Spot>,
    /// Zeichenzelle der Agentenfenster, Pixel bei 100 % ([`wt::learn_cell`]).
    cell: (f64, f64),
    clock: fn() -> f64,
    /// Slots, die das Deck gerade selbst schließt: ihr Tod ist kein Ereignis.
    closing: BTreeSet<String>,
    /// Pane-Shells, die seit dem letzten [`Self::drain_ended`] gestorben sind.
    ended: Vec<Ended>,
    /// Schon einmal nachgesehen? Was beim allerersten Blick tot ist, starb vor diesem Deck.
    scanned: bool,
    /// Fällige Nachlege-Durchgänge je Block ([`SETTLE_AFTER_S`]).
    settle: Vec<(String, f64)>,
    /// Farbschema neuer Agentenfenster bei festem Hell/Dunkel (`--colorScheme`); `None` =
    /// das Profil wählt nach dem Thema von Windows Terminal.
    scheme: Option<&'static str>,
    /// Monitore beim letzten Blick ([`deck_core::arrange::resized_in_place`]).
    screens: Vec<deck_core::arrange::Screen>,
    /// Der versteckt wartende Reserve-Agent (`wt_reserve.rs`). Er zählt nirgends als
    /// lebender Slot, bis [`Self::create_agent`] ihn übernimmt.
    reserve: ReserveState,
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
        let num = |v: &Value, k: &str| v.get(k).and_then(Value::as_f64).filter(|n| n.is_finite() && *n >= 1.0);
        let cell = load_json(&setup.hosts_file.with_file_name(CELL_FILE))
            .and_then(|v| Some((num(&v, "w")?, num(&v, "h")?)))
            .unwrap_or(wt::CELL_DIP_DEFAULT);
        let reserve = ReserveState::load(&setup.hosts_file);
        WtHost {
            setup,
            fx,
            worker,
            hosts: Arc::new(RwLock::new(hosts)),
            pending: HashMap::new(),
            alive: BTreeMap::new(),
            started: BTreeMap::new(),
            alive_ts: -1.0,
            last_fg: None,
            tiler: WtTiler::default(),
            screen: HashMap::new(),
            frame_hold: FrameHold::default(),
            spots: HashMap::new(),
            cell,
            clock,
            closing: BTreeSet::new(),
            ended: Vec::new(),
            scanned: false,
            settle: Vec::new(),
            scheme: None,
            screens: Vec::new(),
            reserve,
        }
    }

    /// Farbschema für neue Agentenfenster (`deck_core::appearance::Appearance::wt_scheme`).
    /// Offene Fenster behalten ihres bis zum nächsten Start: Windows Terminal kann das
    /// Schema eines laufenden Fensters von außen nicht umstellen.
    pub fn set_color_scheme(&mut self, scheme: Option<&'static str>) {
        self.scheme = scheme;
    }

    pub fn host_map(&self) -> HostMap {
        self.hosts.clone()
    }

    pub fn frame_hold(&self) -> FrameHold {
        self.frame_hold.clone()
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
        let mut started = BTreeMap::new();
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
                    started.insert(slot.to_string(), wt::filetime_unix(created));
                }
                _ => {
                    let _ = fs::remove_file(&path);
                    let by_deck = self.closing.remove(slot);
                    // Die Reserve war nie ein Agent: ihr Tod ist kein Absturz.
                    if !self.reserve.is(slot) {
                        self.ended.push(Ended { slot: slot.to_string(), by_deck, at_start: !self.scanned });
                    }
                }
            }
        }
        self.reap_doomed(&mut out, &mut started, now);
        // Die versteckte Reserve lebt, ist aber (noch) kein Agent: raus aus der Liste.
        self.reserve.pid = self.reserve.slot().and_then(|r| out.remove(r));
        if let Some(r) = self.reserve.slot() {
            started.remove(r);
        }
        self.note_new_agents(&out, now);
        self.scanned = true;
        self.alive = out.clone();
        self.started = started;
        self.alive_ts = now;
        self.pending.retain(|n, t| !out.contains_key(n) && now - *t <= PENDING_TTL_S);
        self.measure_cell(&out);
        // Neu kacheln, sobald ein Agent dazukommt oder geht. Die PID-Datei schreibt die
        // Shell IM Fenster: es steht also schon, wenn der Slot hier auftaucht. Gehen zählt
        // schon, wenn das Fenster weg ist: die Shell stirbt erst eine gute Sekunde später
        // (Claude räumt auf), so lange stünde sonst ein Loch im Raster.
        let windows: BTreeSet<String> = self
            .fx
            .desktop
            .class_windows(wt::WINDOW_CLASS)
            .into_iter()
            .filter_map(|w| wt::slot_from_title(Some(&w.title)))
            .collect();
        let names: Vec<String> = out.keys().filter(|s| windows.contains(*s)).cloned().collect();
        for win in self.hosts().into_keys() {
            let slots = wt::slots_of(&win, &names);
            if self.tiler.changed(&win, &slots) {
                // Wer schon liegt, behält seinen Platz in der Reihenfolge (zuletzt
                // gemessene Lage), Neue kommen hinten dazu. Sonst sortierte jedes neue
                // Fenster den Bildschirm nach Slot-Namen um.
                let seen: Vec<String> = self.screen.remove(&win).map(|(_, r, _)| r.concat()).unwrap_or_default();
                let mut slots: Vec<String> = seen.into_iter().filter(|s| slots.contains(s)).collect::<Vec<_>>();
                slots.extend(wt::slots_of(&win, &names).into_iter().filter(|s| !slots.contains(s)).collect::<Vec<_>>());
                let d = self.fx.desktop.clone();
                let hold = self.frame_hold.clone();
                self.worker.run(move || {
                    retile_veiled(d.as_ref(), &slots, &hold);
                });
                self.settle.retain(|(w, _)| *w != win);
                self.settle.extend(SETTLE_AFTER_S.iter().map(|dt| (win.clone(), now + dt)));
            }
        }
        self.watch_screens(now);
        self.settle_due(now, &names);
        out
    }

    /// Neue Agenten ins Protokoll (Slot und PID, nichts vom Inhalt); beim ersten Blick
    /// die schon laufenden in einer Zeile.
    fn note_new_agents(&self, out: &BTreeMap<String, u32>, now: f64) {
        if !self.scanned {
            if !out.is_empty() {
                note(format!("Agenten beim Start: {}", out.keys().cloned().collect::<Vec<_>>().join(", ")));
            }
            return;
        }
        for (slot, pid) in out.iter().filter(|(s, _)| !self.alive.contains_key(*s)) {
            match self.pending.get(slot) {
                Some(t) => note(format!("Agent {slot} läuft (pid {pid}, {:.1} s nach dem Öffnen)", now - t)),
                None => note(format!("Agent {slot} läuft (pid {pid})")),
            }
        }
    }

    /// Hat ein Monitor eine neue Auflösung oder Skalierung, liegen die Fenster an ihren
    /// alten Pixeln: dann jeden Block gleich einmal nachkacheln (unter dem Vorhang, wie
    /// jeder Umzug). Ein Monitor mehr oder weniger zählt nicht ([`resized_in_place`]).
    ///
    /// [`resized_in_place`]: deck_core::arrange::resized_in_place
    fn watch_screens(&mut self, now: f64) {
        let screens: Vec<deck_core::arrange::Screen> = self
            .fx
            .desktop
            .monitors()
            .into_iter()
            .map(|m| (m.device, (i64::from(m.work.left), i64::from(m.work.top), i64::from(m.work.right), i64::from(m.work.bottom)), m.scale))
            .collect();
        if deck_core::arrange::resized_in_place(&self.screens, &screens) {
            note("Anzeige geändert (Auflösung oder Skalierung): Blöcke werden neu gekachelt");
            for win in self.hosts().into_keys() {
                self.settle.retain(|(w, _)| *w != win);
                self.settle.push((win, now));
            }
        }
        self.screens = screens;
    }

    /// Fällige Nachlege-Durchgänge: jeden Block in seiner jetzigen Reihenfolge noch einmal
    /// kacheln. Wer schon liegt, wo er soll, bleibt unberührt liegen (gleiche Lage).
    fn settle_due(&mut self, now: f64, names: &[String]) {
        let due: BTreeSet<String> = self.settle.iter().filter(|(_, t)| *t <= now).map(|(w, _)| w.clone()).collect();
        if due.is_empty() {
            return;
        }
        self.settle.retain(|(_, t)| *t > now);
        for win in due {
            let live = wt::slots_of(&win, names);
            if live.is_empty() {
                continue;
            }
            let d = self.fx.desktop.clone();
            let hold = self.frame_hold.clone();
            self.screen.remove(&win);
            self.worker.run(move || {
                let lying: Vec<String> = wt_tiles::screen_rows(d.as_ref(), &live).concat();
                let order = deck_core::arrange::keep_order(&lying, &live, None);
                // Muss doch noch ein Fenster umziehen, dann wie jeder Umzug unter dem Vorhang.
                retile_veiled(d.as_ref(), &order, &hold);
            });
        }
    }

    /// Ein eben aufgegangenes Fenster vermessen, bevor das Raster es anfasst: aus der
    /// angeforderten Zeichenzahl und seiner Größe folgt die Zelle, und das nächste
    /// Fenster geht pixelgenau in Kachelgröße auf.
    fn measure_cell(&mut self, alive: &BTreeMap<String, u32>) {
        let fresh: Vec<String> = self.spots.keys().filter(|s| alive.contains_key(*s)).cloned().collect();
        for slot in fresh {
            let Some(spot) = self.spots.remove(&slot) else { continue };
            let d = self.fx.desktop.as_ref();
            let Some(h) = wt_tiles::hwnd_of(d, &slot) else { continue };
            let Some(r) = d.frame_rect(h) else { continue };
            let Some(cell) = wt::learn_cell(&spot, (i64::from(r.width()), i64::from(r.height())), d.scale_of(h)) else { continue };
            if cell != self.cell {
                self.cell = cell;
                let _ = save_json(&self.setup.hosts_file.with_file_name(CELL_FILE), &serde_json::json!({"w": cell.0, "h": cell.1}));
            }
        }
        let pending = &self.pending;
        self.spots.retain(|n, _| pending.contains_key(n));
    }

    /// Wo das nächste Fenster dieses Blocks aufgehen soll: in der Kachel, die ihm das
    /// Raster gleich gibt ([`wt_tiles::next_tile`]). Dazu die liegenden Agenten in
    /// Kachel-Reihenfolge und wie viele angeforderte vor ihm dran sind: damit rückt
    /// [`Self::create_agent`] sie vorab beiseite. Spot `None` = ohne Vorgabe öffnen.
    fn next_spot(&mut self, win: &str) -> (Option<(wt::Spot, Rect)>, Vec<String>, Vec<String>) {
        let live = self.terminals(win);
        let mut order: Vec<String> = self.screen_rows(win).0.concat();
        order.extend(live.into_iter().filter(|s| !order.contains(s)).collect::<Vec<_>>());
        let mut ahead: Vec<String> =
            self.pending.keys().filter(|n| wt::split_slot(n).is_some_and(|(w, _)| w == win)).cloned().collect();
        ahead.sort();
        let spot = wt_tiles::next_tile(self.fx.desktop.as_ref(), &order, ahead.len()).map(|(r, scale)| {
            let sp = wt::spot((i64::from(r.left), i64::from(r.top), i64::from(r.width()), i64::from(r.height())), scale, self.cell);
            (sp, r)
        });
        (spot, order, ahead)
    }

    /// Wann die Shell jedes lebenden Agenten gestartet ist (Unix-Sekunden).
    pub fn started(&mut self) -> BTreeMap<String, f64> {
        self.alive_slots();
        self.started.clone()
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

    /// Die freie Kachel neben den Fenstern dieses Blocks ([`crate::wt_gap::gap`]), mit den
    /// Fenstern in der Reihenfolge, in der sie gerade liegen.
    pub fn gap(&mut self, win: &str) -> Option<crate::wt_gap::Gap> {
        let order: Vec<String> = self.screen_rows(win).0.concat();
        if order.is_empty() {
            return None;
        }
        crate::wt_gap::gap(self.fx.desktop.as_ref(), &order)
    }

    /// Nach dem Umsortieren im Deck: die Fenster in der neuen Reihenfolge neu kacheln,
    /// damit der Bildschirm der Karte folgt (und nicht die Karte zurückspringt).
    pub fn retile_in(&mut self, win: &str, order: &[String]) {
        let live = self.terminals(win);
        let slots: Vec<String> = order.iter().filter(|s| live.contains(s)).cloned().collect();
        if slots.is_empty() {
            return;
        }
        self.screen.remove(win);
        let (d, hold) = (self.fx.desktop.clone(), self.frame_hold.clone());
        self.worker.run(move || {
            let plan = wt_tiles::tile_plan(d.as_ref(), &slots, 0);
            if !wt_tiles::only_moves(d.as_ref(), &plan) {
                retile_veiled(d.as_ref(), &slots, &hold);
                return;
            }
            // Umsortieren tauscht Plätze gleich großer Kacheln: kein Terminal muss neu
            // zeichnen, die Fenster springen im selben Bild (gemessen 24.11.2026 mit
            // tools/probe, auch unter Last). Ein Vorhang kostete hier nur Zeit (280 → 680 ms
            // bis zur Ruhe). Ringe und Schatten warten, bis alle liegen, und ziehen dann in
            // einem Schritt nach, statt jedem Fenster einzeln.
            let _hold = Holding::start(&hold);
            wt_tiles::tile(d.as_ref(), &slots, 0);
        });
    }

    /// Den Block vergessen. Laufende Panes bleiben offen: „vergessen“ heißt, das Deck
    /// schaut nicht mehr hin, es schließt nichts.
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

    /// Text an den Agenten. `submit`: Text schreiben und per EIGENEM Enter abschicken
    /// (ein langer Prompt kommt als Paste an, ein mitgeschicktes Enter würde
    /// verschluckt); sonst wird die Zeile direkt ausgeführt (Slash-Kommandos).
    /// `false` = der Slot lebt nicht.
    pub fn send_text(&mut self, slot: &str, text: &str, submit: bool) -> bool {
        self.input(slot, wt::send_steps(text, !submit, submit))
    }

    /// Eine Taste (`enter`, `esc`, `shift-tab` …), `repeat`-mal.
    pub fn key(&mut self, slot: &str, key: &str, repeat: u32) -> bool {
        self.input(slot, wt::key_steps(key, repeat))
    }

    /// Einen Agenten in einem eigenen Fenster `deck-<Slot>` im Ordner des Blocks öffnen.
    /// `args` ist die fertige Flag-Zeile (`agent_flags::flags`). `true` = angenommen.
    pub fn create_agent(&mut self, win: &str, model: Option<&str>, args: Option<&str>) -> bool {
        let Some(folder) = self.folder(win) else { return false };
        let name = wt::next_slot(win, &self.taken(win));
        let argv = match wt::claude_argv(model, args) {
            Ok(a) => a,
            Err(e) => {
                note(format!("wt_host.create {name}: Argumente unbrauchbar ({e})"));
                return false;
            }
        };
        if self.claim_reserve(win, &folder, &argv) {
            return true;
        }
        self.open_agent(win, &name, &folder, &argv);
        true
    }

    /// Vergebene Slot-Namen eines Blocks: lebende, angeforderte und die der Reserve
    /// (auch einer eben beendeten, deren Shell noch kommen kann).
    fn taken(&mut self, win: &str) -> Vec<String> {
        let mut taken = self.terminals(win);
        taken.extend(self.pending.keys().cloned());
        taken.extend(self.reserve.slot().map(String::from));
        taken.extend(self.reserve.doomed_slots().cloned());
        taken
    }

    /// Tote Pane-Shells seit dem letzten Aufruf (`deck_core::crash_resume`).
    pub fn drain_ended(&mut self) -> Vec<Ended> {
        self.alive_slots();
        std::mem::take(&mut self.ended)
    }

    /// Eine abgestürzte Session wieder öffnen: möglichst unter ihrem alten Slot-Namen, im
    /// Ordner `cwd` (falls es ihn gibt, sonst im Ordner des Blocks), mit der Flag-Zeile
    /// `args` und auf Wunsch einem ersten Prompt. Liefert den Slot-Namen, `None` = nicht
    /// gestartet.
    pub fn resume_agent(&mut self, win: &str, prefer: &str, cwd: Option<&str>, args: &str, prompt: Option<&str>) -> Option<String> {
        let block = self.folder(win)?;
        let taken = self.taken(win);
        let fits = wt::split_slot(prefer).is_some_and(|(w, _)| w == win) && !taken.iter().any(|t| t == prefer);
        let name = if fits { prefer.to_string() } else { wt::next_slot(win, &taken) };
        let folder = cwd.filter(|c| std::path::Path::new(c).is_dir()).map_or(block, String::from);
        let mut argv = match wt::claude_argv(None, Some(args)) {
            Ok(a) => a,
            Err(e) => {
                note(format!("wt_host.resume {name}: Argumente unbrauchbar ({e})"));
                return None;
            }
        };
        argv.extend(prompt.map(String::from));
        self.open_agent(win, &name, &folder, &argv);
        Some(name)
    }

    /// Ein Agentenfenster `deck-<name>` in `folder` öffnen, an seinem Platz im Raster.
    ///
    /// Unter dem Vorhang ([`Desktop::curtain`], dasselbe Standbild wie bei „alle nach
    /// vorn“): die liegenden rücken beiseite, das neue geht auf, alle bekommen ihre
    /// Kachel, erst dann geht der Vorhang weg. Man sieht den alten Block stehen und dann
    /// den neuen auf einmal, ohne Überlappen und ohne Nachruckeln.
    fn open_agent(&mut self, win: &str, name: &str, folder: &str, argv: &[String]) {
        note(format!("Agent {name}: wird geöffnet"));
        let name = name.to_string();
        let (spot, order, ahead) = self.next_spot(win);
        self.pending.insert(name.clone(), (self.clock)());
        if let Some((sp, _)) = &spot {
            self.spots.insert(name.clone(), *sp);
        }
        let target = spot.as_ref().map(|(_, r)| *r);
        let line = self.window_line(&name, folder, argv, spot.as_ref().map(|(sp, _)| sp));
        let launcher = self.fx.launcher.clone();
        let d = self.fx.desktop.clone();
        let hold = self.frame_hold.clone();
        self.screen.remove(win);
        self.worker.run(move || {
            let d = d.as_ref();
            under_curtain(d, target, &hold, |wait| {
                let Some(wait) = wait else {
                    // Ohne Vorhang: erst die liegenden beiseite, dann das neue in die frei
                    // gewordene Kachel.
                    if !order.is_empty() {
                        wt_tiles::tile(d, &order, ahead.len() + 1);
                    }
                    launcher.launch(&line);
                    return;
                };
                // Aufträge laufen nacheinander: wer vorher angefordert wurde, steht schon
                // (oder kommt nicht mehr). Also zählt, was wirklich ein Fenster hat.
                let mut block = order;
                block.extend(ahead.into_iter().filter(|n| wt_tiles::hwnd_of(d, n).is_some()));
                if !block.is_empty() {
                    wt_tiles::tile(d, &block, 1);
                }
                launcher.launch(&line);
                let until = Instant::now() + OPEN_WAIT;
                while wt_tiles::hwnd_of(d, &name).is_none() && Instant::now() < until {
                    wait(Duration::from_millis(40));
                }
                block.push(name);
                // Windows Terminal setzt die Größe eines frischen Fensters gern noch einmal
                // selbst (ganze Zeichen, Rand weg): so lange nachlegen, bis es hält.
                place_until_still(d, &block, wait, OPEN_ROUNDS);
            });
        });
    }

    /// Kommandozeile für `wt.exe`: PowerShell mit `wt_agent.ps1`, bei Agenten in WSL
    /// `deck-wsl.exe` mit dem Ordner als Linux-Pfad (`deck_core::wsl_launch`).
    fn window_line(&self, name: &str, folder: &str, argv: &[String], spot: Option<&wt::Spot>) -> Vec<String> {
        let s = &self.setup;
        let state = s.state_dir.to_string_lossy();
        let Some(l) = &s.wsl else {
            let spec = wt::launch_spec(name, &state, argv);
            return wt::wt_argv(&s.wt_exe, name, folder, &s.shell, &s.script.to_string_lossy(), &spec, s.profile.as_deref(), self.scheme, spot);
        };
        // Ein Ordner außerhalb der Distro (anderes Laufwerk ohne Einhängung) bleibt ohne
        // Linux-Pfad: dann startet der Agent in seinem Home.
        let cwd = l.wsl.to_linux(folder).unwrap_or_else(|| if l.wsl.home.is_empty() { "~".into() } else { l.wsl.home.clone() });
        let spec = deck_core::wsl_launch::WslSpec {
            slot: name.to_string(),
            state: state.into_owned(),
            distro: l.wsl.distro.clone(),
            mount: l.wsl.mount_root.clone(),
            cwd,
            script: l.script.clone(),
            args: argv.to_vec(),
        };
        let cmd = vec![l.launcher.clone(), wt::encode_spec(&spec.to_json())];
        wt::wt_argv_cmd(&s.wt_exe, name, &l.window_dir, &cmd, s.profile.as_deref(), self.scheme, spot)
    }

    /// Das Fenster des Agenten nach vorn. `false` = der Slot lebt nicht.
    pub fn focus_pane(&mut self, slot: &str) -> bool {
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

    /// Vollbild-Taste ([`deck_win::arrange::toggle_maximize`]): das Fenster des Agenten
    /// maximieren und nach vorn holen; mit `may_restore` geht ein maximiertes zurück an
    /// seinen Platz. `false` = der Slot lebt nicht.
    pub fn toggle_maximize(&mut self, slot: &str, may_restore: bool) -> bool {
        if !self.alive_slots().contains_key(slot) {
            return false;
        }
        let (d, slot) = (self.fx.desktop.clone(), slot.to_string());
        self.worker.run(move || {
            let Some(h) = wt_tiles::hwnd_of(d.as_ref(), &slot) else { return };
            match d.toggle_maximize(h, may_restore) {
                Ok(true) => {
                    let _ = d.focus(h);
                }
                Ok(false) => {}
                Err(e) => note(format!("wt_host.maximize {slot}: {e}")),
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
        let (d, hold) = (self.fx.desktop.clone(), self.frame_hold.clone());
        self.worker.run(move || {
            let _hold = Holding::start(&hold);
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
    pub fn front_all(&mut self, slots: &[String], cursor: Option<(i32, i32)>) -> bool {
        let alive = self.alive_slots();
        let slots: Vec<String> = slots.iter().filter(|s| alive.contains_key(*s)).cloned().collect();
        if slots.is_empty() {
            return false;
        }
        let (d, hold) = (self.fx.desktop.clone(), self.frame_hold.clone());
        self.worker.run(move || {
            let _hold = Holding::start(&hold);
            wt_tiles::bring_front(d.as_ref(), &slots, cursor, &|wait| crate::group_shadow::catch_up(&hold, wait));
        });
        true
    }

    /// Liegen alle lebenden unter `slots` offen ([`wt_tiles::all_open`])?
    pub fn all_open(&mut self, slots: &[String]) -> bool {
        let alive = self.alive_slots();
        let slots: Vec<String> = slots.iter().filter(|s| alive.contains_key(*s)).cloned().collect();
        wt_tiles::all_open(self.fx.desktop.as_ref(), &slots)
    }

    /// Liegen die lebenden unter `slots` auf dem Monitor unter `cursor`
    /// ([`wt_tiles::on_cursor_monitor`])?
    pub fn on_cursor_monitor(&mut self, slots: &[String], cursor: Option<(i32, i32)>) -> bool {
        let alive = self.alive_slots();
        let slots: Vec<String> = slots.iter().filter(|s| alive.contains_key(*s)).cloned().collect();
        wt_tiles::on_cursor_monitor(self.fx.desktop.as_ref(), &slots, cursor)
    }

    /// Das fremde Fenster, das einen der lebenden unter `slots` verdeckt ([`wt_tiles::coverer`]).
    pub fn coverer(&mut self, slots: &[String]) -> Option<deck_win::Hwnd> {
        let alive = self.alive_slots();
        let slots: Vec<String> = slots.iter().filter(|s| alive.contains_key(*s)).cloned().collect();
        wt_tiles::coverer(self.fx.desktop.as_ref(), &slots)
    }

    /// Taste „alle nach vorn“, zweiter Druck ([`wt_tiles::hide`]): zurück nach `back`
    /// und die Agentenfenster minimieren.
    pub fn hide_all(&mut self, slots: &[String], back: Option<deck_win::Hwnd>) {
        let slots = slots.to_vec();
        let (d, hold) = (self.fx.desktop.clone(), self.frame_hold.clone());
        self.worker.run(move || {
            let _hold = Holding::start(&hold);
            wt_tiles::hide(d.as_ref(), &slots, back, &|wait| crate::group_shadow::catch_up(&hold, wait));
        });
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
    /// das Fenster dann selbst zu). `false` = der Slot lebt nicht.
    pub fn close_agent(&mut self, slot: &str) -> bool {
        let Some(pid) = self.alive_slots().get(slot).copied() else { return false };
        note(format!("Agent {slot}: wird geschlossen (pid {pid})"));
        let path = self.setup.state_dir.join(format!("{slot}{}", wt::PID_SUFFIX));
        let p = self.fx.procs.clone();
        self.closing.insert(slot.to_string());
        // Die übrigen des Blocks in ihrer jetzigen Reihenfolge: sie rücken unter dem
        // Vorhang zusammen, man sieht weder das Loch noch das Nachrutschen.
        let win: String = slot.chars().take(1).collect();
        let lying: Vec<String> = self.screen_rows(&win).0.concat();
        let rest = deck_core::arrange::keep_order(&lying, &self.terminals(&win), Some(slot));
        let slot = slot.to_string();
        let d = self.fx.desktop.clone();
        let hold = self.frame_hold.clone();
        self.screen.remove(&win);
        self.worker.run(move || {
            let d = d.as_ref();
            let around = wt_tiles::hwnd_of(d, &slot).and_then(|h| d.window_rect(h));
            under_curtain(d, around, &hold, |wait| {
                match p.kill_tree(pid) {
                    Ok(()) => {
                        let _ = fs::remove_file(&path);
                        // Ein Ende ohne das Deck protokolliert `crash_resume` samt Urteil.
                        note(format!("Agent {slot} beendet (vom Deck geschlossen)"));
                    }
                    Err(e) => {
                        note(format!("wt_host.close pid={pid}: {e}"));
                        return;
                    }
                }
                let Some(wait) = wait else { return };
                let until = Instant::now() + CLOSE_WAIT;
                while wt_tiles::hwnd_of(d, &slot).is_some() && Instant::now() < until {
                    wait(Duration::from_millis(30));
                }
                if !rest.is_empty() {
                    place_until_still(d, &rest, wait, 2);
                }
            });
        });
        self.alive_ts = -1.0;
        true
    }
}

/// Für Tests, die einen Neustart des Decks nachstellen: derselbe Host noch einmal frisch.
#[cfg(test)]
impl WtHost {
    pub(crate) fn setup_for_tests(&self) -> WtSetup {
        self.setup.clone()
    }
    pub(crate) fn effects_for_tests(&self) -> WtEffects {
        self.fx.clone()
    }
}
