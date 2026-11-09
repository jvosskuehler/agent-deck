//! Die echte Umgebung des Neustarts: Plan-Datei, VS Code finden und starten, Fenster
//! schließen, Prozesse zählen und beenden. Getrennt vom Ablauf (`restart.rs`), damit
//! dessen Tests sie gar nicht erst sehen.
//!
//! Was schließt oder beendet, geht über `deck_win::effects` (dort prozessweit sperrbar);
//! das Starten von `Code.exe` sperrt [`crate::guard`]. In Tests startet also nichts.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;

use deck_core::claude::summary_cache::transcript_path;
use deck_core::config::VSCODE_MARKER;
use deck_core::restart_plan::{self, ClientInfo, Plan, foreign_repos, open_args};
use deck_core::slot_state::SlotState;
use deck_win::effects::{Desktop, Processes};

use crate::gather::editor_windows;
use crate::guard;
use crate::restart::{PlanStore, RestartHost};
use crate::vscode_install::{code_exe, vscode_bases};

/// `restart_plan.json` im App-Ordner.
#[derive(Debug, Clone)]
pub struct PlanFile(pub PathBuf);

impl PlanFile {
    pub fn standard() -> PlanFile {
        PlanFile(deck_core::paths::app_dir().join(deck_core::paths::PLAN_FILE))
    }
}

impl PlanStore for PlanFile {
    fn load(&self) -> Option<Plan> {
        restart_plan::load(&self.0)
    }
    fn save(&self, plan: &Plan) {
        restart_plan::save(plan, &self.0);
    }
    fn clear(&self) {
        restart_plan::clear(&self.0);
    }
}

/// Gibt es zu (Session, cwd) noch ein Transcript? Nur dann lässt sich die Session
/// aufnehmen: eine ID in der Zustandsdatei überlebt das Aufräumen in
/// `~/.claude/projects`, und `claude --resume` auf eine verschwundene Unterhaltung
/// startet gar nicht erst (Regel 2).
pub fn has_transcript(projects: &Path, session_id: &str, cwd: &str) -> bool {
    transcript_path(projects, session_id, Some(cwd).filter(|c| !c.is_empty())).is_some()
}

/// Den Plan aus dem Ist-Zustand bauen, mit der Transcript-Prüfung verdrahtet.
pub fn build_plan(
    clients: &[ClientInfo],
    states: &HashMap<String, SlotState>,
    order: &BTreeMap<String, Vec<String>>,
    model: Option<&str>,
    slot_effort: &BTreeMap<String, String>,
    projects: &Path,
    now: f64,
) -> Plan {
    let check = |sid: &str, cwd: &str| has_transcript(projects, sid, cwd);
    restart_plan::build(clients, states, order, model, slot_effort, Some(&check), now)
}

/// Offene VS-Code-Fenster, die das Deck NICHT kennt, nach Repo benannt. Der Neustart
/// schließt sie mit; darum gehören sie VORHER in den Bestätigungstext. Leer heißt auch
/// „konnte nicht nachsehen“: die Warnung ist eine Zugabe, kein Tor.
pub fn foreign_vscode_windows(desktop: &dyn Desktop, known: &[String]) -> Vec<String> {
    foreign_repos(&desktop.list_titles(&[VSCODE_MARKER]), known)
}

/// Titel der offenen VS-Code-Hauptfenster (für die Warnung vor dem Neustart).
pub fn open_editor_titles(desktop: &dyn Desktop) -> Vec<String> {
    editor_windows(desktop).into_iter().map(|w| w.title).collect()
}

/// Die echte Umgebung.
pub struct SystemRestartHost {
    desktop: Arc<dyn Desktop>,
    procs: Arc<dyn Processes>,
    clock: Instant,
}

impl SystemRestartHost {
    pub fn new(desktop: Arc<dyn Desktop>, procs: Arc<dyn Processes>) -> SystemRestartHost {
        SystemRestartHost { desktop, procs, clock: Instant::now() }
    }
}

impl RestartHost for SystemRestartHost {
    fn find_code_exe(&self) -> Option<PathBuf> {
        code_exe(&vscode_bases())
    }

    fn count_exe(&self, name: &str) -> Option<usize> {
        self.procs.count_exe(name)
    }

    /// Ob Pfad oder Remote-Adresse, entscheidet `open_args` (ein Remote-Ordner geht nur
    /// über `--folder-uri`, sonst würde die Adresse als Dateiname gelesen).
    fn launch(&self, exe: &Path, folder: &str) -> bool {
        if guard::check("VS Code starten").is_err() {
            return false;
        }
        Command::new(exe).args(open_args(folder)).spawn().is_ok()
    }

