//! Ein gemeinsamer Schatten für bündig liegende Agentenfenster (`deck_core::group_shadow`).
//!
//! [`ShadowWatch`] schaut, welche Agentenfenster Kante an Kante liegen. Denen stellt es
//! den Schatten von Windows ab (er fiele sonst auf den Nachbarn) und plant je Gruppe
//! einen gemeinsamen. Frei liegende und gestapelte Fenster behalten ihren eigenen. Im
//! selben Bild liegt der Statusrand: um jedes Fenster der Gruppe ein feiner Ring in der
//! Farbe seines Zustands ([`StatusMap`]). Zusätzlich bekommt JEDES Agentenfenster seinen
//! Ring als eigenes Fenster direkt hinter sich ([`RingLayer`]): der Schatten der Gruppe
//! liegt hinter ihrem untersten Fenster, und wer ein einzelnes nach vorn klickt, während
//! ein fremdes Fenster dazwischen liegt, sähe an ihm sonst gar keinen Rand.
//! [`ShadowLayer`] zeigt die geplanten Schatten in Fenstern aus `deck_win::shadow_window`.
//! Beides läuft im Thread des Fokus-Rahmens (`crate::focus_frame`).

use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

use deck_core::arrange::Area;
use deck_core::group_shadow::{Look, bounds, groups, render, render_ring, ring_px, ring_rgb};
use deck_core::view::Status;
use deck_core::wt::WINDOW_CLASS;
use deck_win::effects::Desktop;
use deck_win::shadow_window::ShadowWindow;
use deck_win::{Hwnd, Rect};

use crate::applog::note;
use crate::wt_host::{HostMap, WtHost};

/// Nahtfarbe zwischen den Fenstern: eine Stufe neben dem Papier des Terminals.
const SEAM_LIGHT: u32 = 0xDFDAD9;
const SEAM_DARK: u32 = 0x2A273F;

/// Zustand je Slot, wie die Kachel ihn zeigt. Der Takt schreibt, der Thread des
/// Fokus-Rahmens liest.
pub type StatusMap = Arc<RwLock<HashMap<String, Status>>>;

/// Die eine Zustandstafel der App.
pub fn statuses() -> StatusMap {
    static MAP: OnceLock<StatusMap> = OnceLock::new();
    MAP.get_or_init(StatusMap::default).clone()
}

/// Der Schatten einer Gruppe: wo ihre Fenster liegen und hinter welchem er liegt.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub rects: Vec<Area>,
    /// Farbe der Naht je Fenster, `0xRRGGBB`.
    pub colors: Vec<u32>,
    /// Das unterste Fenster der Gruppe; der Schatten kommt direkt dahinter.
    pub below: Hwnd,
    pub look: Look,
}

/// Der Ring eines Agentenfensters: wo es liegt, wie breit und in welcher Farbe.
#[derive(Clone, Debug, PartialEq)]
pub struct Ring {
    pub hwnd: Hwnd,
    pub rect: Area,
    pub width: i64,
    pub rgb: u32,
}

pub struct ShadowWatch {
    desktop: Arc<dyn Desktop>,
    hosts: HostMap,
    statuses: StatusMap,
    /// Je Agentenfenster: hat es seinen Schatten von Windows (`true`) oder nicht?
    own_shadow: HashMap<Hwnd, bool>,
    pub plans: Vec<Plan>,
    /// Ein Ring je offenem Agentenfenster, ob es in einer Gruppe liegt oder frei.
    pub rings: Vec<Ring>,
}

impl ShadowWatch {
    pub fn new(desktop: Arc<dyn Desktop>, hosts: HostMap, statuses: StatusMap) -> ShadowWatch {
        ShadowWatch { desktop, hosts, statuses, own_shadow: HashMap::new(), plans: Vec::new(), rings: Vec::new() }
    }

