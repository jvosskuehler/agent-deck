//! Auf- und Zuklappen als kleine Zustandsmaschine: ein Takt füttert sie mit der
//! Zeigerposition, sie sagt, ob jetzt auf- oder zugeklappt wird. Genau zwei Zustände,
//! offen und zu; ein halb offenes Deck gibt es hier nicht.

use super::Area;

/// So lange muss der Zeiger draußen sein, bevor eingeklappt wird (ms).
pub const COLLAPSE_DELAY_MS: f64 = 320.0;
/// Von außen aufgeklappt (Zweitstart): so lange offen halten (ms).
pub const REQUEST_HOLD_MS: f64 = 4000.0;
/// Nach dem Einklappen so lange kein Aufklappen über den Poll (ms), damit ein Zeiger,
/// der zufällig über dem Griff steht, das Deck nicht im selben Atemzug wieder aufreißt.
pub const REVEAL_LOCK_MS: f64 = 220.0;

/// Was der Takt dem Fenster-Code aufträgt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockAction {
    None,
    Reveal,
    Collapse,
}

/// Auf- und Zuklappen über die Zeigerposition. Zeiten in Millisekunden einer
/// monotonen Uhr.
#[derive(Debug, Clone, Default)]
pub struct AutoHide {
    expanded: bool,
    outside_since: Option<f64>,
    hold_until: f64,
    reveal_lock: f64,
}

/// Was ein Takt über die Welt weiß.
#[derive(Debug, Clone, Copy)]
pub struct Sight {
    pub now: f64,
    /// Zeigerposition; `None` = nicht lesbar (dann ändert sich nichts).
    pub pointer: Option<(i32, i32)>,
    /// Sichtbares Panel (ohne Schattenraum).
    pub body: Area,
    /// Aufklapp-Zone des Griffs.
    pub zone: Area,
    /// Kulanz um das Panel in Pixeln.
    pub margin: i32,
    /// Etwas hält das Panel offen (Kontextmenü offen, Ziehen): nicht einklappen.
    pub hold: bool,
}

impl AutoHide {
    /// Startzustand: eingeklappt (beim Start ist nur der Griff zu sehen).
    pub fn collapsed() -> AutoHide {
        AutoHide::default()
    }

    /// Startzustand: offen (aus dem schwebenden Fenster heraus angedockt).
    pub fn expanded() -> AutoHide {
        AutoHide { expanded: true, ..AutoHide::default() }
    }

    pub fn is_expanded(&self) -> bool {
        self.expanded
    }

    /// Ein Takt. Liefert, ob jetzt auf- oder zugeklappt werden soll.
    pub fn step(&mut self, s: Sight) -> DockAction {
        let Some((x, y)) = s.pointer else { return DockAction::None };
        if !self.expanded {
            if s.now < self.reveal_lock || !s.zone.contains(x, y, 0) {
                return DockAction::None;
            }
            return self.reveal();
        }
        if s.hold || s.now < self.hold_until || s.body.contains(x, y, s.margin) {
            // Haltefrist: danach gilt wieder die volle Verzögerung, nicht schlagartig zu.
            self.outside_since = None;
            return DockAction::None;
        }
        match self.outside_since {
            None => {
                self.outside_since = Some(s.now);
                DockAction::None
            }
            Some(t0) if s.now - t0 >= COLLAPSE_DELAY_MS => self.collapse(s.now),
            Some(_) => DockAction::None,
        }
    }

    /// Aufklappen (Hover auf dem Griff oder von außen). Schon offen → nichts.
    pub fn reveal(&mut self) -> DockAction {
        self.outside_since = None;
        if self.expanded {
            return DockAction::None;
        }
        self.expanded = true;
        DockAction::Reveal
    }

    /// Von außen aufklappen (Zweitstart) und eine Weile offen halten: der Zeiger steht
    /// dann nicht auf dem Deck, ohne Haltefrist wäre es nach 320 ms wieder weg.
    pub fn reveal_for_request(&mut self, now: f64) -> DockAction {
        self.hold_until = now + REQUEST_HOLD_MS;
        self.reveal()
    }

