//! Andocken an der Bildschirmkante: wo Panel und Griff liegen und wann das Panel
//! auf- oder zuklappt. Die Rechnung hinter `deck/dock/` der Python-Fassung, ohne Fenster.
//!
//! Koordinaten sind physische Bildschirmpixel (Per-Monitor-V2), Rechtecke (links, oben,
//! rechts, unten). Maße der Oberfläche (Griff 152 × 30, Schattenraum 24 px) sind
//! CSS-Pixel und werden mit der Skalierung des Monitors umgerechnet.
//!
//! Auf- und Zuklappen ist eine kleine Zustandsmaschine ([`AutoHide`]), die ein Takt
//! (70 ms wie `dock/poll.py`) mit der Zeigerposition füttert: Griff berührt → auf,
//! Zeiger länger als 320 ms außerhalb des Panels → zu. Sie kennt genau zwei Zustände,
//! offen und zu; ein halb offenes Deck gibt es hier nicht.

/// Ein Rechteck in Bildschirmpixeln (links, oben, rechts, unten).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Area {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Area {
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Area {
        Area { left, top, right, bottom }
    }

    pub const fn width(&self) -> i32 {
        self.right - self.left
    }

    pub const fn height(&self) -> i32 {
        self.bottom - self.top
    }

    /// Liegt der Punkt im um `margin` vergrößerten Rechteck?
    pub const fn contains(&self, x: i32, y: i32, margin: i32) -> bool {
        x >= self.left - margin && x < self.right + margin && y >= self.top - margin && y < self.bottom + margin
    }
}

/// Die Kante, an der das Deck andockt. `Off` = frei schwebend, nie eingeklappt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Top,
    Left,
    Right,
    Bottom,
    Off,
}

impl Edge {
    /// Wert aus `deck_settings.json`. Unbekanntes und Fehlendes ist `Off`, wie `_norm`
    /// in Python: lieber ein sichtbares Fenster als eines hinter einer erfundenen Kante.
    pub fn parse(value: Option<&str>) -> Edge {
        match value.map(str::trim) {
            Some("top") => Edge::Top,
            Some("left") => Edge::Left,
            Some("right") => Edge::Right,
            Some("bottom") => Edge::Bottom,
            _ => Edge::Off,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Edge::Top => "top",
            Edge::Left => "left",
            Edge::Right => "right",
            Edge::Bottom => "bottom",
            Edge::Off => "off",
        }
    }

    /// Senkrechte Kante (links/rechts): der Griff steht hochkant, „entlang“ ist y.
    pub const fn vertical(self) -> bool {
        matches!(self, Edge::Left | Edge::Right)
    }
}

/// Länge des Griff-Fensters entlang der Kante (CSS-Pixel, IPC.md).
pub const HANDLE_LONG: f64 = 152.0;
/// Dicke des Griff-Fensters quer zur Kante (CSS-Pixel).
pub const HANDLE_SHORT: f64 = 30.0;
/// So tief (von der Kante aus) klappt die Berührung des Griffs auf. Wie die Kapsel in
/// Python (`capsule_extent`): der Rest des Griff-Fensters ist Raum für den Schein, und
/// ein Zeiger, der nur an der Kante entlangstreift, soll das Deck nicht aufreißen.
pub const REVEAL_DEPTH: f64 = 16.0;
/// Kulanz um das Panel, bevor „Zeiger draußen“ gilt (`INSIDE_MARGIN`).
pub const INSIDE_MARGIN: f64 = 8.0;
/// Schattenraum des Panel-Fensters an der freien Seite (CSS-Pixel, IPC.md).
pub const SHADOW_GUTTER: f64 = 24.0;
/// Takt des Zeiger-Polls (ms).
pub const POLL_MS: u64 = 70;
/// So lange muss der Zeiger draußen sein, bevor eingeklappt wird (ms).
pub const COLLAPSE_DELAY_MS: f64 = 320.0;
/// Von außen aufgeklappt (Zweitstart): so lange offen halten (ms).
pub const REQUEST_HOLD_MS: f64 = 4000.0;
/// Nach dem Einklappen so lange kein Aufklappen über den Poll (ms), damit ein Zeiger,
/// der zufällig über dem Griff steht, das Deck nicht im selben Atemzug wieder aufreißt.
pub const REVEAL_LOCK_MS: f64 = 220.0;
/// Nach `phase: closed` so lange warten, bevor das Panel-Fenster verschwindet (ms):
/// die Oberfläche gleitet 180 ms hinaus.
pub const HIDE_AFTER_CLOSE_MS: u64 = 200;

