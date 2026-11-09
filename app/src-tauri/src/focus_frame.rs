//! Der leuchtende Rahmen um das Windows-Terminal-Agentenfenster, in das gerade getippt
//! wird (`deck/ui/focus_frame.py`). Gezeichnet auf der Grafikkarte mit dem Rahmen aus
//! `deck_win::frame_gpu_overlay`; dieses Modul entscheidet, ob und um welches Fenster.
//!
//! Bei jedem Fokuswechsel kommt ein zufälliges Design dran, nie zweimal dasselbe
//! hintereinander. Die Agentenfenster bekommen eckige Ecken: ein eckiger Rahmen um eine
//! runde Win11-Ecke ließe dort den Hintergrund durchscheinen. Das geschieht bei jedem
//! Fokuswechsel für alle Agentenfenster, damit gekachelte Fenster gleich aussehen.
//! Dazu bekommt ihr Rand die Grundfarbe des Terminals: der Win11-Rand ist sonst
//! halbdurchsichtig, zwischen zwei Kacheln schien eine Fuge Hintergrund durch.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use deck_core::wt::{WINDOW_CLASS, paper_for};
use deck_win::Hwnd;
use deck_win::effects::Desktop;
use deck_win::focus::Corners;
use deck_win::frame_fx::Design;

use crate::applog::note;
use crate::wt_host::{HostMap, WtHost};

/// So oft wird geschaut, welches Fenster vorn ist.
const WATCH: Duration = Duration::from_millis(60);

/// Welches Fenster bekommt den Rahmen, mit welchem Design?
pub struct FrameWatch {
    desktop: Arc<dyn Desktop>,
    hosts: HostMap,
    last_fg: Option<Hwnd>,
    squared: HashSet<Hwnd>,
    /// Modus (hell/dunkel), in dem `squared` gestylt wurde.
    styled_light: Option<Option<bool>>,
    pub target: Option<Hwnd>,
    pub design: Option<Design>,
    pub seed: i32,
    rng: u64,
}

impl FrameWatch {
    pub fn new(desktop: Arc<dyn Desktop>, hosts: HostMap, seed: u64) -> FrameWatch {
        FrameWatch { desktop, hosts, last_fg: None, squared: HashSet::new(), styled_light: None, target: None, design: None, seed: 0, rng: seed | 1 }
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

    fn square_agents(&mut self) {
        let light = self.desktop.light_theme();
        if self.styled_light != Some(light) {
            self.styled_light = Some(light);
            self.squared.clear();
        }
        let border = paper_for(light);
        for w in self.desktop.class_windows(WINDOW_CLASS) {
            if !self.squared.contains(&w.hwnd) && self.is_agent_title(&w.title) {
                let _ = self.desktop.set_corners(w.hwnd, Corners::Square);
                let _ = self.desktop.set_border(w.hwnd, border);
                self.squared.insert(w.hwnd);
            }
        }
    }

    /// Einmal hinschauen. `true`, wenn sich das Ziel geändert hat.
    pub fn step(&mut self) -> bool {
        let fg = self.desktop.foreground();
        let fg_hwnd = fg.as_ref().map(|w| w.hwnd);
        if fg_hwnd != self.last_fg {
            self.last_fg = fg_hwnd;
            self.square_agents();
        }
        let target = fg
            .filter(|w| w.class == WINDOW_CLASS && self.is_agent_title(&w.title) && !self.desktop.is_minimized(w.hwnd))
            .map(|w| w.hwnd);
        if target == self.target {
            return false;
        }
        self.target = target;
        if target.is_some() {
            let r = self.random();
            self.design = Design::pick(&Design::ALL, self.design, r);
            self.seed = (self.random() % (1 << 30)) as i32;
        }
        true
    }
}

/// Den Rahmen-Thread starten. Fehlt die Grafikkarte, gibt es eben keinen Rahmen (einmal
/// ins Log), das Deck läuft weiter.
pub fn start(desktop: Arc<dyn Desktop>, hosts: HostMap) {
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(7);
    std::thread::Builder::new()
        .name("focus-frame".into())
        .spawn(move || {
            let mut frame = match deck_win::frame_gpu_overlay::GpuFocusFrame::new() {
                Ok(f) => f,
                Err(e) => {
                    note(format!("Fokus-Rahmen nicht verfügbar: {e}"));
                    return;
                }
            };
            let mut watch = FrameWatch::new(desktop, hosts, seed);
            let mut since_watch = Instant::now();
            let mut t0 = Instant::now();
            let mut logged = false;
            loop {
                deck_win::overlay::pump_messages();
                if since_watch.elapsed() >= WATCH || watch.target.is_none() {
                    since_watch = Instant::now();
                    if watch.step() {
                        t0 = Instant::now();
                    }
                }
                match (watch.target, watch.design) {
                    (Some(target), Some(design)) => {
                        // draw wartet auf den Bildwechsel: das ist der Takt der Animation.
                        if let Err(e) = frame.draw(target, design, watch.seed, t0.elapsed().as_secs_f64())
                            && !logged
                        {
                            note(format!("Fokus-Rahmen: {e}"));
                            logged = true;
                            std::thread::sleep(WATCH);
                        }
                    }
                    _ => {
                        frame.hide();
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
    fn only_a_focused_agent_window_of_a_terminal_block_gets_the_frame() {
        let d = Arc::new(FakeDesktop::default());
        d.add_wt(Hwnd(5), "A1", Rect::new(0, 0, 10, 10), false);
        d.add_wt(Hwnd(6), "B1", Rect::new(0, 0, 10, 10), false);
        let mut w = FrameWatch::new(d.clone(), hosts(), 42);
        d.set_foreground(Hwnd(6), "B1", WINDOW_CLASS);
        assert!(!w.step(), "B ist kein Terminal-Block");
        d.set_foreground(Hwnd(5), "A1", WINDOW_CLASS);
        assert!(w.step());
        assert_eq!(w.target, Some(Hwnd(5)));
        let first = w.design;
        d.set_foreground(Hwnd(9), "Editor", "Chrome_WidgetWin_1");
        assert!(w.step());
        assert_eq!(w.target, None);
        d.set_foreground(Hwnd(5), "A1", WINDOW_CLASS);
        w.step();
        assert_ne!(w.design, first, "nie zweimal dasselbe Design hintereinander");
        assert_eq!(d.cornered(), vec![Hwnd(5)], "nur Agentenfenster werden eckig");
        assert_eq!(d.bordered(), vec![(Hwnd(5), 0xFAF4ED)], "Rand in Papierfarbe, ohne Auskunft hell");
    }

    #[test]
    fn border_follows_the_windows_theme() {
        let d = Arc::new(FakeDesktop::default());
        d.add_wt(Hwnd(5), "A1", Rect::new(0, 0, 10, 10), false);
        let mut w = FrameWatch::new(d.clone(), hosts(), 42);
        d.set_light(Some(false));
        d.set_foreground(Hwnd(5), "A1", WINDOW_CLASS);
        w.step();
        d.set_foreground(Hwnd(9), "Editor", "Chrome_WidgetWin_1");
        w.step();
        assert_eq!(d.bordered(), vec![(Hwnd(5), 0x191724)], "gleicher Modus: nicht neu stylen");
        d.set_light(Some(true));
        d.set_foreground(Hwnd(5), "A1", WINDOW_CLASS);
        w.step();
        assert_eq!(d.bordered(), vec![(Hwnd(5), 0x191724), (Hwnd(5), 0xFAF4ED)], "Moduswechsel färbt neu");
    }
}
