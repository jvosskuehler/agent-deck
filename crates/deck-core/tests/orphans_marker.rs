//! Die Herkunftsmarke `AGENT_DECK_SHELL`: Wert, Lesen aus dem Umgebungsblock und Regel 3
//! des Merkers (Zuordnung, wenn die Eltern-Kette gerissen ist).

use std::cell::RefCell;

use deck_core::orphans::model::unix_to_filetime;
use deck_core::orphans::{ProcDetails, ProcRow, ShellId, Tracker, marker_in_env, marker_value, marks_shell, parse_marker};

const T0: f64 = 1_800_000_000.0;

fn ft(unix: f64) -> u64 {
    unix_to_filetime(unix)
}

fn p(pid: u32, ppid: u32, exe: &str, started: f64) -> ProcRow {
    ProcRow { pid, ppid, exe: exe.into(), start: Some(ft(started)) }
}

fn shell(slot: &str, pid: u32, started: f64) -> ShellId {
    ShellId { slot: slot.into(), pid, created: ft(started) as i64 }
}

/// Marken je PID, wie die Umgebung sie hergäbe; zählt, wer gelesen wurde.
struct Env {
    marks: Vec<(u32, (u32, i64))>,
    reads: RefCell<Vec<u32>>,
}

impl Env {
    fn new(marks: &[(u32, u32, f64)]) -> Env {
        Env { marks: marks.iter().map(|(pid, shell, started)| (*pid, (*shell, ft(*started) as i64))).collect(), reads: RefCell::default() }
    }
}

fn observe(tr: &mut Tracker, now: f64, shells: &[ShellId], procs: &[ProcRow], env: &Env) -> bool {
    let mut marker = |pid: u32, _| {
        env.reads.borrow_mut().push(pid);
        env.marks.iter().find(|(p, _)| *p == pid).map(|(_, m)| *m)
    };
    tr.observe(now, shells, procs, &mut marker, &mut |pid, _| Some(ProcDetails { cmdline: Some(format!("cmd {pid}")), image: None, session: Some(1) }))
}

fn orphan_pids(tr: &Tracker) -> Vec<u32> {
    let mut v: Vec<u32> = tr.orphans().iter().map(|t| t.pid).collect();
    v.sort_unstable();
    v
}

/// Agent A2: Shell 100, claude 101. Die Werkzeug-Shell (777) ist schon weg; node 105
/// zeigt auf sie, und niemand hat sie je gesehen.
fn agent_with_server() -> Vec<ProcRow> {
    vec![p(100, 50, "pwsh.exe", T0), p(101, 100, "claude.exe", T0 + 1.0), p(105, 777, "node.exe", T0 + 8.0)]
}

#[test]
fn the_marker_value_names_pid_and_start_and_parses_back() {
    assert_eq!(marker_value(4711, 134_000_000_000_000_000), "4711.134000000000000000");
    assert_eq!(parse_marker(" 4711.134000000000000000 "), Some((4711, 134_000_000_000_000_000)));
    for bad in ["", "4711", "x.1", "0.5", "4711.", "-1.5"] {
        assert_eq!(parse_marker(bad), None, "{bad:?}");
    }
}

#[test]
fn the_marker_is_found_in_an_environment_block_whatever_the_case_of_its_name() {
    let block = "=C:=C:\\x\0Path=C:\\bin\0agent_deck_shell=100.42\0AGENT_SLOT=A2";
    assert_eq!(marker_in_env(block), Some((100, 42)));
    assert_eq!(marker_in_env("PATH=x\0AGENT_DECK_SHELLX=1.2\0"), None);
    assert_eq!(marker_in_env(""), None);
}

#[test]
fn a_marker_matches_its_shell_within_the_powershell_second() {
    assert!(marks_shell((100, ft(T0) as i64), 100, ft(T0 + 0.5)));
    assert!(!marks_shell((100, ft(T0) as i64), 100, ft(T0 + 3.0)));
    assert!(!marks_shell((101, ft(T0) as i64), 100, ft(T0)));
}

