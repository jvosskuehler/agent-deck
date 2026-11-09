//! Der Dienst gegen ein simuliertes System: Datei, Zeilen, Beenden samt Nachweis.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use deck_core::i18n::Lang;
use deck_core::orphans::model::unix_to_filetime;
use deck_core::orphans::{Key, ProcDetails, ProcRow, row_id};

use super::{ACCESS_DENIED, OrphanWatch, ProcSource};

const T0: f64 = 1_800_000_000.0;

#[derive(Default)]
struct FakeSys {
    procs: Mutex<Vec<ProcRow>>,
    ports: Mutex<Vec<(u32, u16)>>,
    killed: Mutex<Vec<Key>>,
    immortal: HashSet<u32>,
    denied: HashSet<u32>,
    /// Herkunftsmarke je PID (aus der Umgebung) und welche PIDs danach gefragt wurden.
    markers: Mutex<HashMap<u32, (u32, i64)>>,
    marker_reads: Mutex<Vec<u32>>,
}

impl FakeSys {
    fn add(&self, pid: u32, ppid: u32, exe: &str, started: f64) {
        self.procs.lock().unwrap().push(ProcRow { pid, ppid, exe: exe.into(), start: Some(unix_to_filetime(started)) });
    }
    fn remove(&self, pid: u32) {
        self.procs.lock().unwrap().retain(|p| p.pid != pid);
        self.ports.lock().unwrap().retain(|(p, _)| *p != pid);
    }
}

impl ProcSource for FakeSys {
    fn snapshot(&self) -> Option<Vec<ProcRow>> {
        Some(self.procs.lock().unwrap().clone())
    }
    fn details(&self, pid: u32, start: u64) -> Option<ProcDetails> {
        self.running(pid, start).then(|| ProcDetails { cmdline: Some(format!("node server-{pid}.js")), image: None, session: Some(1) })
    }
    fn listening(&self) -> Option<Vec<(u32, u16)>> {
        Some(self.ports.lock().unwrap().clone())
    }
    fn terminate(&self, pid: u32, start: u64) -> Result<bool, String> {
        if !self.running(pid, start) {
            return Ok(false);
        }
        if self.denied.contains(&pid) {
            return Err(ACCESS_DENIED.into());
        }
        self.killed.lock().unwrap().push((pid, start));
        if !self.immortal.contains(&pid) {
            self.remove(pid);
        }
        Ok(true)
    }
    fn running(&self, pid: u32, start: u64) -> bool {
        self.procs.lock().unwrap().iter().any(|p| p.pid == pid && p.start == Some(start))
    }
    fn wait_any_exit(&self, _: &[Key], _: Duration) -> bool {
        false
    }
    fn marker(&self, pid: u32, start: u64) -> Option<(u32, i64)> {
        self.marker_reads.lock().unwrap().push(pid);
        self.running(pid, start).then(|| self.markers.lock().unwrap().get(&pid).copied()).flatten()
    }
}

/// Agent A2 (Shell 100) mit node 102 (Port 3000) und dessen Kind 103; dann Fenster zu.
fn setup(sys: FakeSys) -> (tempfile::TempDir, Arc<FakeSys>, OrphanWatch) {
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path().join("state");
    std::fs::create_dir_all(&state).unwrap();
    let pidfile = format!(r#"{{"pid":100,"created":{},"slot":"A2"}}"#, unix_to_filetime(T0));
    std::fs::write(state.join("A2.wtpid"), pidfile).unwrap();
    let sys = Arc::new(sys);
    sys.add(100, 1, "pwsh.exe", T0);
    sys.add(101, 100, "claude.exe", T0 + 1.0);
    sys.add(102, 101, "node.exe", T0 + 2.0);
    sys.add(103, 102, "node.exe", T0 + 3.0);
    sys.ports.lock().unwrap().push((103, 3000));
    let w = OrphanWatch::new(sys.clone(), tmp.path(), &state);
    w.scan(T0 + 10.0);
    sys.remove(100);
    sys.remove(101);
    w.scan(T0 + 12.0);
    (tmp, sys, w)
}

fn key(pid: u32, started: f64) -> Key {
    (pid, unix_to_filetime(started))
}

#[test]
fn the_closed_agents_processes_become_one_row_and_land_in_the_file() {
    let (tmp, _sys, w) = setup(FakeSys::default());
    let rows = w.rows(T0 + 72.0, Lang::German);
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].pid, rows[0].port_text.as_str(), rows[0].origin.as_str()), (102, ":3000", "A2 · zu seit 1 min"));
    // Ein neuer Dienst (Neustart des Decks) kennt sie aus der Datei.
    let again = OrphanWatch::new(Arc::new(FakeSys::default()), tmp.path(), &tmp.path().join("state"));
    assert_eq!(again.tracker().orphans().len(), 2);
}

#[test]
fn killing_a_row_terminates_its_tree_top_down_with_exact_start_times() {
    let (_tmp, sys, w) = setup(FakeSys::default());
    w.kill(&row_id(key(102, T0 + 2.0)), T0 + 20.0, Lang::German).unwrap();
    assert_eq!(*sys.killed.lock().unwrap(), vec![key(102, T0 + 2.0), key(103, T0 + 3.0)]);
    assert!(w.rows(T0 + 21.0, Lang::German).is_empty());
}

