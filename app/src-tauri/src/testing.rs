//! Attrappen für die Tests der App: ein Desktop, Prozesse, Konsole und Starter. Sie
//! merken sich, was die Logik tun wollte; nichts davon
//! erreicht das echte System. Zusätzlich sperrt jeder Test `forbid_effects()`.

use std::collections::BTreeMap;
use std::sync::Mutex;

use deck_win::arrange::ClassWindow;
use deck_win::console_input::Step;
use deck_win::effects::{ConsoleInput, Desktop, Processes};
use deck_win::focus::Corners;
use deck_win::monitor::Monitor;
use deck_win::taskbar::TaskbarGroup;
use deck_win::procs::ProcessInfo;
use deck_win::{Hwnd, Rect, Result, WindowInfo};

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
    minimized: Vec<Hwnd>,
    raised: Vec<Hwnd>,
    shadows: Vec<(Hwnd, bool)>,
    /// Fenster von oben nach unten.
    z: Vec<Hwnd>,
    monitors: Vec<Monitor>,
    idle_s: f64,
}

#[derive(Default)]
pub struct FakeDesktop(Mutex<Desk>);

impl FakeDesktop {
    pub fn add_wt(&self, hwnd: Hwnd, title: &str, rect: Rect, minimized: bool) {
        self.0.lock().unwrap().wt.push((ClassWindow { hwnd, title: title.into(), minimized }, rect));
    }
    /// Ein fremdes Fenster (kein Agent), das lebt: etwa das, aus dem man kam.
    pub fn add_title(&self, hwnd: Hwnd, title: &str) {
        self.0.lock().unwrap().titles.push((hwnd, title.into()));
    }
    pub fn set_foreground(&self, hwnd: Hwnd, title: &str, class: &str) {
        let mut d = self.0.lock().unwrap();
        d.fg = Some(WindowInfo { hwnd, title: title.into(), class: class.into(), pid: 1 });
        // Wer aktiv wird, liegt obenauf.
        d.z.retain(|h| *h != hwnd);
        d.z.insert(0, hwnd);
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
    /// Fenster, die ohne Tastatur nach oben gelegt wurden, in dieser Reihenfolge.
    pub fn raised(&self) -> Vec<Hwnd> {
        self.0.lock().unwrap().raised.clone()
    }
    /// Bei welchen Fenstern der Schatten ab- (`false`) oder angestellt wurde.
    pub fn shadows(&self) -> Vec<(Hwnd, bool)> {
        self.0.lock().unwrap().shadows.clone()
    }
    /// Ein Agentenfenster wurde verschoben.
    pub fn move_wt(&self, hwnd: Hwnd, rect: Rect) {
        self.0.lock().unwrap().wt.iter_mut().filter(|(w, _)| w.hwnd == hwnd).for_each(|(_, r)| *r = rect);
    }
    /// Fenster, die minimiert werden sollten, in dieser Reihenfolge.
    pub fn minimized(&self) -> Vec<Hwnd> {
        self.0.lock().unwrap().minimized.clone()
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

impl Desktop for FakeDesktop {
    fn foreground(&self) -> Option<WindowInfo> {
        self.0.lock().unwrap().fg.clone()
    }
    fn is_minimized(&self, hwnd: Hwnd) -> bool {
        self.0.lock().unwrap().wt.iter().any(|(w, _)| w.hwnd == hwnd && w.minimized)
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
    fn z_order(&self) -> Vec<Hwnd> {
        self.0.lock().unwrap().z.clone()
    }
    /// Die Z-Ordnung, dahinter die Agentenfenster, die noch nie vorn lagen. Ein fremdes
    /// Fenster füllt den ganzen Schirm, minimierte Agenten fehlen.
    fn solid_windows(&self) -> Vec<(Hwnd, Rect)> {
        let d = self.0.lock().unwrap();
        let mut order = d.z.clone();
        order.extend(d.wt.iter().map(|(w, _)| w.hwnd).filter(|h| !d.z.contains(h)));
        order
            .into_iter()
            .filter_map(|h| match d.wt.iter().find(|(w, _)| w.hwnd == h) {
                Some((w, r)) => (!w.minimized).then_some((h, *r)),
                None => Some((h, Rect::new(0, 0, 1920, 1080))),
            })
            .collect()
    }
    fn raise(&self, hwnd: Hwnd) -> Result<()> {
        let mut d = self.0.lock().unwrap();
        d.raised.push(hwnd);
        d.z.retain(|h| *h != hwnd);
        d.z.insert(0, hwnd);
        Ok(())
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
        let mut d = self.0.lock().unwrap();
        d.restored.push(hwnd);
        d.wt.iter_mut().filter(|(w, _)| w.hwnd == hwnd).for_each(|(w, _)| w.minimized = false);
        Ok(true)
    }
    fn minimize(&self, hwnd: Hwnd) -> Result<bool> {
        let mut d = self.0.lock().unwrap();
        d.minimized.push(hwnd);
        d.wt.iter_mut().filter(|(w, _)| w.hwnd == hwnd).for_each(|(w, _)| w.minimized = true);
        Ok(true)
    }
    fn set_corners(&self, hwnd: Hwnd, _: Corners) -> Result<bool> {
        self.0.lock().unwrap().corners.push(hwnd);
        Ok(true)
    }
    fn set_shadow(&self, hwnd: Hwnd, on: bool) -> Result<bool> {
        self.0.lock().unwrap().shadows.push((hwnd, on));
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