#[test]
fn a_server_whose_launching_shell_died_unseen_belongs_to_the_agent_through_its_marker() {
    let mut tr = Tracker::default();
    let env = Env::new(&[(101, 100, T0), (105, 100, T0)]);
    observe(&mut tr, T0 + 10.0, &[shell("A2", 100, T0)], &agent_with_server(), &env);
    assert!(tr.get((105, ft(T0 + 8.0))).is_some_and(|t| t.shell_pid == 100));
    assert_eq!(*env.reads.borrow(), vec![105], "claude kam über die Eltern-Kette, ohne Lesen");

    observe(&mut tr, T0 + 12.0, &[], &[p(105, 777, "node.exe", T0 + 8.0)], &env);
    assert_eq!(orphan_pids(&tr), vec![105]);
}

#[test]
fn without_a_marker_the_same_server_stays_unknown_which_is_the_gap_the_marker_closes() {
    let mut tr = Tracker::default();
    observe(&mut tr, T0 + 10.0, &[shell("A2", 100, T0)], &agent_with_server(), &Env::new(&[]));
    assert!(tr.get((105, ft(T0 + 8.0))).is_none());
}

#[test]
fn each_environment_is_read_once_and_never_for_processes_older_than_every_shell() {
    let mut tr = Tracker::default();
    let mut procs = agent_with_server();
    procs.extend([p(300, 4, "svchost.exe", T0 - 900.0), p(301, 1, "code.exe", T0 + 9.0)]);
    let env = Env::new(&[(105, 100, T0)]);
    let s = [shell("A2", 100, T0)];
    observe(&mut tr, T0 + 10.0, &s, &procs, &env);
    observe(&mut tr, T0 + 12.0, &s, &procs, &env);
    let mut reads = env.reads.borrow().clone();
    reads.sort_unstable();
    assert_eq!(reads, vec![105, 301], "jeder junge, nicht zugeordnete Prozess genau einmal; 300 ist älter als jede Shell");
    assert!(tr.get((301, ft(T0 + 9.0))).is_none(), "ohne Marke kein Agentenprozess");
}

#[test]
fn without_any_agent_shell_no_environment_is_read() {
    let mut tr = Tracker::default();
    let env = Env::new(&[]);
    observe(&mut tr, T0 + 10.0, &[], &agent_with_server(), &env);
    assert!(env.reads.borrow().is_empty());
}

#[test]
fn a_marker_seen_before_its_pidfile_counts_once_the_shell_is_known_without_reading_again() {
    let mut tr = Tracker::default();
    let env = Env::new(&[(105, 100, T0)]);
    let procs = agent_with_server();
    // Eine andere Shell ist schon bekannt, A2 schreibt ihre .wtpid erst später.
    let other = [shell("A1", 90, T0 - 5.0)];
    let mut with_other = procs.clone();
    with_other.push(p(90, 50, "pwsh.exe", T0 - 5.0));
    observe(&mut tr, T0 + 10.0, &other, &with_other, &env);
    assert!(tr.get((105, ft(T0 + 8.0))).is_none());
    observe(&mut tr, T0 + 12.0, &[other[0].clone(), shell("A2", 100, T0)], &with_other, &env);
    assert!(tr.get((105, ft(T0 + 8.0))).is_some_and(|t| t.shell_pid == 100));
    assert_eq!(env.reads.borrow().iter().filter(|p| **p == 105).count(), 1);
}

#[test]
fn a_marker_naming_a_shell_that_started_after_the_process_is_a_reused_pid() {
    let mut tr = Tracker::default();
    // node 106 trägt die Marke einer früheren Shell mit PID 100; die heutige 100 ist jünger.
    let procs = vec![p(100, 50, "pwsh.exe", T0), p(106, 1, "node.exe", T0 - 30.0)];
    let env = Env::new(&[(106, 100, T0 - 60.0)]);
    observe(&mut tr, T0 + 10.0, &[shell("A2", 100, T0)], &procs, &env);
    assert!(tr.tracked().next().is_none());
}