#[test]
fn a_row_that_is_already_gone_counts_as_killed_and_an_unknown_one_kills_nothing() {
    let (_tmp, sys, w) = setup(FakeSys::default());
    sys.remove(102);
    sys.remove(103);
    assert_eq!(w.kill(&row_id(key(102, T0 + 2.0)), T0 + 20.0, Lang::German), Ok(()));
    assert!(w.kill("kaputt", T0 + 20.0, Lang::German).is_err());
    assert!(sys.killed.lock().unwrap().is_empty());
}

#[test]
fn a_process_whose_window_is_open_or_that_is_protected_is_never_killed() {
    let sys = FakeSys::default();
    let (_tmp, sys, w) = setup(sys);
    // claude 101 lief weiter (hier wieder lebend eingetragen): geschützt.
    sys.add(104, 1, "pwsh.exe", T0 + 30.0);
    assert!(w.kill(&row_id(key(104, T0 + 30.0)), T0 + 31.0, Lang::German).is_err());
    assert!(w.kill(&row_id(key(103, T0 + 3.0)), T0 + 31.0, Lang::German).is_err(), "nur die Wurzel ist eine Zeile");
    assert!(sys.killed.lock().unwrap().is_empty());
}

#[test]
fn a_process_that_survives_the_kill_shows_an_error_on_its_row() {
    let (_tmp, _sys, w) = setup(FakeSys { immortal: HashSet::from([103]), ..FakeSys::default() });
    let err = w.kill(&row_id(key(102, T0 + 2.0)), T0 + 20.0, Lang::German).unwrap_err();
    assert!(err.contains("läuft noch: PID 103"), "{err}");
    assert!(err.contains("Port noch belegt :3000 (PID 103)"), "{err}");
    let rows = w.rows(T0 + 21.0, Lang::German);
    assert_eq!(rows[0].pid, 103, "103 ist jetzt die Wurzel");
}

#[test]
fn access_denied_reads_as_such_in_the_row() {
    let (_tmp, _sys, w) = setup(FakeSys { denied: HashSet::from([102]), ..FakeSys::default() });
    let err = w.kill(&row_id(key(102, T0 + 2.0)), T0 + 20.0, Lang::English).unwrap_err();
    assert!(err.contains("PID 102: access denied (elevated?)"), "{err}");
    let rows = w.rows(T0 + 21.0, Lang::English);
    assert_eq!(rows[0].error.as_deref(), Some(err.as_str()));
    assert!(!rows[0].busy);
}

#[test]
fn kill_all_stops_every_row() {
    let (_tmp, sys, w) = setup(FakeSys::default());
    sys.add(110, 100, "dotnet.exe", T0 + 5.0);
    // 110 hing an der (inzwischen toten) Shell und wird beim nächsten Blick erkannt.
    w.scan(T0 + 13.0);
    assert_eq!(w.rows(T0 + 14.0, Lang::German).len(), 2);
    w.kill_all(T0 + 14.0, Lang::German).unwrap();
    assert!(w.rows(T0 + 15.0, Lang::German).is_empty());
    assert_eq!(sys.killed.lock().unwrap().len(), 3);
}

#[test]
fn a_server_whose_launching_tool_shell_vanished_unseen_is_found_by_its_marker_and_killed() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path().join("state");
    std::fs::create_dir_all(&state).unwrap();
    let created = unix_to_filetime(T0) as i64;
    std::fs::write(state.join("B3.wtpid"), format!(r#"{{"pid":200,"created":{created},"slot":"B3"}}"#)).unwrap();
    let sys = Arc::new(FakeSys::default());
    sys.add(150, 1, "explorer.exe", T0 - 500.0);
    sys.add(200, 1, "pwsh.exe", T0);
    sys.add(201, 200, "claude.exe", T0 + 1.0);
    // Die Werkzeug-Shell 202 hat node 203 abgesetzt und war weg, bevor jemand hinsah.
    sys.add(203, 202, "node.exe", T0 + 5.0);
    sys.ports.lock().unwrap().push((203, 5173));
    sys.markers.lock().unwrap().extend([(201, (200, created)), (203, (200, created))]);
    let w = OrphanWatch::new(sys.clone(), tmp.path(), &state);
    w.scan(T0 + 6.0);
    w.scan(T0 + 7.0);
    let reads = sys.marker_reads.lock().unwrap().clone();
    assert_eq!(reads, vec![203], "nur der nicht zugeordnete, junge Prozess, und nur einmal");
    sys.remove(200);
    sys.remove(201);
    w.scan(T0 + 8.0);
    let rows = w.rows(T0 + 9.0, Lang::German);
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].pid, rows[0].port_text.as_str(), rows[0].origin.as_str()), (203, ":5173", "B3 · gerade zu"));
    w.kill(&rows[0].id, T0 + 10.0, Lang::German).unwrap();
    assert_eq!(*sys.killed.lock().unwrap(), vec![key(203, T0 + 5.0)]);
}
