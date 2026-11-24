//! Reserve-Agent im Windows-Terminal-Host: anlegen, verstecken, übernehmen, austauschen.
//! Wann was geschieht, rechnet `deck_core::reserve`; hier stehen Fenster und Prozesse. Ob
//! es eine Reserve gibt, sagt der Aufrufer ([`WtHost::keep_reserve`] mit `want`); in der
//! Team-Ausgabe nie (`reserve_agent.rs`).
//!
//! Ablauf:
//! 1. **Anlegen** (nur bei Ruhe, [`reserve::quiet`]): Slot-Namen vergeben, `wt.exe` wie
//!    für einen echten Agenten an dessen künftigem Platz starten (dieselbe Kommandozeile,
//!    also auch in WSL, wenn die Agenten dort laufen), unter dem Vorhang. Sobald das
//!    Fenster da ist, wird es versteckt und der Fokus geht an das Fenster zurück, das
//!    vorher vorn war. Windows Terminal kann kein Fenster ohne Aktivieren öffnen.
//! 2. **Warten**: versteckt zählt das Fenster nirgends mit (`class_windows` sieht nur
//!    sichtbare), und [`WtHost::alive_slots`] nimmt den Slot aus der Liste der lebenden.
//! 3. **Übernehmen** ([`WtHost::create_agent`] mit passendem Rezept): unter dem Vorhang an
//!    seinen Platz im Raster legen, zeigen, fokussieren. Ab da ist er ein gewöhnlicher Agent.
//! 4. **Austauschen**: andere Einstellungen, zu alt oder Shell tot → beenden; die nächste
//!    entsteht beim nächsten ruhigen Moment.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use deck_core::reserve::{self, Recipe, Reserve, Situation, Step as Plan};

use super::*;

/// So lange nach dem Verstecken schaut das Deck noch, ob Windows Terminal das Fenster
/// wieder zeigt.
const HIDE_SETTLE: Duration = Duration::from_millis(1000);

/// Wie das Anlegen auf dem Arbeiter-Thread ausging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    /// Fenster da und versteckt: übernehmbar.
    Ready,
    /// Kein Fenster gekommen oder nicht zu verstecken.
    NoWindow,
    /// Direkt vor dem Start war es nicht mehr ruhig: nichts gestartet.
    Skipped,
    /// Während das Fenster vorn war, kam eine Eingabe: die Reserve ist verdorben.
    Typed,
}

type OutcomeSlot = Arc<Mutex<Option<Outcome>>>;

/// Zustand der Reserve im WT-Host.
#[derive(Default)]
pub struct ReserveState {
    current: Option<Reserve>,
    /// Shell der Reserve, wenn sie lebt (aus [`WtHost::alive_slots`]).
    pub(super) pid: Option<u32>,
    /// Vor diesem Zeitpunkt kein neuer Versuch (nach einem Fehlschlag).
    retry_after: f64,
    outcome: OutcomeSlot,
    file: PathBuf,
    /// Aus der Reserve übernommene Slots, bis das Deck sie abholt
    /// ([`WtHost::take_from_reserve`]).
    claimed: BTreeSet<String>,
    /// Beendete Reserven, deren Shell beim Beenden noch keine PID-Datei hatte: taucht
    /// sie noch auf, wird sie beendet statt als Agent ohne Fenster zu gelten. Wert = bis
    /// wann (Unix-Sekunden).
    doomed: BTreeMap<String, f64>,
}

impl ReserveState {
    /// Die gemerkte Reserve neben `wt_hosts.json` laden (überlebt einen Neustart des Decks).
    pub(super) fn load(hosts_file: &Path) -> ReserveState {
        let file = hosts_file.with_file_name(reserve::FILE);
        let current = reserve::load(load_json(&file));
        ReserveState { current, file, ..ReserveState::default() }
    }

    /// Ist das der Slot der Reserve?
    pub(super) fn is(&self, slot: &str) -> bool {
        self.slot() == Some(slot)
    }

    /// Namen beendeter Reserven, deren Shell noch kommen kann: nicht neu vergeben, sonst
    /// träfe `reap_doomed` die neue.
    pub(super) fn doomed_slots(&self) -> impl Iterator<Item = &String> {
        self.doomed.keys()
    }

    pub(super) fn slot(&self) -> Option<&str> {
        self.current.as_ref().map(|r| r.slot.as_str())
    }

    fn set(&mut self, r: Option<Reserve>) {
        self.current = r;
        match &self.current {
            Some(r) => {
                if let Ok(v) = serde_json::to_value(r) {
                    let _ = save_json(&self.file, &v);
                }
            }
            None => {
                let _ = fs::remove_file(&self.file);
            }
        }
    }
}

