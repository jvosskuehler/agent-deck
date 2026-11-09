//! Attrappen für die Traits aus `deck_win::effects`, geteilt von den Tests dieses
//! Crates. Sie schreiben mit, was die Logik tun WOLLTE, und tun selbst nichts.

use std::sync::Mutex;

use deck_win::arrange::{ClassWindow, Placement, ShowState, TopWindow};
use deck_win::effects::{Desktop, Processes};
use deck_win::focus::Corners;
use deck_win::monitor::Monitor;
use deck_win::procs::ProcessInfo;
use deck_win::window::WindowInfo;
use deck_win::{Error, Hwnd, Rect, Result};

/// Ein Desktop aus festen Fenstern; `move_maximized` und `post_close` protokollieren.
#[derive(Default)]
pub struct FakeDesktop {
    pub windows: Vec<TopWindow>,
    /// Arbeitsfläche der Hauptanzeige (`work_area(0, 0)`).
    pub area: Option<Rect>,
    /// Normalrechteck jedes Fensters.
    pub normal: Rect,
    /// Fenster, deren Umzug scheitert.
    pub refuse: Vec<Hwnd>,
    pub moved: Mutex<Vec<(Hwnd, Rect)>>,
    pub closed: Mutex<Vec<Hwnd>>,
    pub titles: Vec<String>,
}

impl Desktop for FakeDesktop {
    fn foreground(&self) -> Option<WindowInfo> {
        None
    }
    fn describe(&self, _: Hwnd) -> Option<WindowInfo> {
        None
    }
    fn is_minimized(&self, _: Hwnd) -> bool {
        false
    }
    fn find_window(&self, _: &[&str]) -> Option<Hwnd> {
        None
    }
    fn list_titles(&self, needles: &[&str]) -> Vec<String> {
        self.titles.iter().filter(|t| needles.iter().all(|n| t.contains(n))).cloned().collect()
    }
    fn class_windows(&self, _: &str) -> Vec<ClassWindow> {
        Vec::new()
    }
    fn visible_windows(&self) -> Vec<TopWindow> {
        self.windows.clone()
    }
    fn windows_of_pid(&self, _: u32) -> Vec<Hwnd> {
        Vec::new()
    }
    fn window_rect(&self, _: Hwnd) -> Option<Rect> {
        None
    }
    fn placement(&self, _: Hwnd) -> Option<Placement> {
        Some(Placement { state: ShowState::Maximized, rect: self.normal, normal: self.normal })
    }
    fn monitors(&self) -> Vec<Monitor> {
        Vec::new()
    }
    fn monitor_of_window(&self, _: Hwnd) -> Option<Monitor> {
        None
    }
    fn work_area(&self, x: i32, y: i32) -> Option<Rect> {
        self.area.filter(|_| x == 0 && y == 0)
    }
    fn scale_of(&self, _: Hwnd) -> f64 {
        1.0
    }
    fn is_alive(&self, _: Hwnd) -> bool {
        false
    }
    fn focus(&self, _: Hwnd) -> Result<bool> {
        Ok(false)
    }
    fn place(&self, _: Hwnd, _: Rect) -> Result<()> {
        Ok(())
    }
    fn restore(&self, _: Hwnd) -> Result<bool> {
        Ok(false)
    }
    fn move_maximized(&self, hwnd: Hwnd, normal: Rect) -> Result<()> {
        if self.refuse.contains(&hwnd) {
            return Err(Error::Rejected("verschwunden".into()));
        }
        self.moved.lock().unwrap().push((hwnd, normal));
        Ok(())
    }
    fn post_close(&self, hwnd: Hwnd) -> Result<()> {
        self.closed.lock().unwrap().push(hwnd);
        Ok(())
    }
    fn set_corners(&self, _: Hwnd, _: Corners) -> Result<bool> {
        Ok(false)
    }
}

/// Eine Prozessliste aus festen Einträgen; `kill_*` protokollieren nur.
#[derive(Default)]
pub struct FakeProcesses {
    /// `None` = Liste nicht lesbar.
    pub list: Option<Vec<ProcessInfo>>,
    pub killed: Mutex<Vec<String>>,
}

impl Processes for FakeProcesses {
    fn list(&self) -> Option<Vec<ProcessInfo>> {
        self.list.clone()
    }
    fn count_exe(&self, name: &str) -> Option<usize> {
        Some(self.list.as_ref()?.iter().filter(|p| p.exe.eq_ignore_ascii_case(name)).count())
    }
    fn process_start(&self, pid: u32) -> Option<u64> {
        self.list.as_ref()?.iter().find(|p| p.pid == pid).and_then(|p| p.start)
    }
    fn kill_tree(&self, pid: u32) -> Result<()> {
        self.killed.lock().unwrap().push(format!("tree {pid}"));
        Ok(())
    }
    fn kill_by_name(&self, exe: &str) -> Result<()> {
        self.killed.lock().unwrap().push(format!("name {exe}"));
        Ok(())
    }
}
