//! Was „Läuft noch ohne Fenster“ zeigt und was „Beenden“ trifft: Sperrliste, Bäume als
//! eine Zeile, Reihenfolge, Texte.

use std::collections::HashMap;

use deck_core::i18n::Lang;
use deck_core::orphans::model::unix_to_filetime;
use deck_core::orphans::rows::{closed_since, port_text};
use deck_core::orphans::rules::display_command;
use deck_core::orphans::{KillState, ProcDetails, ProcRow, Protected, ShellId, Tracker, kill_order, orphan_rows, protected, roots, row_id};

const T0: f64 = 1_800_000_000.0;

fn ft(unix: f64) -> u64 {
    unix_to_filetime(unix)
}

struct Proc {
    pid: u32,
    ppid: u32,
    exe: &'static str,
    started: f64,
    cmd: &'static str,
    session: u32,
}

fn pr(pid: u32, ppid: u32, exe: &'static str, started: f64, cmd: &'static str) -> Proc {
    Proc { pid, ppid, exe, started, cmd, session: 1 }
}

/// Agenten-Shells (Slot, PID, Zeitpunkt des Schließens) und Prozesse darunter, die
/// weiterlaufen. Jede Shell geht zu ihrer Zeit zu (unendlich = bleibt offen).
fn orphaned(shells: &[(&str, u32, f64)], procs: &[Proc]) -> Tracker {
    let mut tr = Tracker::default();
    let rows = |with_shells: bool| -> Vec<ProcRow> {
        let mut v: Vec<ProcRow> = procs.iter().map(|p| ProcRow { pid: p.pid, ppid: p.ppid, exe: p.exe.into(), start: Some(ft(p.started)) }).collect();
        if with_shells {
            v.extend(shells.iter().map(|(_, pid, _)| ProcRow { pid: *pid, ppid: 1, exe: "pwsh.exe".into(), start: Some(ft(T0)) }));
        }
        v
    };
    let ids: Vec<ShellId> = shells.iter().map(|(s, pid, _)| ShellId { slot: (*s).into(), pid: *pid, created: ft(T0) as i64 }).collect();
    let mut details = |pid: u32, _| {
        procs.iter().find(|p| p.pid == pid).map(|p| ProcDetails { cmdline: Some(p.cmd.into()), image: None, session: Some(p.session) })
    };
    tr.observe(T0 + 10.0, &ids, &rows(true), &mut |_, _| None, &mut details);
    // Jede Shell geht zu ihrer Zeit zu; eine Sekunde vorher sieht der Merker noch alle,
    // die dann offen sind (wie im Betrieb, wo er alle 2 s schaut).
    let mut closes: Vec<f64> = shells.iter().map(|(.., c)| *c).filter(|c| c.is_finite()).collect();
    closes.sort_by(f64::total_cmp);
    for c in closes {
        for (at, open_after) in [(c - 1.0, c - 1.0), (c, c)] {
            let open: Vec<usize> = (0..shells.len()).filter(|i| shells[*i].2 > open_after).collect();
            let still: Vec<ShellId> = open.iter().map(|i| ids[*i].clone()).collect();
            let mut r = rows(false);
            r.extend(open.iter().map(|i| ProcRow { pid: shells[*i].1, ppid: 1, exe: "pwsh.exe".into(), start: Some(ft(T0)) }));
            tr.observe(at, &still, &r, &mut |_, _| None, &mut details);
        }
    }
    tr
}

/// Exe, Exe-Pfad, Kommandozeile, Sitzung, erwarteter Grund.
type Case<'a> = (&'a str, Option<&'a str>, Option<&'a str>, Option<u32>, Protected);

const NODE: &str = "\"C:\\Program Files\\nodejs\\node.exe\"";

#[test]
fn claude_the_deck_docker_consoles_terminal_explorer_and_session_zero_are_never_offered() {
    let cases: [Case; 10] = [
        ("claude.exe", None, None, Some(1), Protected::Claude),
        ("node.exe", None, Some("node C:\\Users\\x\\AppData\\Roaming\\npm\\node_modules\\@anthropic-ai\\claude-code\\cli.js"), Some(1), Protected::Claude),
        ("node.exe", None, Some("node C:\\Users\\x\\.local\\bin\\claude.js --resume"), Some(1), Protected::Claude),
        ("agent-deck.exe", None, None, Some(1), Protected::Deck),
        ("helper.exe", Some("C:\\Users\\x\\AppData\\Local\\claude-agent-deck\\bin\\helper.exe"), None, Some(1), Protected::Deck),
        ("com.docker.backend.exe", None, None, Some(1), Protected::Docker),
        ("conhost.exe", None, None, Some(1), Protected::Console),
        ("WindowsTerminal.exe", None, None, Some(1), Protected::Terminal),
        ("explorer.exe", None, None, Some(1), Protected::Explorer),
        ("node.exe", None, Some("node server.js"), Some(0), Protected::Session0),
    ];
    for (exe, image, cmd, session, why) in cases {
        assert_eq!(protected(1000, exe, image, cmd, session), Some(why), "{exe} {cmd:?}");
    }
    assert_eq!(protected(4, "node.exe", None, None, Some(1)), Some(Protected::System));
    assert_eq!(protected(1000, "csrss.exe", None, None, None), Some(Protected::System));
}

