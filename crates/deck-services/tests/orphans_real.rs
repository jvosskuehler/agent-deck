//! „Läuft noch ohne Fenster“ gegen das ECHTE System, nur mit selbst gestarteten Prozessen.
//!
//! Läuft nicht mit `cargo test`, nur ausdrücklich (startet node, pwsh und cmd versteckt und
//! beendet sie):
//!
//! ```text
//! cargo test -p deck-services --test orphans_real -- --ignored --nocapture
//! ```
//!
//! Ablauf: eine versteckte pwsh spielt die Agenten-Shell. Sie führt den Kopf des echten
//! `resources/wt_agent.ps1` aus (alles vor „4)“, mit einer Spec aus `wt::launch_spec`):
//! `.wtpid` schreiben, `AGENT_SLOT` und die Herkunftsmarke `AGENT_DECK_SHELL` setzen. Dann
//! startet sie (a) einen node-HTTP-Server, (b) über einen Zwischenprozess, der nach 3 s
//! endet, einen Enkel, (b2) dasselbe mit einem Zwischenprozess, der sofort endet, (c) node
//! ohne Port, (d) node per `Start-Process -WindowStyle Hidden` und (sim) einen node, der
//! Claude spielt. Daneben läuft ein fremder node (u), den die Shell nicht kennt.
//!
//! Der Fall, an dem die Eltern-Kette scheitert: auf ein Zeichen startet (sim) wie Claudes
//! Werkzeug eine frische pwsh mit `Start-Process -WindowStyle Hidden node server.js` (e)
//! und ein `cmd /c start /b node server.js` (f). Beide Zwischenglieder enden sofort, und
//! während sie leben, schaut der Dienst bewusst NICHT hin: kein Schnappschuss hat sie je
//! gesehen. Ein zweiter Dienst, der keine Marken liest, zeigt, dass die Eltern-Kette allein
//! (e) und (f) nicht findet.
//!
//! Dann wird die Shell beendet („Fenster zu“). Erwartet: alles außer (u) steht als Waise da
//! (Slot, Ports), „Beenden“ räumt es ab, die Ports sind frei, (u) läuft weiter. Beendet
//! werden darf über den Dienst NUR, was der Test selbst gestartet hat; jeder andere Versuch
//! landet in `violations`. Scheitert der Test, räumt [`Cleanup`] seine Prozesse trotzdem ab.

use std::collections::HashMap;
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use deck_core::i18n::Lang;
use deck_core::orphans::{Key, ProcDetails, ProcRow, parse_row_id, row_id};
use deck_services::orphans::{OrphanWatch, ProcSource, Win32Source, read_shells};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const SLOT: &str = "T9";

/// Die echte Quelle, aber Beenden nur für erlaubte (selbst gestartete) Prozesse.
#[derive(Default)]
struct Guarded {
    real: Win32Source,
    /// Keine Marken lesen (so ordnete der Dienst vor der Marke zu).
    blind: bool,
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
    fn marker(&self, pid: u32, start: u64) -> Option<(u32, i64)> {
        if self.blind { None } else { self.real.marker(pid, start) }
    }
}

/// Räumt beim Verlassen ab, was der Test gestartet hat: die beiden direkten Kinder und
/// jeden Prozess aus einer `.pid`-Datei, dessen Kommandozeile den Testordner nennt.
struct Cleanup {
    dir: PathBuf,
    children: Vec<Child>,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        for c in &mut self.children {
            let _ = c.kill();
            let _ = c.wait();
        }
        let marker = self.dir.to_string_lossy().to_string();
        for e in fs::read_dir(&self.dir).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let Some(stem) = name.strip_suffix(".pid") else { continue };
            let Some((pid, start)) = proc_of(&self.dir, stem) else { continue };
            let ours = deck_win::proc_scan::details(pid, start).and_then(|d| d.cmdline).is_some_and(|c| c.contains(&marker));
            if ours {
                let _ = deck_win::proc_scan::terminate_exact(pid, start);
            }
        }
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
/// Spielt Claude: wartet auf `go`, dann wie das Bash-/PowerShell-Werkzeug je eine frische
/// Shell (nicht abgekoppelt, wie bei Claude), die einen Server absetzt und sofort endet.
/// Schreibt PID und Lebensdauer der Zwischenglieder.
const NODE_AGENT: &str = r#"const fs=require('fs'),cp=require('child_process');const [,,dir]=process.argv;
fs.writeFileSync(dir+'/sim.pid',String(process.pid));
const node=process.execPath, srv=dir+'\\server.js';
const mid=(name,cmd,args,opts)=>{const t=Date.now();const c=cp.spawn(cmd,args,{windowsHide:true,stdio:'ignore',...opts});
  fs.writeFileSync(dir+'/'+name+'.pid',String(c.pid));
  c.on('exit',()=>fs.writeFileSync(dir+'/'+name+'.exit',String(Date.now()-t)));};
