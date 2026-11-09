//! Der Merker für „Läuft noch ohne Fenster“: wer gehört zu welchem Agenten, wann ist ein
//! Prozess eine Waise, und wann ist eine PID nur eine neu vergebene Nummer.

use deck_core::orphans::model::{filetime_to_unix, unix_to_filetime};
use deck_core::orphans::tracker::{KEEP_GONE_S, STALE_SEEN_S};
use deck_core::orphans::{ProcDetails, ProcRow, Saved, ShellId, Tracker};

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

fn details() -> impl FnMut(u32, u64) -> Option<ProcDetails> {
    |pid, _| Some(ProcDetails { cmdline: Some(format!("cmd of {pid}")), image: None, session: Some(1) })
}

fn observe(tr: &mut Tracker, now: f64, shells: &[ShellId], procs: &[ProcRow]) -> bool {
    tr.observe(now, shells, procs, &mut |_, _| None, &mut details())
}

fn orphan_pids(tr: &Tracker) -> Vec<u32> {
    let mut v: Vec<u32> = tr.orphans().iter().map(|t| t.pid).collect();
    v.sort_unstable();
    v
}

/// Ein Agent A2: Shell 100, darin claude 101, darin bash 102 mit node 103.
fn agent_tree() -> Vec<ProcRow> {
    vec![p(100, 50, "pwsh.exe", T0), p(101, 100, "claude.exe", T0 + 1.0), p(102, 101, "bash.exe", T0 + 5.0), p(103, 102, "node.exe", T0 + 6.0)]
}

#[test]
fn descendants_seen_while_the_agent_lives_are_orphans_once_its_shell_is_gone() {
    let mut tr = Tracker::default();
    let shells = [shell("A2", 100, T0)];
    assert!(observe(&mut tr, T0 + 10.0, &shells, &agent_tree()));
    assert!(tr.orphans().is_empty(), "solange das Fenster offen ist, gibt es keine Waisen");

    // Fenster zu: Shell und claude sterben, bash und node laufen weiter.
    let after = vec![p(102, 101, "bash.exe", T0 + 5.0), p(103, 102, "node.exe", T0 + 6.0)];
    observe(&mut tr, T0 + 12.0, &[], &after);
    assert_eq!(orphan_pids(&tr), vec![102, 103]);
    let rec = tr.shell((100, ft(T0))).unwrap();
    assert_eq!((rec.slot.as_str(), rec.closed_at), ("A2", Some(T0 + 12.0)));
}

#[test]
fn a_child_started_just_before_the_close_and_never_seen_is_caught_by_its_remembered_parent() {
    let mut tr = Tracker::default();
    observe(&mut tr, T0 + 10.0, &[shell("A2", 100, T0)], &agent_tree());
    // Zwischen zwei Blicken: node 103 startet ein Kind 104, dann geht das Fenster zu.
    let after = vec![p(103, 102, "node.exe", T0 + 6.0), p(104, 103, "node.exe", T0 + 11.0)];
    observe(&mut tr, T0 + 12.0, &[], &after);
    assert_eq!(orphan_pids(&tr), vec![103, 104]);
}

#[test]
fn a_grandchild_whose_parent_died_unseen_is_caught_through_the_dead_remembered_grandparent() {
    let mut tr = Tracker::default();
    observe(&mut tr, T0 + 10.0, &[shell("A2", 100, T0)], &agent_tree());
    // bash 102 (gemerkt) startet cmd 105 und stirbt; 105 startet node 106 und endet.
    // Beim nächsten Blick lebt nur noch 106, dessen Eltern-PID 105 nie gesehen wurde.
    let after = vec![p(105, 102, "cmd.exe", T0 + 10.5), p(106, 105, "node.exe", T0 + 10.6)];
    observe(&mut tr, T0 + 12.0, &[], &after);
    assert_eq!(orphan_pids(&tr), vec![105, 106], "106 hängt über 105 an der gemerkten 102");
}

#[test]
fn a_direct_child_of_the_shell_started_right_before_the_close_is_caught_through_the_closed_shell() {
    let mut tr = Tracker::default();
    observe(&mut tr, T0 + 10.0, &[shell("A2", 100, T0)], &[p(100, 50, "pwsh.exe", T0)]);
    let after = vec![p(107, 100, "node.exe", T0 + 10.5)];
    observe(&mut tr, T0 + 11.0, &[], &after);
    assert_eq!(orphan_pids(&tr), vec![107]);
}