    /// Dieselbe Auswahl wie beim Einsammeln: eine Regel, ein Ort. Gefragt ist nicht
    /// geschlossen (bei ungespeicherten Dateien fragt VS Code selbst nach).
    fn close_editor_windows(&self) -> usize {
        editor_windows(self.desktop.as_ref()).iter().filter(|w| self.desktop.post_close(w.hwnd).is_ok()).count()
    }

    fn kill_editors(&self, exe: &str) -> bool {
        self.procs.kill_by_name(exe).is_ok()
    }

    fn now(&self) -> f64 {
        self.clock.elapsed().as_secs_f64()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{FakeDesktop, FakeProcesses};
    use deck_win::Hwnd;
    use deck_win::arrange::TopWindow;

    #[test]
    fn the_real_host_starts_nothing_in_tests() {
        let host = SystemRestartHost::new(Arc::new(FakeDesktop::default()), Arc::new(FakeProcesses::default()));
        assert!(!host.launch(Path::new(r"C:\VSCode\Code.exe"), r"C:\Repos\alpha"));
    }

    #[test]
    fn closing_asks_exactly_the_vs_code_main_windows() {
        let code = r"C:\VS Code\Code.exe";
        let d = FakeDesktop {
            windows: vec![
                TopWindow { hwnd: Hwnd(1), title: "a - Visual Studio Code".into(), exe: Some(code.into()), style: 0x0001_0000 },
                TopWindow { hwnd: Hwnd(2), title: "Speichern?".into(), exe: Some(code.into()), style: 0 },
                TopWindow { hwnd: Hwnd(3), title: "x - Visual Studio Code - Firefox".into(), exe: Some("firefox.exe".into()), style: 0x0001_0000 },
            ],
            ..Default::default()
        };
        let d = Arc::new(d);
        let host = SystemRestartHost::new(d.clone(), Arc::new(FakeProcesses::default()));
        assert_eq!(host.close_editor_windows(), 1);
        assert_eq!(*d.closed.lock().unwrap(), [Hwnd(1)]);
    }

    #[test]
    fn foreign_windows_are_named_by_their_repo() {
        let d = FakeDesktop {
            titles: vec!["a.py - alpha - Visual Studio Code".into(), "b.py - fremd - Visual Studio Code".into()],
            ..Default::default()
        };
        assert_eq!(foreign_vscode_windows(&d, &["alpha".to_string()]), ["fremd"]);
    }

    #[test]
    fn a_session_without_transcript_starts_fresh_in_the_plan() {
        let d = tempfile::tempdir().unwrap();
        let cwd = r"C:\Repos\alpha";
        let enc = deck_core::claude::summarize::enc_cwd(cwd);
        let sid_ok = "11111111-1111-1111-1111-111111111111";
        let sid_gone = "22222222-2222-2222-2222-222222222222";
        std::fs::create_dir_all(d.path().join(&enc)).unwrap();
        std::fs::write(d.path().join(&enc).join(format!("{sid_ok}.jsonl")), "{}").unwrap();
        let client = ClientInfo {
            window: Some("A".into()),
            workspace: Some("alpha".into()),
            folder: Some(cwd.into()),
            slots: vec!["A1".into(), "A2".into()],
        };
        let mut states = HashMap::new();
        for (slot, sid) in [("A1", sid_ok), ("A2", sid_gone)] {
            let mut st = SlotState::new();
            st.insert("session_id".into(), sid.into());
            st.insert("cwd".into(), cwd.into());
            states.insert(slot.to_string(), st);
        }
        let plan = build_plan(&[client], &states, &BTreeMap::new(), None, &BTreeMap::new(), d.path(), 1.0);
        let slots = &plan.windows[0].slots;
        assert!(!slots[0].fresh && slots[0].args.contains(sid_ok));
        assert!(slots[1].fresh && !slots[1].args.contains("--resume"));
        assert_eq!(slots[1].cwd, cwd, "cwd reist mit, auch ohne Session");
    }

    #[test]
    fn the_plan_file_round_trips_and_clears() {
        let d = tempfile::tempdir().unwrap();
        let store = PlanFile(d.path().join("restart_plan.json"));
        assert!(store.load().is_none());
        let plan = Plan {
            ts: 1.0,
            windows: vec![deck_core::restart_plan::PlanWindow { window: "A".into(), folder: r"C:\r".into(), ..Default::default() }],
            skipped: vec![],
        };
        store.save(&plan);
        assert_eq!(store.load(), Some(plan));
        store.clear();
        assert!(store.load().is_none());
    }
}
