//! Die Naht für Tests: alles, was die App am System liest oder verändert, hinter drei
//! Traits. Die App hält `Arc<dyn Desktop>` usw.; in Tests steckt dort eine Attrappe.
//!
//! Lesen und Verändern stehen bewusst im selben Trait: eine Attrappe simuliert dann
//! einen ganzen Desktop (welches Fenster vorn ist, welche es gibt) und protokolliert,
//! was die Logik damit tun wollte. Zusätzlich sperrt [`crate::forbid_effects`] die
//! echten Eingriffe prozessweit, falls irgendwo doch die echte Fassung landet.

use std::path::PathBuf;

use crate::arrange::{self, ClassWindow};
use crate::console_input::{self, Step};
use crate::dpi;
use crate::focus::{self, Corners};
use crate::idle;
use crate::monitor::{self, Monitor};
use crate::procs::{self, ProcessInfo};
use crate::taskbar::{self, TaskbarGroup};
use crate::types::{Error, Hwnd, Rect, Result};
use crate::window::{self, WindowInfo};

/// Fenster und Monitore.
pub trait Desktop: Send + Sync {
    // ── lesen ──
    fn foreground(&self) -> Option<WindowInfo>;
    fn is_minimized(&self, hwnd: Hwnd) -> bool;
    fn find_window(&self, needles: &[&str]) -> Option<Hwnd>;
    fn list_titles(&self, needles: &[&str]) -> Vec<String>;
    fn class_windows(&self, class: &str) -> Vec<ClassWindow>;
    fn window_rect(&self, hwnd: Hwnd) -> Option<Rect>;
    fn monitors(&self) -> Vec<Monitor>;
    fn work_area(&self, x: i32, y: i32) -> Option<Rect>;
    fn scale_of(&self, hwnd: Hwnd) -> f64;
    /// Lebt das Fenster noch und ist es sichtbar?
    fn is_alive(&self, hwnd: Hwnd) -> bool;
    /// Sekunden seit der letzten Taste oder Mausbewegung. Ohne eigene Antwort 0, also
    /// „gerade getippt“: wer es nicht weiß, stört lieber nicht.
    fn input_idle_s(&self) -> f64 {
        0.0
    }
    // ── verändern ──
    fn focus(&self, hwnd: Hwnd) -> Result<bool>;
    fn place(&self, hwnd: Hwnd, visible: Rect) -> Result<()>;
    /// Minimiertes Fenster wiederherstellen, ohne es zu aktivieren.
    fn restore(&self, hwnd: Hwnd) -> Result<bool>;
    fn set_corners(&self, hwnd: Hwnd, corners: Corners) -> Result<bool>;
    /// Fensterrand deckend in `rgb` (`0xRRGGBB`); `Ok(false)` = geht hier nicht.
    fn set_border(&self, _hwnd: Hwnd, _rgb: u32) -> Result<bool> {
        Ok(false)
    }
    /// Windows-Apps im hellen Modus? `None` = unbekannt.
    fn light_theme(&self) -> Option<bool> {
        None
    }
    /// Eigener Taskleisten-Knopf: das Fenster in `group` stellen.
    fn join_taskbar_group(&self, _hwnd: Hwnd, _group: &TaskbarGroup) -> Result<()> {
        Ok(())
    }
}

/// Prozesse.
pub trait Processes: Send + Sync {
    /// `None` = konnte nicht nachsehen (nie als „läuft nicht“ deuten).
    fn list(&self) -> Option<Vec<ProcessInfo>>;
    fn process_start(&self, pid: u32) -> Option<u64>;
    fn kill_tree(&self, pid: u32) -> Result<()>;
}

/// Tasten in fremde Konsolen.
pub trait ConsoleInput: Send + Sync {
    /// Schritte in die Konsole von `pid`; liefert die Zahl der Eingabe-Ereignisse.
    fn send(&self, pid: u32, steps: &[Step]) -> Result<usize>;
}

/// Die echte Fassung von [`Desktop`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Win32Desktop;