#[test]
fn a_child_started_after_its_remembered_parent_died_is_someone_elses() {
    let mut tr = Tracker::default();
    observe(&mut tr, T0 + 10.0, &[shell("A2", 100, T0)], &agent_tree());
    observe(&mut tr, T0 + 12.0, &[], &[p(103, 102, "node.exe", T0 + 6.0)]);
    // Viel später trägt ein fremder Prozess 200 die alte Eltern-PID 102 als ppid: er
    // entstand lange nach deren Tod (die Nummer war neu vergeben).
    observe(&mut tr, T0 + 25.0, &[], &[p(103, 102, "node.exe", T0 + 6.0), p(200, 102, "Code.exe", T0 + 24.0)]);
    assert_eq!(orphan_pids(&tr), vec![103]);
}

#[test]
fn a_reused_pid_with_another_start_time_is_not_the_remembered_process() {
    let mut tr = Tracker::default();
    observe(&mut tr, T0 + 10.0, &[shell("A2", 100, T0)], &agent_tree());
    // node 103 endet; die PID 103 bekommt ein fremdes Programm.
    observe(&mut tr, T0 + 12.0, &[], &[p(103, 7, "chrome.exe", T0 + 11.5)]);
    assert!(tr.orphans().is_empty(), "gleiche PID, andere Startzeit: ein anderer Prozess");
}

#[test]
fn a_living_parent_started_after_the_child_holds_a_reused_pid_and_is_not_its_parent() {
    let mut tr = Tracker::default();
    // Shell 100 lebt; node 300 nennt ppid 400, und unter 400 läuft ein Prozess, der
    // NACH 300 gestartet ist und selbst Kind der Shell ist. Der ist nicht 300s Elter.
    let procs = vec![p(100, 50, "pwsh.exe", T0), p(400, 100, "bash.exe", T0 + 9.0), p(300, 400, "node.exe", T0 + 2.0)];
    observe(&mut tr, T0 + 10.0, &[shell("A2", 100, T0)], &procs);
    let tracked: Vec<u32> = tr.tracked().map(|t| t.pid).collect();
    assert_eq!(tracked, vec![400], "300 gehört nicht zum Agenten");
}

#[test]
fn processes_outside_every_agent_shell_are_never_tracked() {
    let mut tr = Tracker::default();
    let mut procs = agent_tree();
    procs.extend([p(900, 4, "svchost.exe", T0 - 99.0), p(901, 900, "node.exe", T0 + 3.0), p(902, 0, "explorer.exe", T0 - 50.0)]);
    observe(&mut tr, T0 + 10.0, &[shell("A2", 100, T0)], &procs);
    observe(&mut tr, T0 + 12.0, &[], &procs[2..]);
    assert!(tr.tracked().all(|t| t.pid < 900));
    assert_eq!(orphan_pids(&tr), vec![102, 103]);
}

#[test]
fn a_shell_with_a_wrong_start_time_in_its_pidfile_is_not_an_agent() {
    let mut tr = Tracker::default();
    // .wtpid nennt PID 100, aber mit einer Startzeit 5 s daneben: alte Datei, neue PID.
    observe(&mut tr, T0 + 10.0, &[shell("A2", 100, T0 - 5.0)], &agent_tree());
    assert!(tr.shells().is_empty());
    assert_eq!(tr.tracked().count(), 0);
}

#[test]
fn the_pidfile_start_time_counts_within_one_second_like_powershell_writes_it() {
    let mut tr = Tracker::default();
    observe(&mut tr, T0 + 10.0, &[shell("A2", 100, T0 + 0.4)], &agent_tree());
    assert_eq!(tr.shells().len(), 1);
    assert_eq!(tr.shells()[0].start, ft(T0), "gemerkt wird die genaue Startzeit aus dem Schnappschuss");
}

#[test]
fn without_details_a_process_is_not_remembered_yet_and_comes_again_next_time() {
    let mut tr = Tracker::default();
    tr.observe(T0 + 10.0, &[shell("A2", 100, T0)], &agent_tree(), &mut |_, _| None, &mut |_, _| None);
    assert_eq!(tr.tracked().count(), 0);
    observe(&mut tr, T0 + 12.0, &[shell("A2", 100, T0)], &agent_tree());
    assert_eq!(tr.tracked().count(), 3);
}

