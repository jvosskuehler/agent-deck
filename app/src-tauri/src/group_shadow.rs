//! Ein gemeinsamer Schatten für bündig liegende Agentenfenster (`deck_core::group_shadow`).
//!
//! [`ShadowWatch`] schaut, welche Agentenfenster Kante an Kante liegen. Denen stellt es
//! den Schatten von Windows ab (er fiele sonst auf den Nachbarn) und plant je Gruppe
//! einen gemeinsamen. Frei liegende und gestapelte Fenster behalten ihren eigenen.
//! [`ShadowLayer`] zeigt die geplanten Schatten in Fenstern aus `deck_win::shadow_window`.
//! Beides läuft im Thread des Fokus-Rahmens (`crate::focus_frame`).

use std::collections::HashMap;
use std::sync::Arc;

use deck_core::arrange::Area;
use deck_core::group_shadow::{Look, bounds, groups, render, seam_px};
use deck_core::wt::WINDOW_CLASS;
use deck_win::effects::Desktop;
use deck_win::shadow_window::ShadowWindow;
use deck_win::{Hwnd, Rect};

use crate::applog::note;
use crate::wt_host::{HostMap, WtHost};

/// Nahtfarbe zwischen den Fenstern: eine Stufe neben dem Papier des Terminals.
const SEAM_LIGHT: u32 = 0xDFDAD9;
const SEAM_DARK: u32 = 0x2A273F;

/// Der Schatten einer Gruppe: wo ihre Fenster liegen und hinter welchem er liegt.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub rects: Vec<Area>,
    /// Das unterste Fenster der Gruppe; der Schatten kommt direkt dahinter.
    pub below: Hwnd,
    pub look: Look,
}

pub struct ShadowWatch {
    desktop: Arc<dyn Desktop>,
    hosts: HostMap,
    /// Je Agentenfenster: hat es seinen Schatten von Windows (`true`) oder nicht?
    own_shadow: HashMap<Hwnd, bool>,
    pub plans: Vec<Plan>,
}

impl ShadowWatch {
    pub fn new(desktop: Arc<dyn Desktop>, hosts: HostMap) -> ShadowWatch {
        ShadowWatch { desktop, hosts, own_shadow: HashMap::new(), plans: Vec::new() }
    }

    /// Einmal hinschauen. `true`, wenn sich die geplanten Schatten geändert haben.
    pub fn step(&mut self) -> bool {
        let wins: Vec<(Hwnd, Area)> = self
            .desktop
            .class_windows(WINDOW_CLASS)
            .into_iter()
            .filter(|w| !w.minimized && WtHost::slot_of_title(&self.hosts, &w.title).is_some())
            .filter_map(|w| self.desktop.frame_rect(w.hwnd).map(|r| (w.hwnd, area(r))))
            .collect();
        let rects: Vec<Area> = wins.iter().map(|(_, r)| *r).collect();
        let scale = |h: Hwnd| self.desktop.scale_of(h);
        // Der Spalt zwischen zwei Kacheln ist die Naht. Dazu reichlich Luft: ohne Schatten
        // zeichnet Windows ein Fenster ein paar Pixel kleiner (bei 150 % bis zu 4 je
        // Seite). Wäre die Grenze enger, fiele ein Fenster durch das Abstellen aus der
        // Gruppe, bekäme den Schatten zurück, läge wieder bündig, und so fort.
        let tol = wins.iter().map(|(h, _)| seam_px(scale(*h)) + (6.0 * scale(*h)).round() as i64).max().unwrap_or(7);
        let found = groups(&rects, tol);

        // Windows-Schatten: aus für Fenster in einer Gruppe, an für alle anderen. Auch
        // „an“ wird einmal gesetzt: ein früheres Deck kann ihn abgestellt haben.
        let grouped: Vec<usize> = found.iter().flatten().copied().collect();
        self.own_shadow.retain(|h, _| wins.iter().any(|(w, _)| w == h));
        for (i, (hwnd, _)) in wins.iter().enumerate() {
            let own = !grouped.contains(&i);
            if self.own_shadow.get(hwnd) != Some(&own) && self.desktop.set_shadow(*hwnd, own).is_ok() {
                self.own_shadow.insert(*hwnd, own);
            }
        }

        let z = self.desktop.z_order();
        let depth = |h: Hwnd| z.iter().position(|x| *x == h).unwrap_or(usize::MAX);
        let seam = if self.desktop.light_theme() == Some(false) { SEAM_DARK } else { SEAM_LIGHT };
        let plans: Vec<Plan> = found
            .iter()
            .filter_map(|members| {
                let below = members.iter().map(|i| wins[*i].0).max_by_key(|h| depth(*h))?;
                Some(Plan {
                    rects: members.iter().map(|i| rects[*i]).collect(),
                    below,
                    look: Look::at(scale(below), seam),
                })
            })
            .collect();
        let changed = plans != self.plans;
        self.plans = plans;
        changed
    }
}

fn area(r: Rect) -> Area {
    (i64::from(r.left), i64::from(r.top), i64::from(r.right), i64::from(r.bottom))
}

