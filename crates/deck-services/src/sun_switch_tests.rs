use std::io;
use std::time::Instant;

use super::*;

/// Windows als Attrappe: zwei Werte, jedes Schreiben wird mitgezählt.
#[derive(Default)]
struct FakeTheme {
    values: Mutex<(Option<bool>, Option<bool>)>,
    writes: Mutex<Vec<bool>>,
    broken: Mutex<bool>,
}

impl FakeTheme {
    fn set_by_hand(&self, light: bool) {
        *self.values.lock().unwrap() = (Some(light), Some(light));
    }
    fn writes(&self) -> Vec<bool> {
        self.writes.lock().unwrap().clone()
    }
}

impl WindowsTheme for FakeTheme {
    fn read(&self) -> (Option<bool>, Option<bool>) {
        *self.values.lock().unwrap()
    }
    fn write(&self, light: bool) -> io::Result<()> {
        if *self.broken.lock().unwrap() {
            return Err(io::Error::other("kaputt"));
        }
        self.set_by_hand(light);
        self.writes.lock().unwrap().push(light);
        Ok(())
    }
}

fn hamburg() -> &'static Place {
    sun::place("hamburg").unwrap()
}

/// 17.11.2026 in UTC (Aufgang 06:48, Untergang 15:22).
fn nov17(h: i64, m: i64) -> f64 {
    let midnight = 1_794_873_600; // 2026-11-17T00:00:00Z
    (midnight + h * 3600 + m * 60) as f64
}

fn quiet() -> impl Fn(String) {
    |_| {}
}

#[test]
fn the_reference_day_starts_at_utc_midnight() {
    assert_eq!(utc_stamp(nov17(0, 0) as i64), "2026-11-17 00:00:00Z");
}

#[test]
fn at_start_the_deck_puts_windows_where_the_sun_is() {
    let t = FakeTheme::default();
    t.set_by_hand(true);
    let mut c = Checker::default();
    assert_eq!(c.check(nov17(20, 0), hamburg(), &t, true, &quiet()), Some(false));
    assert_eq!(c.check(nov17(12, 0), hamburg(), &t, true, &quiet()), Some(true));
    assert_eq!(t.writes(), [false, true]);
}

#[test]
fn a_hand_on_the_switch_lasts_until_the_sun_rises_or_sets() {
    let t = FakeTheme::default();
    let mut c = Checker::default();
    c.check(nov17(10, 0), hamburg(), &t, true, &quiet());
    t.set_by_hand(false);
    assert_eq!(c.check(nov17(12, 0), hamburg(), &t, false, &quiet()), None, "Takt ohne Wechsel: von Hand bleibt");
    assert_eq!(t.read(), (Some(false), Some(false)));
    assert_eq!(c.check(nov17(16, 0), hamburg(), &t, false, &quiet()), None, "Untergang: schon dunkel, nichts zu tun");
    assert_eq!(c.check(nov17(23, 0), hamburg(), &t, false, &quiet()), None);
    assert_eq!(c.check(nov17(24 + 7, 0), hamburg(), &t, false, &quiet()), Some(true), "Aufgang am nächsten Morgen");
}

#[test]
fn only_a_switch_is_logged_and_a_lasting_error_once_plus_its_recovery() {
    let t = FakeTheme::default();
    let lines = Mutex::new(Vec::<String>::new());
    let log = |l: String| lines.lock().unwrap().push(l);
    let mut c = Checker::default();
    c.check(nov17(20, 0), hamburg(), &t, true, &log);
    c.check(nov17(20, 10), hamburg(), &t, true, &log);
    c.check(nov17(20, 20), hamburg(), &t, false, &log);
    assert_eq!(lines.lock().unwrap().len(), 1, "ein Umschalten, keine Takte");
    assert!(lines.lock().unwrap()[0].contains("Windows auf dunkel"));
    assert!(lines.lock().unwrap()[0].contains("nächster Wechsel 2026-11-18 06:"), "nennt den nächsten Wechsel");

    *t.broken.lock().unwrap() = true;
    for m in 0..3 {
        c.check(nov17(12, m), hamburg(), &t, true, &log);
    }
    *t.broken.lock().unwrap() = false;
    c.check(nov17(12, 5), hamburg(), &t, true, &log);
    let lines = lines.lock().unwrap();
    assert_eq!(lines.len(), 4, "{lines:?}");
    assert!(lines[1].contains("nicht auf hell"));
    assert!(lines[2].contains("nach 3 Fehlversuchen"));
    assert!(lines[3].contains("Windows auf hell"));
}

#[test]
fn the_timer_aims_just_past_the_next_change_but_never_sleeps_longer_than_the_safety_tick() {
    let at = nov17(15, 15);
    let next = sun::next_change(at, hamburg()).unwrap();
    let w = wait_for(at, hamburg()).as_secs_f64();
    assert!((w - (next - at + 30.0)).abs() < 0.01, "kurz nach dem Untergang");
    assert_eq!(wait_for(nov17(20, 0), hamburg()), SAFETY_TICK);
}

#[test]
fn the_thread_switches_once_a_place_is_set_and_stays_idle_without_one() {
    fn evening() -> f64 {
        1_794_873_600.0 + 20.0 * 3600.0
    }
    let t = Arc::new(FakeTheme::default());
    t.set_by_hand(true);
    let s = SunSwitch::start(t.clone(), evening, Arc::new(|_| {}));
    std::thread::sleep(Duration::from_millis(50));
    assert!(t.writes().is_empty(), "ohne Ort tut er nichts");
    s.set(Some(hamburg()));
    let deadline = Instant::now() + Duration::from_secs(5);
    while t.writes().is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(t.writes(), [false]);
    t.set_by_hand(true);
    s.check_now();
    let deadline = Instant::now() + Duration::from_secs(5);
    while t.writes().len() < 2 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(t.writes(), [false, false], "nach dem Aufwachen wird durchgesetzt");
}