fn px(css: f64, scale: f64) -> i32 {
    (css * scale).round() as i32
}

fn clamp(v: i32, lo: i32, hi: i32) -> i32 {
    v.max(lo).min(hi.max(lo))
}

/// Startpunkt des Griffs entlang der Kante. `along` ist der gemerkte Wert
/// (`dock_along`, linke bzw. obere Kante des Griffs); fehlt er, steht der Griff mittig.
/// Immer in die Arbeitsfläche geklemmt: ein Monitorwechsel darf ihn nicht verlieren.
pub fn handle_start(edge: Edge, work: Area, along: Option<i32>, scale: f64) -> i32 {
    let len = px(HANDLE_LONG, scale);
    let (lo, hi) = if edge.vertical() { (work.top, work.bottom) } else { (work.left, work.right) };
    let start = along.unwrap_or(lo + (hi - lo - len) / 2);
    clamp(start, lo, hi - len)
}

/// Fenster des Griffs: bündig an der Kante, `HANDLE_LONG` lang.
pub fn handle_rect(edge: Edge, work: Area, along: Option<i32>, scale: f64) -> Area {
    let (long, short) = (px(HANDLE_LONG, scale), px(HANDLE_SHORT, scale));
    let s = handle_start(edge, work, along, scale);
    match edge {
        Edge::Top | Edge::Off => Area::new(s, work.top, s + long, work.top + short),
        Edge::Bottom => Area::new(s, work.bottom - short, s + long, work.bottom),
        Edge::Left => Area::new(work.left, s, work.left + short, s + long),
        Edge::Right => Area::new(work.right - short, s, work.right, s + long),
    }
}

/// Der Teil des Griffs, dessen Berührung aufklappt: die ersten `REVEAL_DEPTH` Pixel ab
/// der Kante.
pub fn reveal_zone(edge: Edge, handle: Area, scale: f64) -> Area {
    let d = px(REVEAL_DEPTH, scale);
    match edge {
        Edge::Top | Edge::Off => Area { bottom: handle.top + d, ..handle },
        Edge::Bottom => Area { top: handle.bottom - d, ..handle },
        Edge::Left => Area { right: handle.left + d, ..handle },
        Edge::Right => Area { left: handle.right - d, ..handle },
    }
}

/// Fenster des Panels: `size` ist die gemeldete Inhaltsgröße in Bildschirmpixeln
/// (samt Schattenraum). Angedockt liegt es bündig an der Kante, mittig zum Griff, in
/// die Arbeitsfläche geklemmt; frei (`Off`) an `free` bzw. oben links mit Abstand.
pub fn panel_rect(edge: Edge, work: Area, along: Option<i32>, size: (i32, i32), scale: f64, free: Option<(i32, i32)>) -> Area {
    let w = size.0.clamp(1, work.width().max(1));
    let h = size.1.clamp(1, work.height().max(1));
    let center = handle_start(edge, work, along, scale) + px(HANDLE_LONG, scale) / 2;
    let (x, y) = match edge {
        Edge::Top => (clamp(center - w / 2, work.left, work.right - w), work.top),
        Edge::Bottom => (clamp(center - w / 2, work.left, work.right - w), work.bottom - h),
        Edge::Left => (work.left, clamp(center - h / 2, work.top, work.bottom - h)),
        Edge::Right => (work.right - w, clamp(center - h / 2, work.top, work.bottom - h)),
        Edge::Off => {
            let (fx, fy) = free.unwrap_or((work.left + px(40.0, scale), work.top + px(40.0, scale)));
            (clamp(fx, work.left, work.right - w), clamp(fy, work.top, work.bottom - h))
        }
    };
    Area::new(x, y, x + w, y + h)
}

