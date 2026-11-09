//! Attrappen für die Tests der App: ein Desktop, Prozesse, Konsole, Starter und eine
//! VS-Code-Seite ohne Socket. Sie merken sich, was die Logik tun wollte; nichts davon
//! erreicht das echte System. Zusätzlich sperrt jeder Test `forbid_effects()`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use deck_core::protocol::Command;
use deck_win::arrange::ClassWindow;
use deck_win::console_input::Step;
use deck_win::effects::{ConsoleInput, Desktop, Processes};
use deck_win::focus::Corners;
use deck_win::monitor::Monitor;
use deck_win::taskbar::TaskbarGroup;
use deck_win::procs::ProcessInfo;
use deck_win::{Hwnd, Rect, Result, WindowInfo};

use crate::hosts::VsCodeHost;
use crate::wt_setup::Launcher;

#[derive(Default)]
struct Desk {
    wt: Vec<(ClassWindow, Rect)>,
    titles: Vec<(Hwnd, String)>,
    fg: Option<WindowInfo>,
    placed: Vec<(Hwnd, Rect)>,
    focused: Vec<Hwnd>,
    corners: Vec<Hwnd>,
    borders: Vec<(Hwnd, u32)>,
    grouped: Vec<(Hwnd, String)>,
    light: Option<bool>,
    restored: Vec<Hwnd>,
    monitors: Vec<Monitor>,
    idle_s: f64,
}

#[derive(Default)]
pub struct FakeDesktop(Mutex<Desk>);

impl FakeDesktop {
    pub fn add_wt(&self, hwnd: Hwnd, title: &str, rect: Rect, minimized: bool) {
        self.0.lock().unwrap().wt.push((ClassWindow { hwnd, title: title.into(), minimized }, rect));
    }
    /// Ein Fenster mit Titel (für `find_window`/`list_titles`, etwa VS Code).
    pub fn add_title(&self, hwnd: Hwnd, title: &str) {
        self.0.lock().unwrap().titles.push((hwnd, title.into()));
    }
    pub fn clear_titles(&self) {
        self.0.lock().unwrap().titles.clear();
    }
    pub fn set_foreground(&self, hwnd: Hwnd, title: &str, class: &str) {
        self.0.lock().unwrap().fg = Some(WindowInfo { hwnd, title: title.into(), class: class.into(), pid: 1 });
    }
    /// Sekunden seit der letzten Eingabe (ohne Aufruf 0: gerade getippt).
    pub fn set_input_idle(&self, secs: f64) {
        self.0.lock().unwrap().idle_s = secs;
    }
    pub fn placed(&self) -> Vec<(Hwnd, Rect)> {
        self.0.lock().unwrap().placed.clone()
    }
    pub fn focused(&self) -> Vec<Hwnd> {
        self.0.lock().unwrap().focused.clone()
    }
    pub fn cornered(&self) -> Vec<Hwnd> {
        self.0.lock().unwrap().corners.clone()
    }
    pub fn bordered(&self) -> Vec<(Hwnd, u32)> {
        self.0.lock().unwrap().borders.clone()
    }
    /// Fenster, die in eine Taskleisten-Gruppe gestellt wurden, mit deren Kennung.
    pub fn grouped(&self) -> Vec<(Hwnd, String)> {
        self.0.lock().unwrap().grouped.clone()
    }
    /// Ein Agentenfenster ist zugegangen.
    pub fn remove_wt(&self, hwnd: Hwnd) {
        self.0.lock().unwrap().wt.retain(|(w, _)| w.hwnd != hwnd);
    }
    pub fn set_light(&self, light: Option<bool>) {
        self.0.lock().unwrap().light = light;
    }
    pub fn restored(&self) -> Vec<Hwnd> {
        self.0.lock().unwrap().restored.clone()
    }
    /// Einen weiteren Monitor dazustellen (ohne Aufruf gibt es nur den 1920er bei 0,0).
    pub fn add_monitor(&self, rect: Rect, work: Rect, scale: f64) {
        let mut d = self.0.lock().unwrap();
        if d.monitors.is_empty() {
            d.monitors = default_monitors();
        }
        let n = d.monitors.len() as isize + 1;
        d.monitors.push(Monitor { handle: n, rect, work, primary: false, device: format!("D{n}"), scale, refresh_hz: None });
    }
}

