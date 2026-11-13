//! Der leuchtende Rahmen um das Windows-Terminal-Agentenfenster, in das gerade getippt
//! wird (`deck/ui/focus_frame.py`). Gezeichnet auf der Grafikkarte mit dem Rahmen aus
//! `deck_win::frame_gpu_overlay`; dieses Modul entscheidet, ob und um welches Fenster.
//!
//! Der Rahmen ist der eigene Glow (`Design::Eigen`), je einer für helles und dunkles
//! Terminal, eingestellt im Fenster „Glow einstellen“ (`crate::glow`, Strg+Alt+G) und
//! sofort wirksam. Vorgabe dunkel ist Honiggold, nah und dicht (gewählt am 12.11.2026 nach
//! sieben Proberunden). Vom ersten Advent bis Dreikönig kommen stattdessen die drei
//! Winter-Designs dran (`deck_core::season`, Einstellung `frame_season`); geprüft wird
//! bei jedem Fokuswechsel, der Wechsel braucht also keinen Neustart. Die Agentenfenster bekommen eckige Ecken: ein eckiger Rahmen um eine
//! runde Win11-Ecke ließe dort den Hintergrund durchscheinen. Das geschieht bei jedem
//! Fokuswechsel für alle Agentenfenster, damit gekachelte Fenster gleich aussehen.
//! Dazu bekommt ihr Rand die Grundfarbe des Terminals: der Win11-Rand ist sonst
//! halbdurchsichtig, zwischen zwei Kacheln schien eine Fuge Hintergrund durch.
//! Und jedes Agentenfenster kommt einmal in die eigene Taskleisten-Gruppe der Agenten
//! (eigener Knopf mit eigenem Symbol statt des Terminal-Knopfs, `deck_win::taskbar`).
//!
//! Im selben Thread läuft der gemeinsame Schatten bündig liegender Agenten
//! (`crate::group_shadow`): auch er braucht eigene Fenster und deren Nachrichtenpumpe.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use deck_core::group_shadow::ring_rgb;
use deck_core::wt::{WINDOW_CLASS, paper_for};
use deck_win::Hwnd;
use deck_win::effects::Desktop;
use deck_win::focus::Corners;
use deck_win::frame_fx::Design;
use deck_win::taskbar::TaskbarGroup;

use crate::applog::note;
use crate::group_shadow::{RingLayer, ShadowLayer, ShadowWatch, StatusMap};
use crate::wt_host::{FrameHold, HostMap, WtHost};

/// So oft wird geschaut, welches Fenster vorn ist.
const WATCH: Duration = Duration::from_millis(60);

/// Welches Fenster bekommt den Rahmen, mit welchem Design?
pub struct FrameWatch {
    desktop: Arc<dyn Desktop>,
    hosts: HostMap,
    last_fg: Option<Hwnd>,
    squared: HashSet<Hwnd>,
    /// Taskleisten-Gruppe der Agenten; `None` = Fenster bleiben beim Terminal-Knopf.
    group: Option<TaskbarGroup>,
    /// Fenster, die schon in der Gruppe stehen (unabhängig vom hell/dunkel-Wechsel).
    grouped: HashSet<Hwnd>,
    /// Modus (hell/dunkel), in dem `squared` gestylt wurde.
    styled_light: Option<Option<bool>>,
    pub target: Option<Hwnd>,
    pub design: Option<Design>,
    pub seed: i32,
    /// Dunkles Terminal? Ohne Auskunft hell, wie beim Rand.
    pub night: bool,
    /// Ist gerade Winterzeit? Gefragt wird bei jedem Fokuswechsel.
    pub winter: Box<dyn Fn() -> bool>,
    /// Solange gesetzt, bleibt der Rahmen, wo er ist ([`FrameHold`]).
    pub hold: FrameHold,
    /// Fenster, das den Rahmen nicht wegnimmt, wenn es vorn ist: „Glow einstellen“. Man
    /// stellt dort ein und sieht es am zuletzt gewählten Agenten.
    pub keep: Box<dyn Fn(Hwnd) -> bool>,
    /// Zustand je Slot für die Randfarbe; leer = alle in Papierfarbe.
    pub statuses: StatusMap,
    /// Zuletzt gesetzte Randfarbe je Fenster.
    borders: HashMap<Hwnd, u32>,
    rng: u64,
}