    /// Einklappen. Schon zu → nichts.
    pub fn collapse(&mut self, now: f64) -> DockAction {
        self.hold_until = 0.0;
        self.outside_since = None;
        if !self.expanded {
            return DockAction::None;
        }
        self.expanded = false;
        self.reveal_lock = now + REVEAL_LOCK_MS;
        DockAction::Collapse
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dock::{Edge, handle_rect, reveal_zone};

    const WORK: Area = Area::new(0, 0, 1920, 1040);

    fn sight(now: f64, pointer: (i32, i32), body: Area, zone: Area) -> Sight {
        Sight { now, pointer: Some(pointer), body, zone, margin: 8, hold: false }
    }

    #[test]
    fn touching_the_handle_reveals_but_grazing_the_edge_beside_it_does_not() {
        let handle = handle_rect(Edge::Top, WORK, None, 1.0);
        let zone = reveal_zone(Edge::Top, handle, 1.0);
        let body = Area::new(500, 0, 1400, 400);
        let mut a = AutoHide::collapsed();
        assert_eq!(a.step(sight(0.0, (100, 2), body, zone)), DockAction::None);
        assert_eq!(a.step(sight(70.0, (950, 20), body, zone)), DockAction::None, "unter der Kapsel");
        assert_eq!(a.step(sight(140.0, (950, 3), body, zone)), DockAction::Reveal);
        assert!(a.is_expanded());
    }

    #[test]
    fn pointer_outside_the_panel_collapses_only_after_the_delay() {
        let zone = Area::new(0, 0, 10, 10);
        let body = Area::new(500, 0, 1400, 400);
        let mut a = AutoHide::expanded();
        assert_eq!(a.step(sight(0.0, (900, 405), body, zone)), DockAction::None, "Kulanz");
        assert_eq!(a.step(sight(70.0, (900, 600), body, zone)), DockAction::None);
        assert_eq!(a.step(sight(300.0, (900, 600), body, zone)), DockAction::None);
        assert_eq!(a.step(sight(390.0, (900, 600), body, zone)), DockAction::Collapse);
        assert!(!a.is_expanded());
    }

    #[test]
    fn coming_back_in_time_resets_the_collapse_clock() {
        let zone = Area::new(0, 0, 10, 10);
        let body = Area::new(500, 0, 1400, 400);
        let mut a = AutoHide::expanded();
        a.step(sight(0.0, (900, 600), body, zone));
        a.step(sight(200.0, (900, 100), body, zone));
        assert_eq!(a.step(sight(400.0, (900, 600), body, zone)), DockAction::None);
        assert_eq!(a.step(sight(700.0, (900, 600), body, zone)), DockAction::None);
        assert_eq!(a.step(sight(720.0, (900, 600), body, zone)), DockAction::Collapse);
    }

    #[test]
    fn right_after_collapsing_a_pointer_on_the_handle_does_not_reopen_at_once() {
        let zone = Area::new(0, 0, 200, 16);
        let body = Area::new(0, 0, 900, 400);
        let mut a = AutoHide::expanded();
        assert_eq!(a.collapse(1000.0), DockAction::Collapse);
        assert_eq!(a.step(sight(1100.0, (50, 5), body, zone)), DockAction::None);
        assert_eq!(a.step(sight(1230.0, (50, 5), body, zone)), DockAction::Reveal);
    }

    #[test]
    fn a_second_start_keeps_the_deck_open_for_a_while() {
        let zone = Area::new(0, 0, 10, 10);
        let body = Area::new(500, 0, 1400, 400);
        let mut a = AutoHide::collapsed();
        assert_eq!(a.reveal_for_request(0.0), DockAction::Reveal);
        assert_eq!(a.step(sight(3000.0, (0, 900), body, zone)), DockAction::None);
        assert_eq!(a.step(sight(4100.0, (0, 900), body, zone)), DockAction::None, "Uhr startet erst jetzt");
        assert_eq!(a.step(sight(4450.0, (0, 900), body, zone)), DockAction::Collapse);
    }

    #[test]
    fn an_open_menu_holds_the_panel_open() {
        let zone = Area::new(0, 0, 10, 10);
        let body = Area::new(500, 0, 1400, 400);
        let mut a = AutoHide::expanded();
        let held = |now| Sight { hold: true, ..sight(now, (0, 900), body, zone) };
        a.step(held(0.0));
        assert_eq!(a.step(held(1000.0)), DockAction::None);
        assert_eq!(a.step(sight(1100.0, (0, 900), body, zone)), DockAction::None);
    }

    #[test]
    fn unreadable_pointer_changes_nothing() {
        let mut a = AutoHide::expanded();
        let s = Sight { now: 0.0, pointer: None, body: Area::default(), zone: Area::default(), margin: 0, hold: false };
        assert_eq!(a.step(s), DockAction::None);
        assert!(a.is_expanded());
    }

    #[test]
    fn reveal_and_collapse_are_idempotent() {
        let mut a = AutoHide::collapsed();
        assert_eq!(a.collapse(0.0), DockAction::None);
        assert_eq!(a.reveal(), DockAction::Reveal);
        assert_eq!(a.reveal(), DockAction::None);
    }
}