/// Die Schattenfenster, eines je Gruppe. Gezeichnet wird nur, was sich geändert hat.
#[derive(Default)]
pub struct ShadowLayer {
    windows: Vec<ShadowWindow>,
    shown: Vec<Plan>,
    failed: bool,
}

impl ShadowLayer {
    /// Die Fenster auf den Stand von `plans` bringen.
    pub fn apply(&mut self, plans: &[Plan]) {
        while self.windows.len() < plans.len() {
            match ShadowWindow::new() {
                Ok(w) => self.windows.push(w),
                Err(e) => {
                    self.fail(format!("Gruppenschatten nicht verfügbar: {e}"));
                    break;
                }
            }
        }
        for (i, window) in self.windows.iter_mut().enumerate() {
            let Some(plan) = plans.get(i) else {
                window.hide();
                continue;
            };
            let old = self.shown.get(i);
            let done = if old.is_some_and(|o| o.rects == plan.rects && o.look == plan.look) {
                // Dieselbe Lage, nur ein anderes Fenster ist jetzt das unterste.
                window.restack(plan.below)
            } else {
                match bounds(&plan.rects, &plan.look) {
                    Some(b) => {
                        let rect = Rect::new(b.0 as i32, b.1 as i32, b.2 as i32, b.3 as i32);
                        window.show(rect, &render(&plan.rects, b, &plan.look), plan.below)
                    }
                    None => Ok(()),
                }
            };
            if let Err(e) = done
                && !self.failed
            {
                self.failed = true;
                note(format!("Gruppenschatten: {e}"));
            }
        }
        self.shown = plans.to_vec();
    }

    /// Liegt jedes Schattenfenster noch direkt hinter seinem Fenster? Ein Klick auf ein
    /// anderes Programm und zurück kann die Folge ändern, ohne dass sich ein Plan ändert.
    pub fn keep_behind(&mut self, z: &[Hwnd]) {
        for (window, plan) in self.windows.iter_mut().zip(&self.shown) {
            let at = |h: Hwnd| z.iter().position(|x| *x == h);
            if let (Some(own), Some(below)) = (at(window.hwnd()), at(plan.below))
                && own != below + 1
            {
                let _ = window.restack(plan.below);
            }
        }
    }

    fn fail(&mut self, text: String) {
        if !self.failed {
            self.failed = true;
            note(text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeDesktop;
    use std::collections::BTreeMap;
    use std::sync::RwLock;

    fn hosts() -> HostMap {
        Arc::new(RwLock::new(BTreeMap::from([("A".to_string(), "C:/r".to_string())])))
    }

    #[test]
    fn flush_agents_lose_their_own_shadow_and_share_one_behind_the_lowest() {
        let d = Arc::new(FakeDesktop::default());
        d.add_wt(Hwnd(11), "A1", Rect::new(0, 0, 799, 600), false);
        d.add_wt(Hwnd(12), "A2", Rect::new(800, 0, 1600, 600), false);
        d.add_wt(Hwnd(13), "A3", Rect::new(3000, 0, 3800, 600), false);
        d.add_wt(Hwnd(14), "PowerShell", Rect::new(1601, 0, 2400, 600), false);
        // A2 liegt unter A1.
        d.raise(Hwnd(12)).unwrap();
        d.raise(Hwnd(11)).unwrap();
        let mut w = ShadowWatch::new(d.clone(), hosts());
        assert!(w.step());
        assert_eq!(d.shadows(), [(Hwnd(11), false), (Hwnd(12), false), (Hwnd(13), true)], "fremde Fenster bleiben unberührt");
        assert_eq!(w.plans.len(), 1);
        assert_eq!(w.plans[0].rects, [(0, 0, 799, 600), (800, 0, 1600, 600)]);
        assert_eq!(w.plans[0].below, Hwnd(12));
        assert!(!w.step(), "nichts bewegt: nichts neu");
        assert_eq!(d.shadows().len(), 3, "und nichts noch einmal gesetzt");
    }

    #[test]
    fn a_window_dragged_out_of_the_block_gets_its_own_shadow_back() {
        let d = Arc::new(FakeDesktop::default());
        d.add_wt(Hwnd(11), "A1", Rect::new(0, 0, 800, 600), false);
        d.add_wt(Hwnd(12), "A2", Rect::new(800, 0, 1600, 600), false);
        let mut w = ShadowWatch::new(d.clone(), hosts());
        w.step();
        d.move_wt(Hwnd(12), Rect::new(900, 300, 1700, 900));
        assert!(w.step());
        assert!(w.plans.is_empty());
        assert_eq!(d.shadows()[2..], [(Hwnd(11), true), (Hwnd(12), true)]);
    }

    #[test]
    fn another_window_becoming_the_lowest_changes_the_plan() {
        let d = Arc::new(FakeDesktop::default());
        d.add_wt(Hwnd(11), "A1", Rect::new(0, 0, 800, 600), false);
        d.add_wt(Hwnd(12), "A2", Rect::new(800, 0, 1600, 600), false);
        d.raise(Hwnd(12)).unwrap();
        d.raise(Hwnd(11)).unwrap();
        let mut w = ShadowWatch::new(d.clone(), hosts());
        w.step();
        d.raise(Hwnd(12)).unwrap();
        assert!(w.step());
        assert_eq!(w.plans[0].below, Hwnd(11));
    }
}