fn default_monitors() -> Vec<Monitor> {
    let r = Rect::new(0, 0, 1920, 1080);
    vec![Monitor { handle: 1, rect: r, work: Rect::new(0, 0, 1920, 1040), primary: true, device: "D1".into(), scale: 1.0, refresh_hz: None }]
}

fn has_all(title: &str, needles: &[&str]) -> bool {
    needles.iter().all(|n| title.contains(n))
}

impl Desktop for FakeDesktop {
    fn foreground(&self) -> Option<WindowInfo> {
        self.0.lock().unwrap().fg.clone()
    }
    fn is_minimized(&self, hwnd: Hwnd) -> bool {
        self.0.lock().unwrap().wt.iter().any(|(w, _)| w.hwnd == hwnd && w.minimized)
    }
    fn find_window(&self, needles: &[&str]) -> Option<Hwnd> {
        self.0.lock().unwrap().titles.iter().find(|(_, t)| has_all(t, needles)).map(|(h, _)| *h)
    }
    fn list_titles(&self, needles: &[&str]) -> Vec<String> {
        self.0.lock().unwrap().titles.iter().filter(|(_, t)| has_all(t, needles)).map(|(_, t)| t.clone()).collect()
    }
    fn class_windows(&self, _: &str) -> Vec<ClassWindow> {
        self.0.lock().unwrap().wt.iter().map(|(w, _)| w.clone()).collect()
    }
    fn window_rect(&self, hwnd: Hwnd) -> Option<Rect> {
        self.0.lock().unwrap().wt.iter().find(|(w, _)| w.hwnd == hwnd).map(|(_, r)| *r)
    }
    fn monitors(&self) -> Vec<Monitor> {
        let d = self.0.lock().unwrap();
        if d.monitors.is_empty() { default_monitors() } else { d.monitors.clone() }
    }
    fn work_area(&self, _: i32, _: i32) -> Option<Rect> {
        Some(Rect::new(0, 0, 1920, 1040))
    }
    fn scale_of(&self, _: Hwnd) -> f64 {
        1.0
    }
    fn is_alive(&self, hwnd: Hwnd) -> bool {
        let d = self.0.lock().unwrap();
        d.wt.iter().any(|(w, _)| w.hwnd == hwnd) || d.titles.iter().any(|(h, _)| *h == hwnd)
    }
    fn input_idle_s(&self) -> f64 {
        self.0.lock().unwrap().idle_s
    }
    fn focus(&self, hwnd: Hwnd) -> Result<bool> {
        self.0.lock().unwrap().focused.push(hwnd);
        Ok(true)
    }
    fn place(&self, hwnd: Hwnd, visible: Rect) -> Result<()> {
        self.0.lock().unwrap().placed.push((hwnd, visible));
        Ok(())
    }
    fn restore(&self, hwnd: Hwnd) -> Result<bool> {
        self.0.lock().unwrap().restored.push(hwnd);
        Ok(true)
    }
    fn set_corners(&self, hwnd: Hwnd, _: Corners) -> Result<bool> {
        self.0.lock().unwrap().corners.push(hwnd);
        Ok(true)
    }
    fn set_border(&self, hwnd: Hwnd, rgb: u32) -> Result<bool> {
        self.0.lock().unwrap().borders.push((hwnd, rgb));
        Ok(true)
    }
    fn light_theme(&self) -> Option<bool> {
        self.0.lock().unwrap().light
    }
    fn join_taskbar_group(&self, hwnd: Hwnd, group: &TaskbarGroup) -> Result<()> {
        self.0.lock().unwrap().grouped.push((hwnd, group.app_id.clone()));
        Ok(())
    }
}

/// Prozesse: `pid → Startzeit`; merkt sich, was beendet werden sollte.
#[derive(Default)]
pub struct FakeProcesses {
    pub alive: Mutex<BTreeMap<u32, u64>>,
    pub killed: Mutex<Vec<u32>>,
}