const go=()=>{if(!fs.existsSync(dir+'/go'))return setTimeout(go,20);
  mid('e_mid','pwsh',['-NoProfile','-NonInteractive','-Command',`Start-Process -FilePath '${node}' -ArgumentList '"${srv}"','"${dir}"','e' -WindowStyle Hidden`]);
  mid('f_mid','cmd',[`/d /s /c "start "" /b "${node}" "${srv}" "${dir}" f"`],{windowsVerbatimArguments:true});};
go();setInterval(()=>{},1e6);"#;

/// Der Kopf des echten Startskripts: alles vor „# 4)“ (Spec lesen, `.wtpid`, Umgebung samt
/// Marke), ohne claude zu starten.
fn agent_head() -> String {
    let script = include_str!("../../../resources/wt_agent.ps1");
    let end = script.find("\n# 4)").expect("wt_agent.ps1 hat einen Schritt 4");
    script[..end].to_string()
}

fn shell_script(dir: &Path, head: &Path, spec: &str) -> String {
    let d = dir.display();
    format!(
        r#"$ErrorActionPreference = 'Stop'
. '{head}' -Spec '{spec}'
$node = (Get-Command node).Source
Start-Process -FilePath $node -ArgumentList @('"{d}\server.js"', '"{d}"', 'a') -NoNewWindow
Start-Process -FilePath $node -ArgumentList @('"{d}\spawner.js"', '"{d}"', 'mid', 'grand', '3000') -NoNewWindow
Start-Process -FilePath $node -ArgumentList @('"{d}\idle.js"', '"{d}"', 'c') -NoNewWindow
Start-Process -FilePath $node -ArgumentList @('"{d}\idle.js"', '"{d}"', 'd') -WindowStyle Hidden
Start-Process -FilePath $node -ArgumentList @('"{d}\agent-sim.js"', '"{d}"') -NoNewWindow
Start-Sleep -Seconds 5
Start-Process -FilePath $node -ArgumentList @('"{d}\spawner.js"', '"{d}"', 'mid2', 'grand2', '0') -NoNewWindow
Start-Sleep -Seconds 600
"#,
        head = head.display()
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

fn wait_for(what: &str, secs: u64, mut ready: impl FnMut() -> bool, mut each: impl FnMut()) {
    let end = Instant::now() + Duration::from_secs(secs);
    while !ready() {
        assert!(Instant::now() < end, "{what}: kam nicht");
        each();
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
#[ignore = "startet und beendet echte Prozesse; ausdrücklich mit --ignored laufen lassen"]
fn orphans_of_a_closed_agent_shell_are_found_and_killed_and_nothing_else_is_touched() {
    let tmp = tempfile::tempdir().unwrap();
    let dir: PathBuf = tmp.path().join("procs");
    let state = tmp.path().join("state");
    for d in [&dir, &state] {
        fs::create_dir_all(d).unwrap();
    }
    for (name, code) in [("idle.js", NODE_IDLE), ("server.js", NODE_SERVER), ("spawner.js", NODE_SPAWNER), ("agent-sim.js", NODE_AGENT)] {
        fs::write(dir.join(name), code).unwrap();
    }
    let head = tmp.path().join("agent_head.ps1");
    fs::write(&head, agent_head()).unwrap();
    let spec = deck_core::wt::launch_spec(SLOT, &state.to_string_lossy(), &[]);

    let mut cleanup = Cleanup { dir: dir.clone(), children: Vec::new() };
    cleanup.children.push(hidden(Command::new("node").arg(dir.join("idle.js")).arg(&dir).arg("u")).expect("node startet"));
    let shell = hidden(Command::new("pwsh").args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded(&shell_script(&dir, &head, &spec))]))
        .expect("pwsh startet");
    let shell_pid = shell.id();
    cleanup.children.push(shell);

    let src = Arc::new(Guarded::default());
    let watch = OrphanWatch::new(src.clone(), tmp.path(), &state);
    // Derselbe Agent, aber ohne Marken: nur die Eltern-Kette (so war es vorher).
    let src_blind = Arc::new(Guarded { blind: true, ..Guarded::default() });
    let blind_dir = tmp.path().join("blind");
    fs::create_dir_all(&blind_dir).unwrap();
    let watch_blind = OrphanWatch::new(src_blind.clone(), &blind_dir, &state);
    let mut scan_ms = Vec::new();
    let mut scan = || {
        let t = Instant::now();
        watch.scan(now());
        scan_ms.push(t.elapsed().as_secs_f64() * 1000.0);
        watch_blind.scan(now());
    };

    // Wie im Betrieb: hinschauen, solange der Agent lebt (hier alle 100 bis 300 ms statt
    // 2 s, damit der Test nicht trödelt; die Regeln sind dieselben).
    let names = ["a", "mid", "grand", "c", "d", "u", "sim", "grand2"];
    wait_for(
        "Testprozesse",
        30,
        || names.iter().all(|n| dir.join(format!("{n}.pid")).exists()) && dir.join("a.port").exists() && proc_of(&dir, "mid").is_none(),
        &mut scan,
    );
    for _ in 0..3 {
        scan();
        std::thread::sleep(Duration::from_millis(300));
    }
    let ids = read_shells(&state);
    println!("Shell laut .wtpid: {ids:?}");
    let shell_id = ids.first().cloned().expect("Shell aus der .wtpid");
    let sim = proc_of(&dir, "sim").unwrap();
    assert_eq!(src.real.marker(sim.0, sim.1), Some((shell_id.pid, shell_id.created)), "die Marke aus wt_agent.ps1 steht in der Umgebung");

    // Der Fall, an dem die Eltern-Kette scheitert: Werkzeug-Shells, die einen Server
    // absetzen und sofort enden. Solange sie leben, schaut niemand hin.
    fs::write(dir.join("go"), "").unwrap();
    wait_for("(e) und (f)", 30, || ["e_mid.exit", "f_mid.exit", "e.port", "f.port"].iter().all(|f| dir.join(f).exists()), || {});
    let lived = |n: &str| fs::read_to_string(dir.join(format!("{n}.exit"))).unwrap_or_default();
    println!("Zwischenglieder lebten: pwsh (e) {} ms, cmd (f) {} ms; dabei kein Schnappschuss", lived("e_mid"), lived("f_mid"));
    for n in ["e_mid", "f_mid"] {
        assert!(proc_of(&dir, n).is_none(), "({n}) ist schon weg");
    }
    scan();

    let keys: HashMap<&str, Key> =
        ["a", "grand", "c", "d", "u", "grand2", "sim", "e", "f"].iter().map(|n| (*n, proc_of(&dir, n).unwrap_or_else(|| panic!("{n} läuft")))).collect();
    let port = |n: &str| -> u16 { fs::read_to_string(dir.join(format!("{n}.port"))).unwrap().trim().parse().unwrap() };
    let ports: HashMap<&str, u16> = ["a", "e", "f"].iter().map(|n| (*n, port(n))).collect();
    for n in ["e", "f"] {
        let k = keys[n];
        println!("({n}) PID {}: Marke {:?}", k.0, src.real.marker(k.0, k.1));
    }
    assert!(watch.rows(now(), Lang::German).is_empty(), "Fenster offen: keine Waisen");
    let shell_key = watch.tracker().live_shells().first().copied().expect("Shell aus der .wtpid erkannt");
    assert_eq!(shell_pid, shell_key.0, "pwsh selbst ist die Shell der .wtpid");
    *src.allowed.lock().unwrap() = keys.values().copied().collect();

    // „Fenster zu“: die Shell endet. Der Dienst wacht sofort auf und schaut nach.
    let t = Instant::now();
    let mut shell = cleanup.children.pop().unwrap();
    shell.kill().unwrap();
    let _ = shell.wait();
    watch.wait(Duration::from_secs(5));
    let woke_ms = t.elapsed().as_millis();
    assert!(woke_ms < 1500, "Ende der Shell sofort bemerkt ({woke_ms} ms)");
    scan();

    let rows = watch.rows(now(), Lang::German);
    println!("Zeilen nach dem Schließen:");
    for r in &rows {
        println!("  {:<9} {:<6} {:<36} {}  [{}]", r.port_text, r.program, r.command, r.origin, r.id);
    }
    let row_of = |k: Key| rows.iter().find(|r| parse_row_id(&r.id) == Some(k));
    let expected = ["a", "grand", "c", "d", "grand2", "sim", "e", "f"];
    for name in expected {
        let r = row_of(keys[name]).unwrap_or_else(|| panic!("({name}) fehlt in den Waisen"));
        assert!(r.origin.starts_with(&format!("{SLOT} · ")), "{}", r.origin);
    }
    for name in ["a", "e", "f"] {
        assert_eq!(row_of(keys[name]).unwrap().ports, vec![ports[name]], "({name}) mit Port");
    }
    assert_eq!(row_of(keys["c"]).unwrap().port_text, "kein Port");
    assert!(row_of(keys["u"]).is_none(), "fremder node steht nicht in der Liste");
    assert!(watch.tracker().tracked().all(|t| t.key() != keys["u"]), "fremder node wird nicht einmal gemerkt");
    let blind = watch_blind.rows(now(), Lang::German);
    let blind_found: Vec<&str> = ["e", "f"].into_iter().filter(|n| blind.iter().any(|r| parse_row_id(&r.id) == Some(keys[n]))).collect();
    println!("Ohne Marke (nur Eltern-Kette) erkannt von (e), (f): {blind_found:?}");
    assert!(blind_found.is_empty(), "ohne Marke findet die Eltern-Kette (e) und (f) nicht; sonst prüft der Test nichts");

    // Beenden über den Weg der Oberfläche, Zeile für Zeile.
    for name in expected {
        watch.kill(&row_id(keys[name]), now(), Lang::German).unwrap_or_else(|e| panic!("({name}) nicht beendet: {e}"));
    }
    for name in expected {
        let (pid, start) = keys[name];
        assert!(!src.real.running(pid, start), "({name}) läuft noch");
    }
    for (name, p) in &ports {
        assert!(std::net::TcpStream::connect_timeout(&([127, 0, 0, 1], *p).into(), Duration::from_millis(300)).is_err(), "({name}) Port {p} ist frei");
        assert!(src.real.listening().unwrap().iter().all(|(_, q)| q != p));
    }
    assert!(src.real.running(keys["u"].0, keys["u"].1), "fremder node läuft weiter");
    assert!(src.violations.lock().unwrap().is_empty(), "nur Testprozesse beendet: {:?}", src.violations.lock().unwrap());
    assert!(src_blind.terminated.lock().unwrap().is_empty());
    scan();
    let left: Vec<String> = watch.rows(now(), Lang::German).iter().map(|r| r.id.clone()).collect();
    println!("Nach dem Beenden übrig: {left:?}");
    assert!(left.is_empty(), "nichts bleibt übrig");

    let avg = scan_ms.iter().sum::<f64>() / scan_ms.len() as f64;
    let max = scan_ms.iter().cloned().fold(0.0, f64::max);
    println!("Scan: {} Läufe, Mittel {avg:.1} ms, Maximum {max:.1} ms; Shell-Ende bemerkt nach {woke_ms} ms", scan_ms.len());
    println!("Beendet über den Dienst: {} Prozesse", src.terminated.lock().unwrap().len());
}