impl FrameWatch {
    pub fn new(desktop: Arc<dyn Desktop>, hosts: HostMap, group: Option<TaskbarGroup>, seed: u64) -> FrameWatch {
        FrameWatch {
            desktop,
            hosts,
            last_fg: None,
            squared: HashSet::new(),
            group,
            grouped: HashSet::new(),
            styled_light: None,
            target: None,
            design: None,
            seed: 0,
            night: false,
            winter: Box::new(|| false),
            hold: FrameHold::default(),
            keep: Box::new(|_| false),
            statuses: StatusMap::default(),
            borders: HashMap::new(),
            rng: seed | 1,
        }
    }

    fn random(&mut self) -> u64 {
        // xorshift64: genügt für „nicht zweimal dasselbe Design“.
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    fn is_agent_title(&self, title: &str) -> bool {
        WtHost::slot_of_title(&self.hosts, title).is_some()
    }

    /// Den Rand jedes Agentenfensters in der Farbe seines Zustands halten (Entwurf „02a
    /// Fein“); kennt das Deck den Zustand nicht, in Papierfarbe. Es ist die Randfarbe von
    /// Windows selbst. Sichtbar ist sie nur an frei liegenden Fenstern: gekachelten stellt
    /// der Gruppenschatten den Rahmen von Windows ab, dort trägt der Ring in der Naht die
    /// Farbe (`crate::group_shadow`). Gesetzt wird nur, was sich geändert hat.
    fn color_borders(&mut self) {
        let light = self.desktop.light_theme();
        let status = self.statuses.read().map(|m| m.clone()).unwrap_or_default();
        let windows = self.desktop.class_windows(WINDOW_CLASS);
        self.borders.retain(|h, _| windows.iter().any(|w| w.hwnd == *h));
        for w in windows {
            let Some(slot) = WtHost::slot_of_title(&self.hosts, &w.title) else { continue };
            let rgb = status.get(&slot).map_or_else(|| paper_for(light), |s| ring_rgb(*s, light != Some(false)));
            if self.borders.get(&w.hwnd) != Some(&rgb) && self.desktop.set_border(w.hwnd, rgb).is_ok() {
                self.borders.insert(w.hwnd, rgb);
            }
        }
    }

    fn square_agents(&mut self) {
        let light = self.desktop.light_theme();
        self.night = light == Some(false);
        if self.styled_light != Some(light) {
            self.styled_light = Some(light);
            self.squared.clear();
        }
        let windows = self.desktop.class_windows(WINDOW_CLASS);
        // Zugegangene Fenster vergessen: Windows vergibt ein Handle später neu, das neue
        // Fenster bliebe sonst ungestylt.
        let open: HashSet<Hwnd> = windows.iter().map(|w| w.hwnd).collect();
        self.squared.retain(|h| open.contains(h));
        self.grouped.retain(|h| open.contains(h));
        for w in windows {
            if !self.is_agent_title(&w.title) {
                continue;
            }
            if !self.squared.contains(&w.hwnd) {
                let _ = self.desktop.set_corners(w.hwnd, Corners::Square);
                self.squared.insert(w.hwnd);
            }
            if let Some(group) = &self.group
                && self.grouped.insert(w.hwnd)
                && let Err(e) = self.desktop.join_taskbar_group(w.hwnd, group)
            {
                note(format!("Taskleisten-Gruppe für {}: {e}", w.title));
            }
        }
    }

    /// Einmal hinschauen. `true`, wenn sich das Ziel geändert hat.
    pub fn step(&mut self) -> bool {
        if self.hold.load(std::sync::atomic::Ordering::SeqCst) {
            return false;
        }
        let fg = self.desktop.foreground();
        let fg_hwnd = fg.as_ref().map(|w| w.hwnd);
        if fg_hwnd.is_some_and(|h| (self.keep)(h)) {
            return false;
        }
        if fg_hwnd != self.last_fg {
            self.last_fg = fg_hwnd;
            self.square_agents();
        }
        self.color_borders();
        let target = fg
            .filter(|w| w.class == WINDOW_CLASS && self.is_agent_title(&w.title) && !self.desktop.is_minimized(w.hwnd))
            .map(|w| w.hwnd);
        if target == self.target {
            // Ändert sich die Auswahl unter dem offenen Rahmen (Fenster „Glow einstellen“
            // auf oder zu, im Winter), gleich umschalten, nicht erst beim nächsten Fokus.
            let pool = Design::pool((self.winter)());
            if target.is_some() && self.design.is_some_and(|d| !pool.contains(&d)) {
                let r = self.random();
                self.design = Design::pick(pool, self.design, r);
                return true;
            }
            return false;
        }
        self.target = target;
        if target.is_some() {
            let r = self.random();
            self.design = Design::pick(Design::pool((self.winter)()), self.design, r);
            self.seed = (self.random() % (1 << 30)) as i32;
        }
        true
    }
}

/// Den Rahmen-Thread starten. Fehlt die Grafikkarte, gibt es eben keinen Rahmen (einmal
/// ins Log), das Deck läuft weiter. `season` ist die Einstellung `frame_season`,
/// `group_shadow` schaltet den gemeinsamen Schatten bündig liegender Agenten ein, `glow`
/// ist der eigene Glow; jedes Bild liest ihn neu, eine Änderung wirkt also sofort.
pub fn start(
    glow: crate::glow::GlowCell,
    desktop: Arc<dyn Desktop>,
    hosts: HostMap,
    hold: FrameHold,
    group: Option<TaskbarGroup>,
    season: Option<serde_json::Value>,
    group_shadow: bool,
) {
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(7);
    std::thread::Builder::new()
        .name("focus-frame".into())
        .spawn(move || {
            let mut frame = match deck_win::frame_gpu_overlay::GpuFocusFrame::new() {
                Ok(f) => Some(f),
                Err(e) => {
                    note(format!("Fokus-Rahmen nicht verfügbar: {e}"));
                    None
                }
            };
            if frame.is_none() && !group_shadow {
                return;
            }
            let mut shadows = group_shadow.then(|| (ShadowWatch::new(desktop.clone(), hosts.clone(), crate::group_shadow::statuses()), ShadowLayer::default()));
            let mut rings = RingLayer::default();
            let shadow_desktop = desktop.clone();
            let shadow_hold = hold.clone();
            let mut watch = FrameWatch::new(desktop, hosts, group, seed);
            watch.hold = hold;
            watch.statuses = crate::group_shadow::statuses();
            let keep = glow.clone();
            watch.keep = Box::new(move |h| keep.is_window(h.0));
            let preview = glow.clone();
            watch.winter = Box::new(move || {
                !preview.previewing() && deck_core::season::winter_frames(season.as_ref(), deck_win::clock::local_date())
            });
            if (watch.winter)() {
                note("Fokus-Rahmen: Winter-Designs (erster Advent bis Dreikönig oder frame_season)");
            }
            let mut since_watch = Instant::now();
            let mut t0 = Instant::now();
            let mut logged = false;
            loop {
                deck_win::frame_window::pump_messages();
                if since_watch.elapsed() >= WATCH || watch.target.is_none() {
                    since_watch = Instant::now();
                    if watch.step() {
                        t0 = Instant::now();
                    }
                    // Solange das Deck Fenster umlegt, wartet auch der Schatten: sonst
                    // würde er für jeden Zwischenstand neu gerechnet.
                    if let Some((plan, layer)) = shadows.as_mut()
                        && !shadow_hold.load(std::sync::atomic::Ordering::SeqCst)
                    {
                        if plan.step() {
                            layer.apply(&plan.plans);
                            rings.apply(&plan.rings);
                        }
                        let z = shadow_desktop.z_order();
                        rings.keep_behind(&z);
                        layer.keep_behind(&z);
                    }
                }
                match (frame.as_mut().zip(watch.target), watch.design) {
                    (Some((frame, target)), Some(design)) => {
                        // draw wartet auf den Bildwechsel: das ist der Takt der Animation.
                        let params = crate::glow::params(&glow, watch.night);
                        if let Err(e) = frame.draw(target, design, watch.seed, t0.elapsed().as_secs_f64(), watch.night, &params)
                            && !logged
                        {
                            note(format!("Fokus-Rahmen: {e}"));
                            logged = true;
                            std::thread::sleep(WATCH);
                        }
                    }
                    _ => {
                        if let Some(f) = frame.as_mut() {
                            f.hide();
                        }
                        std::thread::sleep(WATCH);
                    }
                }
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeDesktop;
    use deck_win::Rect;
    use std::collections::BTreeMap;
    use std::sync::RwLock;

    fn hosts() -> HostMap {
        Arc::new(RwLock::new(BTreeMap::from([("A".to_string(), "C:/r".to_string())])))
    }

    #[test]
    fn the_window_border_carries_the_agents_status_and_a_resting_agent_keeps_a_quiet_one() {
        use deck_core::view::Status;
        let d = Arc::new(FakeDesktop::default());
        d.add_wt(Hwnd(5), "A1", Rect::new(0, 0, 10, 10), false);
        d.add_wt(Hwnd(6), "PowerShell", Rect::new(0, 0, 10, 10), false);
        let mut w = FrameWatch::new(d.clone(), hosts(), None, 42);
        w.statuses.write().unwrap().insert("A1".into(), Status::Waiting);
        w.step();
        w.step();
        assert_eq!(d.bordered(), vec![(Hwnd(5), 0xE8813F)], "einmal gesetzt, fremde Fenster bleiben");
        w.statuses.write().unwrap().insert("A1".into(), Status::Idle);
        w.step();
        assert_eq!(d.bordered().last(), Some(&(Hwnd(5), 0x9893A5)), "ruhend: gedämpft, nicht farblos");
    }

    #[test]
    fn only_a_focused_agent_window_of_a_terminal_block_gets_the_frame() {
        let d = Arc::new(FakeDesktop::default());
        d.add_wt(Hwnd(5), "A1", Rect::new(0, 0, 10, 10), false);
        d.add_wt(Hwnd(6), "B1", Rect::new(0, 0, 10, 10), false);
        let mut w = FrameWatch::new(d.clone(), hosts(), None, 42);
        d.set_foreground(Hwnd(6), "B1", WINDOW_CLASS);
        assert!(!w.step(), "B ist kein Terminal-Block");
        d.set_foreground(Hwnd(5), "A1", WINDOW_CLASS);
        assert!(w.step());
        assert_eq!(w.target, Some(Hwnd(5)));
        assert_eq!(w.design, Some(Design::Eigen), "außerhalb des Winters der eigene Glow");
        d.set_foreground(Hwnd(9), "Editor", "Notepad");
        assert!(w.step());
        assert_eq!(w.target, None);
        d.set_foreground(Hwnd(5), "A1", WINDOW_CLASS);
        w.step();
        assert_eq!(w.design, Some(Design::Eigen), "auch beim nächsten Fokus");
        assert_eq!(d.cornered(), vec![Hwnd(5)], "nur Agentenfenster werden eckig");
        assert_eq!(d.bordered(), vec![(Hwnd(5), 0xFAF4ED)], "Rand in Papierfarbe, ohne Auskunft hell");
    }

    #[test]
    fn while_the_deck_shuffles_windows_the_frame_stays_and_then_takes_the_last_one() {
        let d = Arc::new(FakeDesktop::default());
        d.add_wt(Hwnd(5), "A1", Rect::new(0, 0, 10, 10), false);
        d.add_wt(Hwnd(6), "A2", Rect::new(0, 0, 10, 10), false);
        let mut w = FrameWatch::new(d.clone(), hosts(), None, 42);
        w.hold.store(true, std::sync::atomic::Ordering::SeqCst);
        d.set_foreground(Hwnd(5), "A1", WINDOW_CLASS);
        assert!(!w.step());
        assert_eq!(w.target, None, "Zwischenstation: kein Rahmen");
        d.set_foreground(Hwnd(6), "A2", WINDOW_CLASS);
        w.hold.store(false, std::sync::atomic::Ordering::SeqCst);
        assert!(w.step());
        assert_eq!(w.target, Some(Hwnd(6)));
    }

    #[test]
    fn the_glow_window_in_front_leaves_the_frame_on_the_last_agent() {
        let d = Arc::new(FakeDesktop::default());
        d.add_wt(Hwnd(5), "A1", Rect::new(0, 0, 10, 10), false);
        let mut w = FrameWatch::new(d.clone(), hosts(), None, 42);
        w.keep = Box::new(|h| h == Hwnd(77));
        d.set_foreground(Hwnd(5), "A1", WINDOW_CLASS);
        w.step();
        d.set_foreground(Hwnd(77), "Glow einstellen", "Tauri Window");
        assert!(!w.step());
        assert_eq!(w.target, Some(Hwnd(5)), "der Rahmen bleibt, man sieht die Änderung");
        d.set_foreground(Hwnd(9), "Editor", "Notepad");
        w.step();
        assert_eq!(w.target, None);
    }

    #[test]
    fn border_follows_the_windows_theme() {
        let d = Arc::new(FakeDesktop::default());
        d.add_wt(Hwnd(5), "A1", Rect::new(0, 0, 10, 10), false);
        let mut w = FrameWatch::new(d.clone(), hosts(), None, 42);
        d.set_light(Some(false));
        d.set_foreground(Hwnd(5), "A1", WINDOW_CLASS);
        w.step();
        d.set_foreground(Hwnd(9), "Editor", "Notepad");
        w.step();
        assert_eq!(d.bordered(), vec![(Hwnd(5), 0x191724)], "gleicher Modus: nicht neu stylen");
        d.set_light(Some(true));
        d.set_foreground(Hwnd(5), "A1", WINDOW_CLASS);
        w.step();
        assert_eq!(d.bordered(), vec![(Hwnd(5), 0x191724), (Hwnd(5), 0xFAF4ED)], "Moduswechsel färbt neu");
    }

    #[test]
    fn in_winter_only_the_winter_designs_take_turns() {
        let d = Arc::new(FakeDesktop::default());
        d.add_wt(Hwnd(5), "A1", Rect::new(0, 0, 10, 10), false);
        let mut w = FrameWatch::new(d.clone(), hosts(), None, 42);
        w.winter = Box::new(|| true);
        let mut seen = HashSet::new();
        for _ in 0..40 {
            d.set_foreground(Hwnd(5), "A1", WINDOW_CLASS);
            w.step();
            let design = w.design.expect("ein Design");
            assert!(design.is_winter(), "{design:?}");
            seen.insert(design);
            d.set_foreground(Hwnd(9), "Editor", "Notepad");
            w.step();
        }
        assert_eq!(seen.len(), Design::WINTER.len(), "alle drei kommen dran");
        w.winter = Box::new(|| false);
        d.set_foreground(Hwnd(5), "A1", WINDOW_CLASS);
        w.step();
        assert!(!w.design.unwrap().is_winter(), "nach Dreikönig wieder die Stammdesigns, ohne Neustart");
    }

    #[test]
    fn snow_is_white_only_on_a_dark_terminal() {
        let d = Arc::new(FakeDesktop::default());
        d.add_wt(Hwnd(5), "A1", Rect::new(0, 0, 10, 10), false);
        let mut w = FrameWatch::new(d.clone(), hosts(), None, 42);
        d.set_foreground(Hwnd(5), "A1", WINDOW_CLASS);
        w.step();
        assert!(!w.night, "ohne Auskunft hell, wie der Rand");
        d.set_light(Some(false));
        d.set_foreground(Hwnd(9), "Editor", "Notepad");
        w.step();
        assert!(w.night);
        d.set_light(Some(true));
        d.set_foreground(Hwnd(5), "A1", WINDOW_CLASS);
        w.step();
        assert!(!w.night);
    }

    fn group() -> Option<TaskbarGroup> {
        Some(TaskbarGroup { app_id: "Agents".into(), icon: "a.ico".into(), name: "Agent Deck".into() })
    }

    #[test]
    fn each_agent_window_joins_the_taskbar_group_once_and_foreign_windows_never() {
        let d = Arc::new(FakeDesktop::default());
        d.add_wt(Hwnd(5), "A1", Rect::new(0, 0, 10, 10), false);
        d.add_wt(Hwnd(6), "PowerShell", Rect::new(0, 0, 10, 10), false);
        let mut w = FrameWatch::new(d.clone(), hosts(), group(), 42);
        d.set_foreground(Hwnd(5), "A1", WINDOW_CLASS);
        w.step();
        d.set_light(Some(false));
        d.set_foreground(Hwnd(6), "PowerShell", WINDOW_CLASS);
        w.step();
        assert_eq!(d.grouped(), vec![(Hwnd(5), "Agents".to_string())], "einmal, auch über einen Moduswechsel");
    }

    #[test]
    fn a_reused_handle_of_a_new_agent_window_joins_again() {
        let d = Arc::new(FakeDesktop::default());
        d.add_wt(Hwnd(5), "A1", Rect::new(0, 0, 10, 10), false);
        let mut w = FrameWatch::new(d.clone(), hosts(), group(), 42);
        d.set_foreground(Hwnd(5), "A1", WINDOW_CLASS);
        w.step();
        d.remove_wt(Hwnd(5));
        d.set_foreground(Hwnd(9), "Editor", "Notepad");
        w.step();
        d.add_wt(Hwnd(5), "A2", Rect::new(0, 0, 10, 10), false);
        d.set_foreground(Hwnd(5), "A2", WINDOW_CLASS);
        w.step();
        assert_eq!(d.grouped().len(), 2);
        assert_eq!(d.cornered(), vec![Hwnd(5), Hwnd(5)], "auch Ecken und Rand kommen neu");
    }

    #[test]
    fn without_a_group_the_windows_stay_with_the_terminal() {
        let d = Arc::new(FakeDesktop::default());
        d.add_wt(Hwnd(5), "A1", Rect::new(0, 0, 10, 10), false);
        let mut w = FrameWatch::new(d.clone(), hosts(), None, 42);
        d.set_foreground(Hwnd(5), "A1", WINDOW_CLASS);
        w.step();
        assert!(d.grouped().is_empty());
    }
}