impl WtHost {
    /// Das Rezept, mit dem ein neuer Agent in `win` starten würde (`None` = kein Block
    /// oder unbrauchbare Argumente).
    pub fn recipe(&self, win: &str, model: Option<&str>, args: Option<&str>) -> Option<Recipe> {
        let folder = self.folder(win)?;
        let argv = wt::claude_argv(model, args).ok()?;
        Some(self.recipe_for(win, &folder, &argv))
    }

    fn recipe_for(&self, win: &str, folder: &str, argv: &[String]) -> Recipe {
        Recipe {
            win: win.to_string(),
            folder: folder.to_string(),
            argv: argv.to_vec(),
            scheme: self.scheme.map(String::from),
            wsl: self.setup.wsl.as_ref().map(|l| l.wsl.distro.clone()),
        }
    }

    /// Ein Takt der Reserve-Pflege: `want` ist die Reserve, die es geben soll (`None` =
    /// keine). Legt an, tauscht aus oder räumt ab ([`reserve::decide`]).
    pub fn keep_reserve(&mut self, want: Option<Recipe>, now: f64) {
        // Team-Ausgabe: nichts zu tun, nicht einmal nach Fenstern fragen.
        if want.is_none() && self.reserve.current.is_none() {
            return;
        }
        self.alive_slots();
        self.collect_outcome(now);
        let d = self.fx.desktop.clone();
        let front = d.foreground();
        let plan = reserve::decide(&Situation {
            now,
            idle_s: d.input_idle_s(),
            front_class: front.as_ref().map(|w| w.class.as_str()),
            want: want.as_ref(),
            current: self.reserve.current.as_ref(),
            alive: self.reserve.pid.is_some(),
            retry_after: self.reserve.retry_after,
        });
        match plan {
            Plan::Keep => {}
            Plan::Retire(why) => self.retire_reserve(why, now, true),
            Plan::Spawn => {
                if let Some(w) = want {
                    self.spawn_reserve(w, now);
                }
            }
        }
    }

    /// Ergebnis des Anlegens abholen: fertig → übernehmbar; sonst abräumen.
    fn collect_outcome(&mut self, now: f64) {
        let got = self.reserve.outcome.lock().ok().and_then(|mut o| o.take());
        let Some(outcome) = got else { return };
        let Some(mut r) = self.reserve.current.clone() else { return };
        match outcome {
            Outcome::Ready => {
                r.ready = true;
                note(format!("Reserve-Agent {} bereit nach {:.1} s", r.slot, now - r.since));
                self.reserve.set(Some(r));
            }
            Outcome::NoWindow => self.retire_reserve("kein Fenster", now, true),
            // Nichts gestartet: kein Fehlschlag, nur später wieder versuchen.
            Outcome::Skipped => {
                note(format!("Reserve-Agent {}: nicht angelegt, es war nicht mehr ruhig", r.slot));
                self.reserve.set(None);
            }
            // Getipptes steht womöglich in der Reserve: nie übergeben. Die nächste kommt
            // nach der nächsten ruhigen Minute, das reicht als Pause.
            Outcome::Typed => self.retire_reserve("Eingabe während des Anlegens", now, false),
        }
    }

    /// Eine Reserve anlegen: unter dem Vorhang starten, verstecken, Fokus zurück.
    fn spawn_reserve(&mut self, recipe: Recipe, now: f64) {
        let win = recipe.win.clone();
        let name = wt::next_slot(&win, &self.taken(&win));
        let (spot, _, _) = self.next_spot(&win);
        let target = spot.as_ref().map(|(_, r)| *r);
        let line = self.window_line(&name, &recipe.folder, &recipe.argv, spot.as_ref().map(|(sp, _)| sp));
        // Ein Rest gleichen Namens (früherer Agent) soll der Reserve nichts vererben.
        deck_core::slot_state::clear_state(&self.setup.state_dir, &name);
        self.reserve.set(Some(Reserve { slot: name.clone(), recipe, since: now, ready: false }));
        let outcome: OutcomeSlot = Arc::new(Mutex::new(None));
        self.reserve.outcome = outcome.clone();
        let (launcher, d, hold) = (self.fx.launcher.clone(), self.fx.desktop.clone(), self.frame_hold.clone());
        note(format!("Reserve-Agent {name} wird angelegt"));
        self.worker.run(move || {
            let d = d.as_ref();
            let result = spawn_hidden(d, &*launcher, &line, &name, target, &hold);
            if let Ok(mut o) = outcome.lock() {
                *o = Some(result);
            }
        });
    }

