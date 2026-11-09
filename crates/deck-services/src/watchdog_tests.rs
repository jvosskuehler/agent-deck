//! Tests zum Wächter (aus `tests/test_ops_watchdog.py`, auf den Herzschlag übertragen).
//! Gestartet wird nichts: der Starter ist eine Attrappe, die Uhr steht.

use std::collections::HashMap;
use std::sync::Mutex;

use super::*;
use crate::heartbeat::{self, EndReason};

const APP: &str = r"C:\Programme\Agent Deck\agent-deck.exe";
const NOW: f64 = 10_000.0;

struct FakeProbe(Option<HashMap<u32, &'static str>>);

impl ProcessProbe for FakeProbe {
    fn probe(&self, pid: u32) -> Probe {
        match &self.0 {
            None => Probe::Unknown,
            Some(m) => m.get(&pid).map_or(Probe::Dead, |e| Probe::Alive(e.to_string())),
        }
    }
}

#[derive(Default)]
struct FakeLauncher(Mutex<Vec<PathBuf>>);

impl Launcher for FakeLauncher {
    fn start_detached(&self, exe: &Path) -> bool {
        self.0.lock().unwrap().push(exe.to_path_buf());
        true
    }
}

fn beat(pid: u32, ts: f64, ended: Option<&str>) -> Heartbeat {
    Heartbeat { pid, exe: APP.into(), ts, ended: ended.map(String::from) }
}

fn probe(alive: &[(u32, &'static str)]) -> FakeProbe {
    FakeProbe(Some(alive.iter().copied().collect()))
}

#[test]
fn a_fresh_beat_of_a_living_app_means_running() {
    assert_eq!(app_state(Some(&beat(7, NOW - 5.0, None)), &probe(&[(7, "agent-deck.exe")]), NOW), AppState::Running { pid: 7 });
}

#[test]
fn a_living_app_with_an_old_beat_hangs_and_is_not_doubled() {
    let s = app_state(Some(&beat(7, NOW - 300.0, None)), &probe(&[(7, "Agent-Deck.EXE")]), NOW);
    assert!(matches!(s, AppState::Hanging { pid: 7, .. }), "{s:?}");
}

#[test]
fn a_recycled_pid_means_gone() {
    let s = app_state(Some(&beat(7, NOW, None)), &probe(&[(7, "notepad.exe")]), NOW);
    assert!(matches!(&s, AppState::Gone { hint } if hint.contains("Fremdprozess")), "{s:?}");
}

#[test]
fn a_dead_pid_or_no_beat_means_gone() {
    assert!(matches!(app_state(Some(&beat(7, NOW, None)), &probe(&[]), NOW), AppState::Gone { .. }));
    assert!(matches!(app_state(None, &probe(&[]), NOW), AppState::Gone { .. }));
}

#[test]
fn an_unreadable_process_list_is_not_gone() {
    assert_eq!(app_state(Some(&beat(7, NOW, None)), &FakeProbe(None), NOW), AppState::Unknown);
}

#[test]
fn the_watchdog_sees_a_clean_end() {
    // Sonst käme das Deck nach jedem Schließen zurück.
    assert!(last_end(Some(&beat(7, NOW, Some("exit")))).starts_with(CLEAN_END));
}

#[test]
fn the_watchdog_sees_a_kill_from_outside() {
    let f = last_end(Some(&beat(7, NOW, None)));
    assert!(f.contains("ABGESCHOSSEN") && !f.starts_with(CLEAN_END));
}

#[test]
fn a_failed_self_restart_and_a_panic_are_restarted() {
    for e in ["restart", "panic"] {
        assert!(!last_end(Some(&beat(7, NOW, Some(e)))).starts_with(CLEAN_END), "{e}");
    }
}

struct Rig {
    _d: tempfile::TempDir,
    wd: Watchdog,
    launched: Arc<FakeLauncher>,
    slept: Arc<Mutex<Vec<Duration>>>,
}

fn rig(alive: &[(u32, &'static str)], app_exe: Option<PathBuf>) -> Rig {
    let d = tempfile::tempdir().unwrap();
    let launched = Arc::new(FakeLauncher::default());
    let slept = Arc::new(Mutex::new(Vec::new()));
    let s = slept.clone();
    let wd = Watchdog {
        app_dir: d.path().to_path_buf(),
        app_exe,
        probe: Arc::new(probe(alive)),
        launcher: launched.clone(),
        clock: Arc::new(|| NOW),
        sleep: Arc::new(move |t| s.lock().unwrap().push(t)),
        log: LogFile::new(d.path().join(WATCHDOG_LOG_FILE), 10_000, Rotate::Rename),
    };
    Rig { _d: d, wd, launched, slept }
}

#[test]
fn a_dead_app_is_restarted_after_a_second_look() {
    let r = rig(&[], Some(PathBuf::from(APP)));
    heartbeat::write(&r.wd.app_dir, &beat(7, NOW - 100.0, None)).unwrap();
    assert_eq!(r.wd.once(), 0);
    assert_eq!(*r.slept.lock().unwrap(), [RECHECK], "erst nach einem zweiten Blick");
    assert_eq!(*r.launched.0.lock().unwrap(), [PathBuf::from(APP)]);
    let log = std::fs::read_to_string(r.wd.app_dir.join(WATCHDOG_LOG_FILE)).unwrap();
    assert!(log.contains("ABGESCHOSSEN") && log.contains("neu gestartet"), "{log}");
}

#[test]
fn a_closed_app_is_left_alone() {
    let r = rig(&[], Some(PathBuf::from(APP)));
    heartbeat::write(&r.wd.app_dir, &beat(7, NOW - 100.0, Some("exit"))).unwrap();
    assert_eq!(r.wd.once(), 0);
    assert!(r.launched.0.lock().unwrap().is_empty());
}

#[test]
fn a_running_or_hanging_app_gets_no_second_one() {
    let r = rig(&[(7, "agent-deck.exe")], Some(PathBuf::from(APP)));
    heartbeat::write(&r.wd.app_dir, &beat(7, NOW - 1.0, None)).unwrap();
    assert_eq!(r.wd.once(), 0);
    heartbeat::write(&r.wd.app_dir, &beat(7, NOW - 600.0, None)).unwrap();
    assert_eq!(r.wd.once(), 1);
    assert!(r.launched.0.lock().unwrap().is_empty());
}

#[test]
fn without_a_known_program_nothing_is_started() {
    let r = rig(&[], None);
    heartbeat::write(&r.wd.app_dir, &beat(7, NOW, None)).unwrap(); // exe gibt es nicht
    assert_eq!(r.wd.once(), 1);
    assert!(r.launched.0.lock().unwrap().is_empty());
}

#[test]
fn the_heartbeat_names_the_program_to_restart() {
    let r = rig(&[], None);
    let exe = r.wd.app_dir.join("agent-deck.exe");
    std::fs::write(&exe, b"").unwrap();
    let b = Heartbeat { exe: exe.to_string_lossy().into_owned(), ..beat(7, NOW, None) };
    assert_eq!(r.wd.target(Some(&b)), Some(exe));
}

#[test]
fn status_reports_without_starting() {
    let r = rig(&[], Some(PathBuf::from(APP)));
    heartbeat::mark_ended(&r.wd.app_dir, EndReason::Panic, NOW);
    let (code, text) = r.wd.status();
    assert_eq!(code, 1);
    assert!(text.contains("NICHT") && text.contains("panic"), "{text}");
    assert!(r.launched.0.lock().unwrap().is_empty());
}

#[test]
fn a_second_loop_watchdog_steps_back() {
    let me = std::env::current_exe().unwrap();
    let my_name: &'static str = Box::leak(me.file_name().unwrap().to_string_lossy().into_owned().into_boxed_str());
    let r = rig(&[(4242, my_name)], None);
    std::fs::write(r.wd.app_dir.join(WATCHDOG_LOCK_FILE), "4242").unwrap();
    assert!(!r.wd.loop_lock(), "ein anderer Wächter lebt");
    std::fs::write(r.wd.app_dir.join(WATCHDOG_LOCK_FILE), "9999").unwrap();
    assert!(r.wd.loop_lock(), "toter Eintrag wird übernommen");
    assert_eq!(std::fs::read_to_string(r.wd.app_dir.join(WATCHDOG_LOCK_FILE)).unwrap(), std::process::id().to_string());
}

#[test]
fn the_real_launcher_starts_nothing_in_tests() {
    assert!(!DetachedLauncher.start_detached(Path::new(APP)));
}