impl Processes for FakeProcesses {
    fn list(&self) -> Option<Vec<ProcessInfo>> {
        None
    }
    fn process_start(&self, pid: u32) -> Option<u64> {
        self.alive.lock().unwrap().get(&pid).copied()
    }
    fn kill_tree(&self, pid: u32) -> Result<()> {
        self.killed.lock().unwrap().push(pid);
        Ok(())
    }
}

#[derive(Default)]
pub struct FakeConsole(pub Mutex<Vec<(u32, Vec<Step>)>>);

impl ConsoleInput for FakeConsole {
    fn send(&self, pid: u32, steps: &[Step]) -> Result<usize> {
        self.0.lock().unwrap().push((pid, steps.to_vec()));
        Ok(steps.len())
    }
}

#[derive(Default)]
pub struct FakeLauncher(pub Mutex<Vec<Vec<String>>>);

impl Launcher for FakeLauncher {
    fn launch(&self, argv: &[String]) -> bool {
        self.0.lock().unwrap().push(argv.to_vec());
        true
    }
}

/// Ein VS-Code-Fenster aus Sicht des Brokers.
#[derive(Clone, Default)]
pub struct FakeWindow {
    pub workspace: String,
    pub window: Option<String>,
    pub slots: Vec<String>,
    pub remote: Option<String>,
}

#[derive(Default)]
pub struct FakeVsCode {
    pub windows: Mutex<Vec<FakeWindow>>,
    pub sent: Mutex<Vec<(String, Command)>>,
    pub seen: Mutex<BTreeSet<String>>,
}

impl FakeVsCode {
    pub fn open(&self, workspace: &str, window: Option<&str>, slots: &[&str]) {
        self.windows.lock().unwrap().push(FakeWindow {
            workspace: workspace.into(),
            window: window.map(String::from),
            slots: slots.iter().map(|s| s.to_string()).collect(),
            remote: None,
        });
    }
    pub fn sent(&self) -> Vec<(String, Command)> {
        self.sent.lock().unwrap().clone()
    }
    fn find<T>(&self, win: &str, f: impl FnOnce(&mut FakeWindow) -> T) -> Option<T> {
        self.windows.lock().unwrap().iter_mut().find(|w| w.window.as_deref() == Some(win)).map(f)
    }
}

impl VsCodeHost for FakeVsCode {
    fn connected(&self, win: &str) -> bool {
        self.find(win, |_| ()).is_some()
    }
    fn remote(&self, win: &str) -> Option<String> {
        self.find(win, |w| w.remote.clone()).flatten()
    }
    fn terminals(&self, win: &str) -> Vec<String> {
        self.find(win, |w| w.slots.clone()).unwrap_or_default()
    }
    fn send(&self, win: &str, cmd: &Command) -> bool {
        let ok = self.connected(win);
        if ok {
            self.sent.lock().unwrap().push((win.to_string(), cmd.clone()));
        }
        ok
    }
    fn assign(&self, workspace: &str, win: &str) -> bool {
        let mut ws = self.windows.lock().unwrap();
        match ws.iter_mut().find(|w| w.workspace.eq_ignore_ascii_case(workspace)) {
            Some(w) => {
                w.window = Some(win.into());
                true
            }
            None => false,
        }
    }
    fn forget(&self, win: &str) -> bool {
        self.find(win, |w| w.window = None).is_some()
    }
    fn drain_seen(&self) -> BTreeSet<String> {
        std::mem::take(&mut *self.seen.lock().unwrap())
    }
    fn workspaces(&self) -> Vec<String> {
        self.windows.lock().unwrap().iter().map(|w| w.workspace.clone()).collect()
    }
    fn workspace_slots(&self, workspace: &str) -> Vec<String> {
        let ws = self.windows.lock().unwrap();
        ws.iter().find(|w| w.workspace.eq_ignore_ascii_case(workspace)).map(|w| w.slots.clone()).unwrap_or_default()
    }
}