    /// Die Reserve beenden (versteckt, also ohne Vorhang). `cooldown`: nach einem
    /// Fehlschlag beim Anlegen eine Pause, sonst kreiste das Deck im 400-ms-Takt.
    fn retire_reserve(&mut self, why: &str, now: f64, cooldown: bool) {
        let Some(r) = self.reserve.current.clone() else { return };
        note(format!("Reserve-Agent {} beendet: {why}", r.slot));
        if cooldown && !r.ready {
            self.reserve.retry_after = now + reserve::FAIL_COOLDOWN_S;
        }
        match self.reserve.pid.take() {
            Some(pid) => self.kill_reserve_shell(&r.slot, pid),
            // Shell noch ohne PID-Datei: kommt sie später, beendet sie `reap_doomed`.
            None => {
                // Uhr des Hosts, wie in `alive_slots`, das die Frist prüft.
                let until = (self.clock)() + 2.0 * reserve::SPAWN_TIMEOUT_S;
                self.reserve.doomed.insert(r.slot.clone(), until);
            }
        }
        deck_core::slot_state::clear_state(&self.setup.state_dir, &r.slot);
        self.reserve.set(None);
        self.alive_ts = -1.0;
    }

    /// Passt die Reserve zu diesem Start, wird sie übernommen: an ihren Platz im Raster,
    /// zeigen, fokussieren. `false` = keine passende, der Aufrufer startet kalt.
    pub(super) fn claim_reserve(&mut self, win: &str, folder: &str, argv: &[String]) -> bool {
        let ask = self.recipe_for(win, folder, argv);
        self.alive_slots();
        if !reserve::fits(self.reserve.current.as_ref(), &ask) || self.reserve.pid.is_none() {
            return false;
        }
        let Some(r) = self.reserve.current.clone() else { return false };
        let name = r.slot.clone();
        // Fenster weg (von Hand geschlossen, WT neu gestartet): abräumen, kalt starten.
        if self.fx.desktop.find_window(wt::WINDOW_CLASS, &name).is_none() {
            self.retire_reserve("Fenster fehlt", (self.clock)(), true);
            return false;
        }
        let (spot, order, ahead) = self.next_spot(win);
        let target = spot.as_ref().map(|(_, r)| *r);
        self.reserve.set(None);
        self.reserve.pid = None;
        self.reserve.claimed.insert(name.clone());
        self.alive_ts = -1.0;
        self.screen.remove(win);
        note(format!("Reserve-Agent {name} übernommen (wartete {:.0} s)", (self.clock)() - r.since));
        let (d, hold) = (self.fx.desktop.clone(), self.frame_hold.clone());
        self.worker.run(move || {
            let d = d.as_ref();
            under_curtain(d, target, &hold, |wait| {
                let Some(h) = d.find_window(wt::WINDOW_CLASS, &name) else {
                    note(format!("Reserve-Agent {name}: Fenster beim Übernehmen weg"));
                    return;
                };
                // Wie beim kalten Öffnen (`open_agent`): die liegenden rücken beiseite, das
                // neue kommt in die frei gewordene Kachel, alles unter dem Vorhang.
                let mut block = order;
                block.extend(ahead.into_iter().filter(|n| wt_tiles::hwnd_of(d, n).is_some()));
                if !block.is_empty() {
                    wt_tiles::tile(d, &block, 1);
                }
                if let Some(t) = target {
                    let _ = d.place(h, t);
                }
                if let Err(e) = d.set_visible(h, true) {
                    note(format!("Reserve-Agent {name}: zeigen ging nicht ({e})"));
                }
                block.push(name.clone());
                match wait {
                    Some(w) => place_until_still(d, &block, w, OPEN_ROUNDS),
                    None => {
                        wt_tiles::tile(d, &block, 0);
                    }
                }
                if let Err(e) = d.focus(h) {
                    note(format!("Reserve-Agent {name}: Fokus ging nicht ({e})"));
                }
            });
        });
        true
    }

    fn kill_reserve_shell(&mut self, slot: &str, pid: u32) {
        self.closing.insert(slot.to_string());
        let p = self.fx.procs.clone();
        let path = self.setup.state_dir.join(format!("{slot}{}", wt::PID_SUFFIX));
        self.worker.run(move || match p.kill_tree(pid) {
            Ok(()) => {
                let _ = fs::remove_file(&path);
            }
            Err(e) => note(format!("Reserve-Agent pid={pid}: nicht beendet ({e})")),
        });
    }

    /// Aus [`WtHost::alive_slots`]: verspätete Shells beendeter Reserven aus `out` nehmen
    /// und beenden.
    pub(super) fn reap_doomed(&mut self, out: &mut BTreeMap<String, u32>, started: &mut BTreeMap<String, f64>, now: f64) {
        self.reserve.doomed.retain(|_, until| *until > now);
        let late: Vec<(String, u32)> = self.reserve.doomed.keys().filter_map(|s| out.get(s).map(|p| (s.clone(), *p))).collect();
        for (slot, pid) in late {
            out.remove(&slot);
            started.remove(&slot);
            self.reserve.doomed.remove(&slot);
            note(format!("Reserve-Agent {slot}: Shell kam nach dem Beenden, wird beendet"));
            self.kill_reserve_shell(&slot, pid);
        }
    }

