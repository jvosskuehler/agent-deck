//! „Läuft noch ohne Fenster“ gegen das ECHTE System, nur mit selbst gestarteten Prozessen.
//!
//! Läuft nicht mit `cargo test`, nur ausdrücklich (startet node und pwsh, beendet sie):
//!
//! ```text
//! cargo test -p deck-services --test orphans_real -- --ignored --nocapture
//! ```
//!
//! Ablauf: eine versteckte pwsh spielt die Agenten-Shell (schreibt ihre `.wtpid` wie
//! `wt_agent.ps1`) und startet (a) einen node-HTTP-Server auf einem freien Port, (b) über
//! einen Zwischenprozess, der danach endet, einen Enkel, (c) node ohne Port, (d) node
//! per `Start-Process -WindowStyle Hidden`. Daneben läuft ein fremder node, den die Shell
//! nicht kennt. Der Dienst schaut wie im Betrieb hin; dann wird die Shell beendet. Erwartet:
//! (a) bis (d) stehen als Waisen da (Slot, Port), „Beenden“ räumt sie ab, der Port ist
//! frei, der fremde node läuft weiter. Beendet werden darf über den Dienst NUR, was der
//! Test selbst gestartet hat; jeder andere Versuch landet in `violations`.
//!
//! Zusätzlich gemessen (nicht verlangt): (b2) ein Zwischenprozess, der sofort nach dem
//! Start seines Kindes endet, also nie gesehen wurde. Der Snapshot-Ansatz kann diesen
//! Enkel nicht zuordnen; der Test schreibt das Ergebnis nur ins Protokoll.

use std::collections::HashMap;
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use deck_core::i18n::Lang;
use deck_core::orphans::{Key, ProcDetails, ProcRow, parse_row_id};
use deck_services::orphans::{OrphanWatch, ProcSource, Win32Source};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const SLOT: &str = "T9";

/// Die echte Quelle, aber Beenden nur für erlaubte (selbst gestartete) Prozesse.
struct Guarded {
    real: Win32Source,
    allowed: Mutex<Vec<Key>>,
    violations: Mutex<Vec<Key>>,
    terminated: Mutex<Vec<Key>>,
}

impl ProcSource for Guarded {
    fn snapshot(&self) -> Option<Vec<ProcRow>> {
        self.real.snapshot()
    }
    fn details(&self, pid: u32, start: u64) -> Option<ProcDetails> {
        self.real.details(pid, start)
    }
    fn listening(&self) -> Option<Vec<(u32, u16)>> {
        self.real.listening()
    }
    fn terminate(&self, pid: u32, start: u64) -> Result<bool, String> {
        if !self.allowed.lock().unwrap().contains(&(pid, start)) {
            self.violations.lock().unwrap().push((pid, start));
            return Err(format!("Test-Gitter: PID {pid} gehört nicht zum Test"));
        }
        self.terminated.lock().unwrap().push((pid, start));
        self.real.terminate(pid, start)
    }
    fn running(&self, pid: u32, start: u64) -> bool {
        self.real.running(pid, start)
    }
    fn wait_any_exit(&self, procs: &[Key], timeout: Duration) -> bool {
        self.real.wait_any_exit(procs, timeout)
    }
}

const NODE_IDLE: &str = r#"const fs=require('fs');const [,,dir,name]=process.argv;
fs.writeFileSync(dir+'/'+name+'.pid',String(process.pid));setInterval(()=>{},1e6);"#;
const NODE_SERVER: &str = r#"const fs=require('fs');const [,,dir,name]=process.argv;
fs.writeFileSync(dir+'/'+name+'.pid',String(process.pid));
const s=require('http').createServer((q,r)=>r.end('ok'));
s.listen(0,'127.0.0.1',()=>fs.writeFileSync(dir+'/'+name+'.port',String(s.address().port)));
setInterval(()=>{},1e6);"#;
/// Zwischenprozess: startet den Enkel abgekoppelt (wie ein Hintergrundjob) und endet nach
/// `wait` Millisekunden.
const NODE_SPAWNER: &str = r#"const fs=require('fs');const path=require('path');const [,,dir,name,child,wait]=process.argv;
fs.writeFileSync(dir+'/'+name+'.pid',String(process.pid));
require('child_process').spawn(process.execPath,[path.join(dir,'idle.js'),dir,child],{detached:true,stdio:'ignore',windowsHide:true}).unref();
setTimeout(()=>process.exit(0),Number(wait));"#;