#[test]
fn a_dev_server_whose_command_line_mentions_claudes_temp_folder_is_not_claude() {
    let cmd = "bash -c \"pnpm dev > C:\\Users\\x\\AppData\\Local\\Temp\\claude\\abc\\tasks\\b1.output\"";
    assert_eq!(protected(1000, "bash.exe", None, Some(cmd), Some(1)), None);
    assert_eq!(protected(1000, "node.exe", None, Some("node C:\\repo\\claude-tools\\x.js"), Some(1)), None);
    assert_eq!(protected(1000, "dotnet.exe", None, None, None), None, "unbekannte Sitzung schützt nicht");
}

#[test]
fn the_shown_command_drops_the_program_and_shortens_paths_and_script_names() {
    let pnpm = format!("{NODE} C:\\Users\\x\\AppData\\Roaming\\npm\\node_modules\\pnpm\\bin\\pnpm.cjs nx serve management-ui");
    assert_eq!(display_command(Some(&pnpm), "node.exe"), "pnpm nx serve management-ui");
    let api = "\"C:\\Program Files\\dotnet\\dotnet.exe\" C:\\repo\\bin\\Debug\\net9.0\\Idm.Orpheus.Management.Api.dll";
    assert_eq!(display_command(Some(api), "dotnet.exe"), "Idm.Orpheus.Management.Api.dll");
    assert_eq!(display_command(Some("node -e \"setInterval(()=>{}, 1000)\""), "node.exe"), "-e setInterval(()=>{}, 1000)");
    assert_eq!(display_command(Some("node.exe"), "node.exe"), "node");
    assert_eq!(display_command(None, "dotnet.exe"), "dotnet");
    assert_eq!(display_command(Some("curl http://localhost:3000/x"), "curl.exe"), "http://localhost:3000/x");
}

#[test]
fn a_process_tree_is_one_row_with_the_ports_of_all_its_members() {
    let pnpm = "node C:\\npm\\node_modules\\pnpm\\bin\\pnpm.cjs nx serve management-ui";
    let procs = [
        pr(101, 100, "claude.exe", T0 + 1.0, "claude"),
        pr(102, 101, "node.exe", T0 + 2.0, pnpm),
        pr(103, 102, "node.exe", T0 + 3.0, "node C:\\repo\\node_modules\\nx\\bin\\nx.js serve"),
        pr(104, 103, "node.exe", T0 + 4.0, "node C:\\repo\\node_modules\\vite\\bin\\vite.js"),
        pr(105, 104, "conhost.exe", T0 + 4.5, "conhost.exe 0x4"),
    ];
    let mut tr = orphaned(&[("A2", 100, T0 + 20.0)], &procs);
    // claude stirbt mit dem Fenster.
    let alive: Vec<ProcRow> = procs[1..].iter().map(|p| ProcRow { pid: p.pid, ppid: p.ppid, exe: p.exe.into(), start: Some(ft(p.started)) }).collect();
    tr.observe(T0 + 21.0, &[], &alive, &mut |_, _| None, &mut |_, _| None);

    let ports = HashMap::from([(104, vec![5173, 3000]), (103, vec![3000])]);
    let rows = orphan_rows(&tr, &ports, &KillState::default(), T0 + 21.0 + 14.0 * 60.0, Lang::German);
    assert_eq!(rows.len(), 1, "ein Baum, eine Zeile: {rows:?}");
    let r = &rows[0];
    assert_eq!((r.pid, r.program.as_str(), r.command.as_str()), (102, "node", "pnpm nx serve management-ui"));
    assert_eq!(r.ports, vec![3000, 5173]);
    assert_eq!(r.port_text, ":3000 :5173");
    assert_eq!(r.origin, "A2 · zu seit 14 min");
    assert_eq!(r.command_full, pnpm);

    let order = kill_order(&tr, (102, ft(T0 + 2.0))).unwrap();
    let pids: Vec<u32> = order.iter().map(|k| k.0).collect();
    assert_eq!(pids, vec![102, 103, 104], "von oben nach unten, ohne conhost und ohne claude");
    assert_eq!(kill_order(&tr, (103, ft(T0 + 3.0))), None, "nur die oberste ist eine Zeile");
    assert_eq!(kill_order(&tr, (102, ft(T0 + 2.5))), None, "andere Startzeit: kein Ziel");
    assert_eq!(roots(&tr), vec![(102, ft(T0 + 2.0))]);
}

