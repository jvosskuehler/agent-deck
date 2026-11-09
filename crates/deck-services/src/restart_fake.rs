//! Attrappen für die Tests des Neustarts (`restart_tests.rs`): Extension, Broker,
//! Prozessliste, Uhr und Plan-Datei, alles gefälscht. Beide Eskalationsstufen (WM_CLOSE
//! an alle Fenster, taskkill) schreiben nur mit.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use deck_core::restart_plan::{Plan, PlanSlot, PlanWindow};

use super::*;

pub(super) const REPO_A: &str = r"C:\Repos\alpha";
pub(super) const REPO_B: &str = r"C:\Repos\beta";

pub(super) fn slot(name: &str, args: &str, cwd: &str) -> PlanSlot {
    PlanSlot { slot: name.into(), cwd: cwd.into(), args: args.into(), fresh: args.is_empty() }
}

pub(super) fn plan() -> Plan {
    Plan {
        ts: 0.0,
        windows: vec![
            PlanWindow {
                window: "A".into(),
                workspace: "alpha".into(),
                folder: REPO_A.into(),
                slots: vec![slot("A1", "--resume \"x\"", REPO_A), slot("A2", "", REPO_A)],
            },
            PlanWindow { window: "B".into(), workspace: "beta".into(), folder: REPO_B.into(), slots: vec![slot("B1", "--resume \"y\"", REPO_B)] },
        ],
        skipped: vec![],
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Ev {
    Save(String),
    Kill(String),
    Shut(String),
    Create(String, String, String, String),
    Appear(String, String),
    Launch(String),
    WmClose,
    Taskkill(String),
    Say(Option<bool>),
}

/// Extension, Broker, Prozessliste, Uhr und Plan-Datei, alles gefälscht. `alive` ist die
/// Folge der Antworten von count_exe (die letzte wiederholt sich; `None` = unlesbar).
pub(super) struct Env {
    pub(super) plan: Plan,
    pub(super) alive: Mutex<Vec<Option<usize>>>,
    pub(super) connect_after: f64,
    pub(super) appear: bool,
    pub(super) create_ok: bool,
    pub(super) exe: Option<PathBuf>,
    pub(super) saved: Mutex<Option<Plan>>,
    pub(super) cleared: AtomicBool,
    pub(super) explode: AtomicBool,
    pub(super) t: Mutex<f64>,
    pub(super) t_start: Mutex<f64>,
    pub(super) log: Mutex<Vec<Ev>>,
    pub(super) terms: Mutex<BTreeMap<String, Vec<String>>>,
    pub(super) queue: Mutex<Vec<(String, String)>>,
}

impl Default for Env {
    fn default() -> Self {
        Env {
            plan: plan(),
            alive: Mutex::new(vec![Some(2), Some(1), Some(0)]),
            connect_after: 0.0,
            appear: true,
            create_ok: true,
            exe: Some(PathBuf::from(r"C:\VSCode\Code.exe")),
            saved: Mutex::new(None),
            cleared: AtomicBool::new(false),
            explode: AtomicBool::new(false),
            t: Mutex::new(1000.0),
            t_start: Mutex::new(1000.0),
            log: Mutex::new(Vec::new()),
            terms: Mutex::new(BTreeMap::new()),
            queue: Mutex::new(Vec::new()),
        }
    }
}

impl Env {
    pub(super) fn push(&self, e: Ev) {
        self.log.lock().unwrap().push(e);
    }
    pub(super) fn events(&self) -> Vec<Ev> {
        self.log.lock().unwrap().clone()
    }
    pub(super) fn launched(&self) -> Vec<String> {
        self.events().into_iter().filter_map(|e| if let Ev::Launch(f) = e { Some(f) } else { None }).collect()
    }
    pub(super) fn count(&self, pred: fn(&Ev) -> bool) -> usize {
        self.events().iter().filter(|e| pred(e)).count()
    }
}

pub(super) struct F(pub(super) Arc<Env>);

impl EditorBridge for F {
    fn save_all(&self, w: &str) -> bool {
        assert!(!self.0.explode.load(Ordering::SeqCst), "Broker kaputt");
        self.0.push(Ev::Save(w.into()));
        true
    }
    fn close_agent(&self, s: &str) -> bool {
        assert!(!self.0.explode.load(Ordering::SeqCst), "Broker kaputt");
        self.0.push(Ev::Kill(s.into()));
        true
    }
    fn close_window(&self, w: &str) -> bool {
        self.0.push(Ev::Shut(w.into()));
        true
    }
    fn create_agent(&self, w: &str, s: &str, args: &str, cwd: &str) -> bool {
        self.0.push(Ev::Create(w.into(), s.into(), args.into(), cwd.into()));
        if !self.0.create_ok {
            return false;
        }
        if self.0.appear {
            self.0.queue.lock().unwrap().push((w.into(), s.into())); // frühestens beim nächsten Blick
        }
        true
    }
    fn connected(&self, _: &str) -> bool {
        *self.0.t.lock().unwrap() - *self.0.t_start.lock().unwrap() >= self.0.connect_after
    }
    fn terminals(&self, w: &str) -> Vec<String> {
        self.0.terms.lock().unwrap().get(w).cloned().unwrap_or_default()
    }
}

impl RestartHost for F {
    fn find_code_exe(&self) -> Option<PathBuf> {
        self.0.exe.clone()
    }
    fn count_exe(&self, _: &str) -> Option<usize> {
        let mut a = self.0.alive.lock().unwrap();
        if a.len() == 1 { a[0] } else { a.remove(0) }
    }
    fn launch(&self, _: &Path, folder: &str) -> bool {
        self.0.push(Ev::Launch(folder.into()));
        true
    }
    fn close_editor_windows(&self) -> usize {
        self.0.push(Ev::WmClose);
        0
    }
    fn kill_editors(&self, exe: &str) -> bool {
        self.0.push(Ev::Taskkill(exe.into()));
        true
    }
    fn now(&self) -> f64 {
        *self.0.t.lock().unwrap()
    }
}

impl PlanStore for F {
    fn load(&self) -> Option<Plan> {
        self.0.saved.lock().unwrap().clone()
    }
    fn save(&self, plan: &Plan) {
        *self.0.saved.lock().unwrap() = Some(plan.clone());
    }
    fn clear(&self) {
        self.0.cleared.store(true, Ordering::SeqCst);
        *self.0.saved.lock().unwrap() = None;
    }
}

pub(super) fn restarter(env: &Arc<Env>) -> Restarter {
    Restarter::new(Box::new(F(env.clone())), Box::new(F(env.clone())), Box::new(F(env.clone())), Lang::German)
}

/// Fahren, bis Ruhe ist. Zwischen zwei Blicken rückt die Uhr, und die zuletzt
/// angeforderten Chats tauchen auf (wie die Extension ihre Terminalliste meldet).
pub(super) fn run(env: &Arc<Env>, ticks: usize) -> Restarter {
    let mut r = restarter(env);
    let e = env.clone();
    let snapshot_env = env.clone();
    r.start(&move || snapshot_env.plan.clone(), Some(Box::new(move |_, ok| e.push(Ev::Say(ok)))));
    for _ in 0..ticks {
        if !r.busy() {
            break;
        }
        r.step();
        *env.t.lock().unwrap() += 0.5;
        let q: Vec<(String, String)> = std::mem::take(&mut *env.queue.lock().unwrap());
        for (w, s) in q {
            env.terms.lock().unwrap().entry(w.clone()).or_default().push(s.clone());
            env.push(Ev::Appear(w, s));
        }
    }
    r
}

pub(super) fn with(f: impl FnOnce(&mut Env)) -> Arc<Env> {
    let mut e = Env::default();
    f(&mut e);
    Arc::new(e)
}