fn shell_script(dir: &Path, state: &Path) -> String {
    let d = dir.display();
    format!(
        r#"$ErrorActionPreference = 'Stop'
$me = Get-Process -Id $PID
$rec = @{{ pid = $PID; created = $me.StartTime.ToFileTimeUtc(); slot = '{SLOT}' }} | ConvertTo-Json -Compress
[IO.File]::WriteAllText('{state}\{SLOT}.wtpid', $rec)
$node = (Get-Command node).Source
Start-Process -FilePath $node -ArgumentList @('"{d}\server.js"', '"{d}"', 'a') -NoNewWindow
Start-Process -FilePath $node -ArgumentList @('"{d}\spawner.js"', '"{d}"', 'mid', 'grand', '3000') -NoNewWindow
Start-Process -FilePath $node -ArgumentList @('"{d}\idle.js"', '"{d}"', 'c') -NoNewWindow
Start-Process -FilePath $node -ArgumentList @('"{d}\idle.js"', '"{d}"', 'd') -WindowStyle Hidden
Start-Sleep -Seconds 5
Start-Process -FilePath $node -ArgumentList @('"{d}\spawner.js"', '"{d}"', 'mid2', 'grand2', '0') -NoNewWindow
Start-Sleep -Seconds 600
"#,
        state = state.display()
    )
}