#[test]
fn a_recycled_slot_name_keeps_the_old_orphans_with_the_old_window() {
    let mut tr = Tracker::default();
    observe(&mut tr, T0 + 10.0, &[shell("A3", 100, T0)], &agent_tree());
    let mut later = vec![p(103, 102, "node.exe", T0 + 6.0)];
    observe(&mut tr, T0 + 12.0, &[], &later);
    // Ein neuer Agent heißt wieder A3, mit eigener Shell.
    later.extend([p(500, 50, "pwsh.exe", T0 + 20.0), p(501, 500, "claude.exe", T0 + 21.0)]);
    observe(&mut tr, T0 + 22.0, &[shell("A3", 500, T0 + 20.0)], &later);
    assert_eq!(orphan_pids(&tr), vec![103]);
    let o = tr.orphans()[0];
    assert_eq!(o.shell_pid, 100);
}

#[test]
fn dead_processes_and_empty_closed_shells_are_forgotten_after_a_while() {
    let mut tr = Tracker::default();
    observe(&mut tr, T0 + 10.0, &[shell("A2", 100, T0)], &agent_tree());
    observe(&mut tr, T0 + 12.0, &[], &[]);
    assert_eq!(tr.tracked().count(), 3, "eben gestorben: noch gemerkt (für spät auftauchende Kinder)");
    assert!(observe(&mut tr, T0 + 13.0 + KEEP_GONE_S, &[], &[]));
    assert_eq!(tr.tracked().count(), 0);
    assert!(tr.shells().is_empty());
}

#[test]
fn a_closed_shell_stays_as_long_as_one_of_its_orphans_runs() {
    let mut tr = Tracker::default();
    observe(&mut tr, T0 + 10.0, &[shell("A2", 100, T0)], &agent_tree());
    let node = [p(103, 102, "node.exe", T0 + 6.0)];
    observe(&mut tr, T0 + 12.0, &[], &node);
    observe(&mut tr, T0 + 7200.0, &[], &node);
    assert_eq!(orphan_pids(&tr), vec![103]);
    assert_eq!(tr.shell((100, ft(T0))).unwrap().closed_at, Some(T0 + 12.0));
}

#[test]
fn the_memory_survives_a_restart_of_the_deck() {
    let mut tr = Tracker::default();
    observe(&mut tr, T0 + 10.0, &[shell("A2", 100, T0)], &agent_tree());
    let bytes = tr.to_saved(T0 + 10.0).to_bytes();
    let mut again = Tracker::from_saved(Saved::parse(&bytes));
    assert_eq!(again, tr);
    // Das Deck war 10 min aus; das Fenster ging in der Zeit zu. Bekannt ist nur der letzte
    // Blick: der gilt als Zeitpunkt des Schließens.
    observe(&mut again, T0 + 610.0, &[], &[p(103, 102, "node.exe", T0 + 6.0)]);
    assert_eq!(orphan_pids(&again), vec![103]);
    const { assert!(600.0 > STALE_SEEN_S) };
    assert_eq!(again.shell((100, ft(T0))).unwrap().closed_at, Some(T0 + 10.0));
}

#[test]
fn a_broken_memory_file_starts_empty() {
    assert_eq!(Saved::parse(b"{kaputt"), Saved::default());
    assert_eq!(Saved::parse(b"[]"), Saved::default());
    let tr = Tracker::from_saved(Saved::parse(br#"{"shells":[],"procs":[]}"#));
    assert!(tr.orphans().is_empty());
}

#[test]
fn nothing_changes_when_nothing_happens() {
    let mut tr = Tracker::default();
    let shells = [shell("A2", 100, T0)];
    assert!(observe(&mut tr, T0 + 10.0, &shells, &agent_tree()));
    assert!(!observe(&mut tr, T0 + 12.0, &shells, &agent_tree()), "kein Grund, die Datei zu schreiben");
}

#[test]
fn a_parent_cycle_from_reused_pids_does_not_hang() {
    let mut tr = Tracker::default();
    let procs = vec![p(100, 50, "pwsh.exe", T0), p(600, 601, "a.exe", T0 + 1.0), p(601, 600, "b.exe", T0 + 1.0)];
    observe(&mut tr, T0 + 10.0, &[shell("A2", 100, T0)], &procs);
    assert_eq!(tr.tracked().count(), 0);
}

#[test]
fn filetime_and_unix_seconds_convert_both_ways() {
    let t = 1_762_700_000.25;
    assert!((filetime_to_unix(unix_to_filetime(t)) - t).abs() < 1e-6);
    assert_eq!(unix_to_filetime(0.0), 116_444_736_000_000_000);
}