#[test]
fn nothing_is_offered_while_the_window_is_open() {
    let procs = [pr(102, 100, "node.exe", T0 + 2.0, "node server.js")];
    let tr = orphaned(&[("A1", 100, f64::INFINITY)], &procs);
    assert!(orphan_rows(&tr, &HashMap::new(), &KillState::default(), T0 + 30.0, Lang::German).is_empty());
    assert_eq!(kill_order(&tr, (102, ft(T0 + 2.0))), None);
}

#[test]
fn a_protected_orphan_is_neither_a_row_nor_killed() {
    let procs = [pr(102, 100, "claude.exe", T0 + 2.0, "claude --resume"), pr(103, 100, "node.exe", T0 + 3.0, "node a.js")];
    let tr = orphaned(&[("A1", 100, T0 + 20.0)], &procs);
    let rows = orphan_rows(&tr, &HashMap::new(), &KillState::default(), T0 + 30.0, Lang::German);
    assert_eq!(rows.iter().map(|r| r.pid).collect::<Vec<_>>(), vec![103]);
    assert_eq!(kill_order(&tr, (102, ft(T0 + 2.0))), None);
}

#[test]
fn rows_come_newest_closed_window_first_then_by_port_with_portless_last() {
    let procs = [
        pr(201, 200, "node.exe", T0 + 2.0, "node old.js"),
        pr(301, 300, "node.exe", T0 + 2.0, "node b.js"),
        pr(302, 300, "dotnet.exe", T0 + 2.0, "dotnet run"),
        pr(303, 300, "node.exe", T0 + 2.0, "node watch.js"),
    ];
    let tr = orphaned(&[("A1", 200, T0 + 20.0), ("A3", 300, T0 + 600.0)], &procs);
    let ports = HashMap::from([(301, vec![5116]), (302, vec![3000]), (201, vec![8080])]);
    let rows = orphan_rows(&tr, &ports, &KillState::default(), T0 + 3600.0, Lang::English);
    let order: Vec<u32> = rows.iter().map(|r| r.pid).collect();
    assert_eq!(order, vec![302, 301, 303, 201]);
    assert_eq!(rows[2].port_text, "no port");
    assert_eq!(rows[0].origin, "A3 · closed 50 min ago");
    assert_eq!(rows[3].origin, "A1 · closed 59 min ago");
}

#[test]
fn a_running_or_failed_kill_shows_on_its_row() {
    let procs = [pr(102, 100, "node.exe", T0 + 2.0, "node a.js")];
    let tr = orphaned(&[("A1", 100, T0 + 20.0)], &procs);
    let key = (102, ft(T0 + 2.0));
    let mut kills = KillState::default();
    kills.busy.insert(key);
    kills.errors.insert(key, "Zugriff verweigert".into());
    let r = &orphan_rows(&tr, &HashMap::new(), &kills, T0 + 30.0, Lang::German)[0];
    assert!(r.busy);
    assert_eq!(r.error.as_deref(), Some("Zugriff verweigert"));
    assert_eq!(r.id, row_id(key));
}

#[test]
fn port_and_time_texts_read_naturally_in_both_languages_without_dashes() {
    assert_eq!(port_text(&[], Lang::German), "kein Port");
    assert_eq!(port_text(&[3000], Lang::German), ":3000");
    assert_eq!(port_text(&[3000, 3001, 3002], Lang::English), ":3000 +2");
    let cases = [(5.0, "gerade zu", "just closed"), (14.0 * 60.0, "zu seit 14 min", "closed 14 min ago"), (7300.0, "zu seit 2 h", "closed 2 h ago"), (3.0 * 86_400.0, "zu seit 3 Tg.", "closed 3 d ago")];
    for (secs, de, en) in cases {
        assert_eq!(closed_since(secs, Lang::German), de);
        assert_eq!(closed_since(secs, Lang::English), en);
        assert!(!de.contains(['—', '–']) && !en.contains(['—', '–']));
    }
}