fn encoded(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in bytes.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            out.push(if i <= c.len() { B64[((n >> (18 - 6 * i)) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

fn hidden(cmd: &mut Command) -> std::io::Result<Child> {
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(CREATE_NO_WINDOW).spawn()
}

/// PID samt Startzeit aus der `.pid`-Datei eines Testprozesses.
fn proc_of(dir: &Path, name: &str) -> Option<Key> {
    let pid: u32 = fs::read_to_string(dir.join(format!("{name}.pid"))).ok()?.trim().parse().ok()?;
    Some((pid, deck_win::procs::process_start(pid)?))
}

fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64()
}

#[test]
#[ignore = "startet und beendet echte Prozesse; ausdrücklich mit --ignored laufen lassen"]
fn orphans_of_a_closed_agent_shell_are_found_and_killed_and_nothing_else_is_touched() {
    let tmp = tempfile::tempdir().unwrap();
    let dir: PathBuf = tmp.path().join("procs");
    let state = tmp.path().join("state");
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir_all(&state).unwrap();
    fs::write(dir.join("idle.js"), NODE_IDLE).unwrap();
    fs::write(dir.join("server.js"), NODE_SERVER).unwrap();
    fs::write(dir.join("spawner.js"), NODE_SPAWNER).unwrap();

    let mut unrelated = hidden(Command::new("node").arg(dir.join("idle.js")).arg(&dir).arg("u")).expect("node startet");
    let mut shell = hidden(Command::new("pwsh").args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded(&shell_script(&dir, &state))]))
        .expect("pwsh startet");

    let src = Arc::new(Guarded { real: Win32Source, allowed: Mutex::default(), violations: Mutex::default(), terminated: Mutex::default() });
    let watch = OrphanWatch::new(src.clone(), tmp.path(), &state);
    let mut scan_ms = Vec::new();
    let mut scan = |w: &OrphanWatch| {
        let t = Instant::now();
        w.scan(now());
        scan_ms.push(t.elapsed().as_secs_f64() * 1000.0);
    };

    // Wie im Betrieb: hinschauen, solange der Agent lebt (hier alle 500 ms statt 2 s,
    // damit der Test nicht trödelt; die Regel ist dieselbe).
    let names = ["a", "mid", "grand", "c", "d", "u"];
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        scan(&watch);
        let ready = names.iter().all(|n| dir.join(format!("{n}.pid")).exists()) && dir.join("a.port").exists() && dir.join("grand2.pid").exists();
        // Der Zwischenprozess (b) muss gesehen worden UND wieder weg sein.
        let mid_gone = proc_of(&dir, "mid").is_none() && dir.join("mid.pid").exists();
        if ready && mid_gone {
            break;
        }
        assert!(Instant::now() < end, "Testprozesse kamen nicht hoch");
        std::thread::sleep(Duration::from_millis(500));
    }
    let keys: HashMap<&str, Key> = ["a", "grand", "c", "d", "u", "grand2"].iter().map(|n| (*n, proc_of(&dir, n).unwrap_or_else(|| panic!("{n} läuft")))).collect();
    let port: u16 = fs::read_to_string(dir.join("a.port")).unwrap().trim().parse().unwrap();
    assert!(watch.rows(now(), Lang::German).is_empty(), "Fenster offen: keine Waisen");
    let shell_pid = watch.tracker().live_shells().first().copied().expect("Shell aus der .wtpid erkannt");
    *src.allowed.lock().unwrap() = keys.values().copied().collect();

    // „Fenster zu“: die Shell endet. Der Dienst wacht sofort auf und schaut nach.
    let t = Instant::now();
    shell.kill().unwrap();
    let _ = shell.wait();
    assert_eq!(shell.id(), shell_pid.0, "pwsh selbst ist die Shell der .wtpid");
    watch.wait(Duration::from_secs(5));
    let woke_ms = t.elapsed().as_millis();
    assert!(woke_ms < 1500, "Ende der Shell sofort bemerkt ({woke_ms} ms)");
    scan(&watch);

    let rows = watch.rows(now(), Lang::German);
    println!("Zeilen nach dem Schließen:");
    for r in &rows {
        println!("  {:<8} {:<6} {:<40} {}  [{}]", r.port_text, r.program, r.command, r.origin, r.id);
    }
    let row_of = |k: Key| rows.iter().find(|r| parse_row_id(&r.id) == Some(k));
    for name in ["a", "grand", "c", "d"] {
        let r = row_of(keys[name]).unwrap_or_else(|| panic!("({name}) fehlt in den Waisen"));
        assert!(r.origin.starts_with(&format!("{SLOT} · ")), "{}", r.origin);
    }
    assert_eq!(row_of(keys["a"]).unwrap().ports, vec![port]);
    assert_eq!(row_of(keys["c"]).unwrap().port_text, "kein Port");
    assert!(row_of(keys["u"]).is_none(), "fremder node steht nicht in der Liste");
    assert!(watch.tracker().tracked().all(|t| t.key() != keys["u"]), "fremder node wird nicht einmal gemerkt");
    let b2 = row_of(keys["grand2"]).is_some();
    println!("(b2) Enkel eines nie gesehenen Zwischenprozesses erkannt: {b2}");

    // Beenden über den Weg der Oberfläche, Zeile für Zeile.
    for name in ["a", "grand", "c", "d"] {
        let id = deck_core::orphans::row_id(keys[name]);
        watch.kill(&id, now(), Lang::German).unwrap_or_else(|e| panic!("({name}) nicht beendet: {e}"));
    }
    for name in ["a", "grand", "c", "d"] {
        let (pid, start) = keys[name];
        assert!(!src.real.running(pid, start), "({name}) läuft noch");
    }
    assert!(std::net::TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(300)).is_err(), "Port {port} ist frei");
    assert!(src.real.listening().unwrap().iter().all(|(_, p)| *p != port));
    assert!(src.real.running(keys["u"].0, keys["u"].1), "fremder node läuft weiter");
    assert!(src.violations.lock().unwrap().is_empty(), "nur Testprozesse beendet: {:?}", src.violations.lock().unwrap());
    scan(&watch);
    let left: Vec<String> = watch.rows(now(), Lang::German).iter().map(|r| r.id.clone()).collect();
    println!("Nach dem Beenden übrig: {left:?}");
    assert!(left.iter().all(|id| b2 && parse_row_id(id) == Some(keys["grand2"])), "nur (b2) darf übrig sein");

    let avg = scan_ms.iter().sum::<f64>() / scan_ms.len() as f64;
    let max = scan_ms.iter().cloned().fold(0.0, f64::max);
    println!("Scan: {} Läufe, Mittel {avg:.1} ms, Maximum {max:.1} ms; Shell-Ende bemerkt nach {woke_ms} ms", scan_ms.len());
    println!("Beendet über den Dienst: {:?}", src.terminated.lock().unwrap());

    // Aufräumen: nur, was dieser Test gestartet hat.
    let _ = unrelated.kill();
    let _ = unrelated.wait();
    if let Some((pid, start)) = proc_of(&dir, "grand2") {
        let _ = deck_win::proc_scan::terminate_exact(pid, start);
    }
}