    /// Die Reserve beenden und `secs` lang keine neue anlegen: für Arbeiten von außen, die
    /// nur gehen, solange weder die Shell noch Windows Terminal laufen (eine Erweiterung,
    /// die beides aktualisiert). `true` = es gab eine.
    pub fn pause_reserve(&mut self, why: &str, now: f64, secs: f64) -> bool {
        self.alive_slots();
        let had = self.reserve.current.is_some();
        self.retire_reserve(why, now, false);
        self.reserve.retry_after = self.reserve.retry_after.max(now + secs);
        had
    }

    /// War dieser Slot eine übernommene Reserve? Einmal `true`, danach vergessen. Für den
    /// Startmodus: ihre Zustandsdatei ist schon frisch, nicht vom Vorgänger geerbt.
    pub fn take_from_reserve(&mut self, slot: &str) -> bool {
        self.reserve.claimed.remove(slot)
    }

    /// Slot der Reserve, falls es eine gibt (Diagnose, Tests, Erweiterungen).
    pub fn reserve_slot(&self) -> Option<String> {
        self.reserve.slot().map(String::from)
    }
}

/// Auf dem Arbeiter-Thread: die Reserve unter dem Vorhang starten, ihr Fenster verstecken
/// und den Fokus zurückgeben. Vorher noch einmal nach Ruhe fragen (der Auftrag kann hinter
/// anderen gewartet haben), hinterher, ob in der Zwischenzeit jemand getippt hat.
fn spawn_hidden(d: &dyn Desktop, launcher: &dyn crate::wt_setup::Launcher, line: &[String], name: &str, target: Option<Rect>, hold: &FrameHold) -> Outcome {
    let before = d.foreground();
    if !reserve::quiet(d.input_idle_s(), before.as_ref().map(|w| w.class.as_str())) {
        return Outcome::Skipped;
    }
    let started = Instant::now();
    let mut result = Outcome::NoWindow;
    under_curtain(d, target, hold, |wait| {
        let pause = |t: Duration| match wait {
            Some(w) => w(t),
            None => std::thread::sleep(t),
        };
        if !launcher.launch(line) {
            return;
        }
        // Windows Terminal legt das Fenster erst unsichtbar an und zeigt es gut 50 ms
        // später selbst (VM-Messung 20.11.2026). Wer es vorher versteckt, versteckt
        // nichts: also warten, bis es sichtbar ist.
        let until = Instant::now() + OPEN_WAIT;
        let hwnd = loop {
            if let Some(h) = d.find_window(wt::WINDOW_CLASS, name).filter(|h| d.is_alive(*h)) {
                break Some(h);
            }
            if Instant::now() >= until {
                break None;
            }
            pause(Duration::from_millis(15));
        };
        let Some(h) = hwnd else { return };
        if let Err(e) = d.set_visible(h, false) {
            note(format!("Reserve-Agent {name}: verstecken ging nicht ({e})"));
            return;
        }
        // Wer vorher vorn war, bekommt die Tastatur sofort zurück, nicht erst nach der
        // Nachschau unten: jede Millisekunde vorn ist eine, in der Tasten verloren gehen.
        give_back(d, before.as_ref(), h, name);
        // Zeigt es sich doch noch einmal, gleich wieder weg (unter dem Vorhang).
        let settle = Instant::now() + HIDE_SETTLE;
        while Instant::now() < settle {
            pause(Duration::from_millis(40));
            if d.is_alive(h) {
                note(format!("Reserve-Agent {name}: zeigte sich wieder, erneut versteckt"));
                let _ = d.set_visible(h, false);
                give_back(d, before.as_ref(), h, name);
            }
        }
        result = Outcome::Ready;
    });
    if result == Outcome::Ready && reserve::typed_while_spawning(d.input_idle_s(), started.elapsed().as_secs_f64()) {
        return Outcome::Typed;
    }
    result
}

/// Fokus an das Fenster zurück, das vor dem Anlegen vorn war.
fn give_back(d: &dyn Desktop, before: Option<&deck_win::WindowInfo>, reserve: deck_win::Hwnd, name: &str) {
    if let Some(b) = before.filter(|b| b.hwnd != reserve)
        && let Err(e) = d.focus(b.hwnd)
    {
        note(format!("Reserve-Agent {name}: Fokus nicht zurückgegeben ({e})"));
    }
}