    /// Einmal hinschauen. `true`, wenn sich die geplanten Schatten oder Ringe geändert haben.
    pub fn step(&mut self) -> bool {
        let found: Vec<(Hwnd, Area, String)> = self
            .desktop
            .class_windows(WINDOW_CLASS)
            .into_iter()
            .filter(|w| !w.minimized)
            .filter_map(|w| Some((w.hwnd, WtHost::slot_of_title(&self.hosts, &w.title)?)))
            .filter_map(|(h, slot)| self.desktop.frame_rect(h).map(|r| (h, area(r), slot)))
            .collect();
        let wins: Vec<(Hwnd, Area)> = found.iter().map(|(h, r, _)| (*h, *r)).collect();
        let rects: Vec<Area> = wins.iter().map(|(_, r)| *r).collect();
        let scale = |h: Hwnd| self.desktop.scale_of(h);
        // Der Spalt zwischen zwei Kacheln sind zwei Ringe. Dazu reichlich Luft: ohne Schatten
        // zeichnet Windows ein Fenster ein paar Pixel kleiner (bei 150 % bis zu 4 je
        // Seite). Wäre die Grenze enger, fiele ein Fenster durch das Abstellen aus der
        // Gruppe, bekäme den Schatten zurück, läge wieder bündig, und so fort.
        let tol = wins.iter().map(|(h, _)| 2 * ring_px(scale(*h)) + (6.0 * scale(*h)).round() as i64).max().unwrap_or(8);
        let parts = groups(&rects, tol);

        // Windows-Schatten: aus für Fenster in einer Gruppe, an für alle anderen. Auch
        // „an“ wird einmal gesetzt: ein früheres Deck kann ihn abgestellt haben.
        let grouped: Vec<usize> = parts.iter().flatten().copied().collect();
        self.own_shadow.retain(|h, _| wins.iter().any(|(w, _)| w == h));
        for (i, (hwnd, _)) in wins.iter().enumerate() {
            let own = !grouped.contains(&i);
            if self.own_shadow.get(hwnd) != Some(&own) && self.desktop.set_shadow(*hwnd, own).is_ok() {
                self.own_shadow.insert(*hwnd, own);
            }
        }

        let z = self.desktop.z_order();
        let depth = |h: Hwnd| z.iter().position(|x| *x == h).unwrap_or(usize::MAX);
        let light = self.desktop.light_theme() != Some(false);
        let seam = if light { SEAM_LIGHT } else { SEAM_DARK };
        let status = self.statuses.read().map(|m| m.clone()).unwrap_or_default();
        let color = |i: usize| status.get(&found[i].2).map_or(seam, |s| ring_rgb(*s, light));
        let plans: Vec<Plan> = parts
            .iter()
            .filter_map(|members| {
                let below = members.iter().map(|i| wins[*i].0).max_by_key(|h| depth(*h))?;
                Some(Plan {
                    rects: members.iter().map(|i| rects[*i]).collect(),
                    colors: members.iter().map(|i| color(*i)).collect(),
                    below,
                    look: Look::at(scale(below), seam),
                })
            })
            .collect();
        let rings: Vec<Ring> = found
            .iter()
            .enumerate()
            .map(|(i, (hwnd, rect, _))| Ring { hwnd: *hwnd, rect: *rect, width: ring_px(scale(*hwnd)), rgb: color(i) })
            .collect();
        let changed = plans != self.plans || rings != self.rings;
        self.plans = plans;
        self.rings = rings;
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
            let done = if old.is_some_and(|o| o.rects == plan.rects && o.colors == plan.colors && o.look == plan.look) {
                // Dieselbe Lage, nur ein anderes Fenster ist jetzt das unterste.
                window.restack(plan.below)
            } else {
                match bounds(&plan.rects, &plan.look) {
                    Some(b) => {
                        let rect = Rect::new(b.0 as i32, b.1 as i32, b.2 as i32, b.3 as i32);
                        window.show(rect, &render(&plan.rects, &plan.colors, b, &plan.look), plan.below)
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
            // Direkt dahinter oder eins weiter: dazwischen darf der Ring des Fensters liegen.
            if let (Some(own), Some(below)) = (at(window.hwnd()), at(plan.below))
                && !(below + 1..=below + 2).contains(&own)
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

/// Die Ringfenster, eines je Agentenfenster, jedes direkt hinter seinem Fenster.
#[derive(Default)]
pub struct RingLayer {
    windows: HashMap<Hwnd, (ShadowWindow, Option<Ring>)>,
    failed: bool,
}

impl RingLayer {
    /// Die Fenster auf den Stand von `rings` bringen. Gezeichnet wird nur, was sich
    /// geändert hat; der Ring eines geschlossenen oder minimierten Fensters verschwindet.
    pub fn apply(&mut self, rings: &[Ring]) {
        self.windows.retain(|h, _| rings.iter().any(|r| r.hwnd == *h));
        for ring in rings {
            if !self.windows.contains_key(&ring.hwnd) {
                match ShadowWindow::new() {
                    Ok(w) => {
                        self.windows.insert(ring.hwnd, (w, None));
                    }
                    Err(e) => {
                        self.fail(format!("Statusrand nicht verfügbar: {e}"));
                        continue;
                    }
                }
            }
            let Some((window, shown)) = self.windows.get_mut(&ring.hwnd) else { continue };
            if shown.as_ref() == Some(ring) {
                continue;
            }
            let (r, m) = (ring.rect, ring.width);
            let rect = Rect::new((r.0 - m) as i32, (r.1 - m) as i32, (r.2 + m) as i32, (r.3 + m) as i32);
            let image = render_ring(r.2 - r.0 + 2 * m, r.3 - r.1 + 2 * m, m, ring.rgb);
            match window.show(rect, &image, ring.hwnd) {
                Ok(()) => *shown = Some(ring.clone()),
                Err(e) => self.fail(format!("Statusrand: {e}")),
            }
        }
    }

    /// Liegt jeder Ring noch direkt hinter seinem Fenster?
    pub fn keep_behind(&mut self, z: &[Hwnd]) {
        let at = |h: Hwnd| z.iter().position(|x| *x == h);
        for (hwnd, (window, _)) in self.windows.iter_mut() {
            if let (Some(own), Some(front)) = (at(window.hwnd()), at(*hwnd))
                && own != front + 1
            {
                let _ = window.restack(*hwnd);
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

    fn watch(d: &Arc<FakeDesktop>) -> ShadowWatch {
        ShadowWatch::new(d.clone(), hosts(), StatusMap::default())
    }

    #[test]
    fn each_side_of_the_seam_takes_the_color_of_its_agents_status_and_follows_it() {
        let d = Arc::new(FakeDesktop::default());
        d.add_wt(Hwnd(11), "A1", Rect::new(0, 0, 799, 600), false);
        d.add_wt(Hwnd(12), "A2", Rect::new(800, 0, 1600, 600), false);
        let statuses = StatusMap::default();
        statuses.write().unwrap().insert("A1".into(), Status::Waiting);
        let mut w = ShadowWatch::new(d.clone(), hosts(), statuses.clone());
        w.step();
        assert_eq!(w.plans[0].colors, [0xE8813F, SEAM_LIGHT], "Rückfrage orange, ohne Zustand neutral");
        assert_eq!(
            w.rings,
            [
                Ring { hwnd: Hwnd(11), rect: (0, 0, 799, 600), width: 1, rgb: 0xE8813F },
                Ring { hwnd: Hwnd(12), rect: (800, 0, 1600, 600), width: 1, rgb: SEAM_LIGHT },
            ],
            "jedes Fenster trägt seinen eigenen Ring"
        );
        statuses.write().unwrap().insert("A2".into(), Status::Done);
        assert!(w.step(), "ein neuer Zustand ist ein neuer Plan");
        assert_eq!(w.plans[0].colors, [0xE8813F, 0x4FA36A]);
        d.set_light(Some(false));
        w.step();
        assert_eq!(w.plans[0].colors, [0xF5A27C, 0x86D2A3], "dunkel die hellen Pigmente");
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
        let mut w = watch(&d);
        assert!(w.step());
        assert_eq!(d.shadows(), [(Hwnd(11), false), (Hwnd(12), false), (Hwnd(13), true)], "fremde Fenster bleiben unberührt");
        assert_eq!(w.plans.len(), 1);
        assert_eq!(w.rings.iter().map(|r| r.hwnd).collect::<Vec<_>>(), [Hwnd(11), Hwnd(12), Hwnd(13)], "auch das frei liegende A3");
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
        let mut w = watch(&d);
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
        let mut w = watch(&d);
        w.step();
        d.raise(Hwnd(12)).unwrap();
        assert!(w.step());
        assert_eq!(w.plans[0].below, Hwnd(11));
    }
}