impl Desktop for Win32Desktop {
    fn foreground(&self) -> Option<WindowInfo> {
        focus::foreground()
    }
    fn is_minimized(&self, hwnd: Hwnd) -> bool {
        window::is_minimized(hwnd)
    }
    fn find_window(&self, needles: &[&str]) -> Option<Hwnd> {
        focus::find_window(needles)
    }
    fn list_titles(&self, needles: &[&str]) -> Vec<String> {
        focus::list_titles(needles)
    }
    fn class_windows(&self, class: &str) -> Vec<ClassWindow> {
        arrange::class_windows(class)
    }
    fn window_rect(&self, hwnd: Hwnd) -> Option<Rect> {
        arrange::window_rect(hwnd)
    }
    fn monitors(&self) -> Vec<Monitor> {
        monitor::monitors()
    }
    fn work_area(&self, x: i32, y: i32) -> Option<Rect> {
        monitor::work_area(x, y)
    }
    fn scale_of(&self, hwnd: Hwnd) -> f64 {
        dpi::factor_for_window(hwnd)
    }
    fn is_alive(&self, hwnd: Hwnd) -> bool {
        window::is_window(hwnd) && window::is_visible(hwnd)
    }
    fn input_idle_s(&self) -> f64 {
        idle::input_idle_s()
    }
    fn focus(&self, hwnd: Hwnd) -> Result<bool> {
        focus::focus_window(hwnd)
    }
    fn place(&self, hwnd: Hwnd, visible: Rect) -> Result<()> {
        arrange::place_window(hwnd, visible)
    }
    fn restore(&self, hwnd: Hwnd) -> Result<bool> {
        arrange::restore_window(hwnd)
    }
    fn set_corners(&self, hwnd: Hwnd, corners: Corners) -> Result<bool> {
        focus::set_corners(hwnd, corners)
    }
    fn set_border(&self, hwnd: Hwnd, rgb: u32) -> Result<bool> {
        focus::set_border_color(hwnd, rgb)
    }
    fn light_theme(&self) -> Option<bool> {
        focus::apps_use_light_theme()
    }
    fn join_taskbar_group(&self, hwnd: Hwnd, group: &TaskbarGroup) -> Result<()> {
        taskbar::join_group(hwnd, group)
    }
}

/// Die echte Fassung von [`Processes`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Win32Processes;

impl Processes for Win32Processes {
    fn list(&self) -> Option<Vec<ProcessInfo>> {
        procs::processes()
    }
    fn process_start(&self, pid: u32) -> Option<u64> {
        procs::process_start(pid)
    }
    fn kill_tree(&self, pid: u32) -> Result<()> {
        procs::kill_tree(pid)
    }
}

/// Die echte Fassung von [`ConsoleInput`]: über den Wegwerfprozess `deck-console`.
#[derive(Clone, Debug)]
pub struct HelperConsole {
    helper: Option<PathBuf>,
}

impl HelperConsole {
    /// Hilfs-Binary an einem festen Ort (etwa aus den Tauri-Ressourcen).
    pub fn at(path: PathBuf) -> HelperConsole {
        HelperConsole { helper: Some(path) }
    }

    /// Hilfs-Binary neben der laufenden Exe suchen ([`console_input::helper_path`]).
    pub fn beside_exe() -> HelperConsole {
        HelperConsole {
            helper: console_input::helper_path(),
        }
    }
}

impl ConsoleInput for HelperConsole {
    fn send(&self, pid: u32, steps: &[Step]) -> Result<usize> {
        let path = self.helper.as_deref().ok_or_else(|| {
            Error::Helper(format!("{} nicht gefunden", console_input::HELPER_EXE))
        })?;
        console_input::send_via_helper(path, pid, steps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// So sieht eine Attrappe in der App aus: sie merkt sich, was getan werden sollte.
    #[derive(Default)]
    struct Recorder(Mutex<Vec<String>>);

    impl Processes for Recorder {
        fn list(&self) -> Option<Vec<ProcessInfo>> {
            None
        }
        fn process_start(&self, _: u32) -> Option<u64> {
            None
        }
        fn kill_tree(&self, pid: u32) -> Result<()> {
            self.0.lock().unwrap().push(format!("kill_tree {pid}"));
            Ok(())
        }
    }

    #[test]
    fn logic_behind_the_trait_runs_against_a_fake_without_touching_the_system() {
        let fake = Arc::new(Recorder::default());
        let procs: Arc<dyn Processes> = fake.clone();
        // „Weiß nicht“ darf die Logik nicht als „läuft nicht“ lesen.
        if procs.list().is_none() {
            procs.kill_tree(42).unwrap();
        }
        assert_eq!(*fake.0.lock().unwrap(), vec!["kill_tree 42"]);
    }

    #[test]
    fn real_implementations_refuse_effects_in_tests() {
        assert!(matches!(
            Win32Processes.kill_tree(1234),
            Err(Error::Forbidden(_))
        ));
        assert!(matches!(
            Win32Desktop.place(Hwnd(1), Rect::new(0, 0, 10, 10)),
            Err(Error::Forbidden(_))
        ));
        assert!(matches!(
            HelperConsole::at("x.exe".into()).send(1, &[]),
            Err(Error::Forbidden(_))
        ));
    }

    #[test]
    fn real_desktop_answers_read_questions() {
        let d: Box<dyn Desktop> = Box::new(Win32Desktop);
        assert!(!d.monitors().is_empty());
        assert!(d.scale_of(Hwnd(0)) >= 0.5);
    }
}