/// Das sichtbare Panel im Fenster: ohne den Schattenraum an der freien Seite. Nur
/// dieses Rechteck zählt für „Zeiger weg → einklappen“ (IPC.md).
pub fn panel_body(edge: Edge, window: Area, scale: f64) -> Area {
    let g = px(SHADOW_GUTTER, scale);
    match edge {
        Edge::Top | Edge::Off => Area { bottom: (window.bottom - g).max(window.top), ..window },
        Edge::Bottom => Area { top: (window.top + g).min(window.bottom), ..window },
        Edge::Left => Area { right: (window.right - g).max(window.left), ..window },
        Edge::Right => Area { left: (window.left + g).min(window.right), ..window },
    }
}

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

    const WORK: Area = Area::new(0, 0, 1920, 1040);

    fn sight(now: f64, pointer: (i32, i32), body: Area, zone: Area) -> Sight {
        Sight { now, pointer: Some(pointer), body, zone, margin: 8, hold: false }
    }

    #[test]
    fn edge_values_from_settings_fall_back_to_floating() {
        assert_eq!(Edge::parse(Some("top")), Edge::Top);
        assert_eq!(Edge::parse(Some("bottom")), Edge::Bottom);
        assert_eq!(Edge::parse(Some("diagonal")), Edge::Off);
        assert_eq!(Edge::parse(None), Edge::Off);
        assert_eq!(Edge::Right.as_str(), "right");
    }

    #[test]
    fn handle_sits_flush_at_the_edge_and_centered_without_a_saved_position() {
        let h = handle_rect(Edge::Top, WORK, None, 1.0);
        assert_eq!(h, Area::new(884, 0, 1036, 30));
        let r = handle_rect(Edge::Right, WORK, None, 1.5);
        assert_eq!((r.right, r.width(), r.height()), (1920, 45, 228));
    }

    #[test]
    fn saved_handle_position_is_clamped_onto_the_monitor() {
        assert_eq!(handle_rect(Edge::Top, WORK, Some(5000), 1.0).right, 1920);
        assert_eq!(handle_rect(Edge::Left, WORK, Some(-300), 1.0).top, 0);
        assert_eq!(handle_rect(Edge::Bottom, WORK, Some(100), 1.0), Area::new(100, 1010, 252, 1040));
    }

    #[test]
    fn panel_is_centered_on_the_handle_and_kept_on_the_monitor() {
        let p = panel_rect(Edge::Top, WORK, Some(100), (900, 400), 1.0, None);
        // Griffmitte 176, das Panel wäre links draußen → an den Rand geklemmt.
        assert_eq!(p, Area::new(0, 0, 900, 400));
        let p = panel_rect(Edge::Top, WORK, None, (900, 400), 1.0, None);
        assert_eq!((p.left, p.top), (510, 0));
        let p = panel_rect(Edge::Right, WORK, None, (900, 400), 1.0, None);
        assert_eq!((p.right, p.top), (1920, 320));
        let p = panel_rect(Edge::Bottom, WORK, None, (900, 400), 1.0, None);
        assert_eq!(p.bottom, 1040);
    }

    #[test]
    fn panel_larger_than_the_monitor_is_cut_to_it() {
        let p = panel_rect(Edge::Left, WORK, None, (900, 3000), 1.0, None);
        assert_eq!((p.top, p.bottom), (0, 1040));
    }

    #[test]
    fn floating_panel_stays_where_it_was_left() {
        let p = panel_rect(Edge::Off, WORK, None, (400, 300), 1.0, Some((700, 200)));
        assert_eq!((p.left, p.top), (700, 200));
        let p = panel_rect(Edge::Off, WORK, None, (400, 300), 1.0, None);
        assert_eq!((p.left, p.top), (40, 40));
    }

    #[test]
    fn only_the_panel_without_its_shadow_counts_as_inside() {
        let w = Area::new(0, 0, 900, 424);
        assert_eq!(panel_body(Edge::Top, w, 1.0).bottom, 400);
        assert_eq!(panel_body(Edge::Bottom, w, 1.0).top, 24);
        assert_eq!(panel_body(Edge::Left, w, 2.0).right, 852);
        assert_eq!(panel_body(Edge::Right, w, 1.0).left, 24);
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
