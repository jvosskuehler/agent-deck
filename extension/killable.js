// Welche Prozesse duerfen zum Abschiessen angeboten werden? - reine Auswahl-Logik
// OHNE vscode/child_process, damit sie sich mit Node allein pruefen laesst
// (tests/test_extension_killable.py fuettert sie ueber node).
//
// Der Zuschnitt ist bewusst eng. Unter EINEM VS-Code-Fenster haengen schnell 100
// Prozesse; angeboten wird davon nur, was tatsaechlich einen TCP-Port haelt - denn
// nur das steht dem naechsten `npm run dev` im Weg. Alles andere waere eine Liste,
// in der man den einen Blocker sucht.
//
// WICHTIGER als das Finden ist das NICHT-Finden: in diesem Prozessbaum laufen die
// Claude-Sessions des Decks. Ein unbedachter Mehrfach-Kill raeumte die eigenen
// Agenten ab, und das Panel zeigte danach stumme Kacheln. Darum eine harte
// Sperrliste, die ausserdem dokumentiert, WARUM etwas geschuetzt ist - die Gruende
// stehen in `keepReason` und wandern in den Test.

const detect = require("./detect");

// Die Abfrage, aus der die Kandidaten kommen. Sie steht HIER und nicht bei ihrem
// Aufrufer, damit ein Test genau das Snippet gegen eine echte Maschine laufen
// lassen kann, das auch produktiv laeuft - eine Kopie im Test bewiese nur sich
// selbst. Ausgefuehrt wird sie in extension.js (dieses Modul bleibt frei von
// child_process, sonst liesse es sich nicht mit Node allein pruefen).
//
// Prozesse UND Listener in EINEM Lauf: zwei Laeufe waeren zwei Zeitpunkte, und ein
// Prozess, der dazwischen stirbt, stuende mit Port aber ohne Namen da.
// Bewusst OHNE " im Snippet - Windows PowerShell 5.1 entfernt die beim Weiterreichen
// an ein natives Programm (siehe CLAUDE.md, "Fallen").
const PS_SCAN = [
  // Ohne diese Zeile schreibt Windows PowerShell 5.1 in der Konsolen-Codepage, und
  // Node liest UTF-8: aus `JorritVosskuehler` wird Mojibake, sichtbar in der
  // Kommandozeile im Auswahldialog. Dieselbe Sorte Fehler wie bei den Hooks, nur in
  // der Gegenrichtung (CLAUDE.md, Falle 5).
  "[Console]::OutputEncoding=[Text.Encoding]::UTF8;",
  "$p = Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,Name,CommandLine,SessionId,",
  "@{n='Start';e={if($_.CreationDate){$_.CreationDate.ToString('o')}}};",
  "$l = @(Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue |",
  "Select-Object LocalPort,OwningProcess);",
  "[pscustomobject]@{procs=@($p);listen=$l} | ConvertTo-Json -Compress -Depth 4",
].join(" ");

// Prozesse, die nie zur Auswahl stehen. Nach Name, mit Begruendung.
const KEEP = [
  // Zuerst die toedlichen: `lsass.exe` zu beenden ist ein sofortiger Bluescreen,
  // `services.exe`/`wininit.exe` ebenso. Sie halten Ports (RPC, SMB) und standen in
  // der ersten Fassung mit in der weiten Liste. Die Sitzungs-Regel unten faengt sie
  // ebenfalls - diese Namen sind der Guertel dazu, falls die SessionId mal fehlt.
  [/^(System|Idle|smss|csrss|wininit|winlogon|services|lsass|LsaIso|svchost|spoolsv|dwm|fontdrvhost)(\.exe)?$/i,
    "Windows-Kernprozess"],
  [/^Code( - Insiders)?\.exe$/i, "VS Code selbst"],
  [/^Microsoft\.VisualStudio\.Code\./i, "VS-Code-Dienst"],
  [/^conhost\.exe$/i, "Konsolenwirt eines Terminals"],
  [/^OpenConsole\.exe$/i, "Konsolenwirt eines Terminals"],
  // Die Shells sind die Terminals selbst: sie zu beenden schliesst den Pane (und
  // damit eine evtl. darin laufende Claude-Session), gibt aber keinen Port frei -
  // den haelt das Kind. Genau darum steht hier das Kind zur Auswahl, nicht die Shell.
  [/^(pwsh|powershell|cmd|bash|sh|zsh|wsl|ssh)\.exe$/i, "Shell eines Terminals"],
];

// Kommandozeilen, die auf das Deck selbst zeigen (Panel, Watchdog, Hooks). Die
// laufen zwar meist ausserhalb von VS Code, koennen aber aus einem Terminal
// gestartet worden sein - dann haengen sie mit im Baum.
const DECK_CMD = /agent[-_]deck|agent_deck\.py|watchdog\.py|statusline\.py|report\.py/i;

// Claude Code laeuft nicht immer als claude.exe: je nach Installation ist es ein
// node-Prozess auf `...\claude\cli.js`. detect.isClaudeProc sieht das nicht (es
// verlangt claude als BEFEHL), und hier waere das Uebersehen teuer - der Nutzer
// killte seine eigene Session. Darum zusaetzlich ein Blick auf den Pfad, aber eng
// am Ordnernamen: `\claude\` und `\.claude\` treffen, `claude-experiments\` nicht.
const CLAUDE_PATH = /[\\/]\.?claude[\\/]/i;

function num(v) {
  const n = Number(v);
  return Number.isFinite(n) ? n : null;
}

// Feldnamen vereinheitlichen: PowerShell liefert Gross-, Tests gern Kleinschreibung.
function pid(p) { return num(p && (p.ProcessId != null ? p.ProcessId : p.processId)); }
function ppid(p) { return num(p && (p.ParentProcessId != null ? p.ParentProcessId : p.parentProcessId)); }
function pname(p) { return String((p && (p.Name != null ? p.Name : p.name)) || ""); }
function pcmd(p) { return String((p && (p.CommandLine != null ? p.CommandLine : p.commandLine)) || ""); }
function pstart(p) { return (p && (p.Start != null ? p.Start : p.start)) || null; }
function psession(p) { return num(p && (p.SessionId != null ? p.SessionId : p.sessionId)); }

/** Warum ist dieser Prozess geschuetzt? null = er darf angeboten werden. */
function keepReason(p, opts) {
  const o = opts || {};
  // Claude zuerst: das sind die Agenten des Decks. Deckt auch den Fall ab, dass
  // Claude unter einem anderen Namen laeuft (npx/node mit claude in der Cmdline).
  if (detect.isClaudeProc(p)) return "Claude-Session";
  // Sitzung 0 ist Windows' Dienste-Sitzung: dort laeuft nichts, was ein Entwickler
  // startet. Gemessen trennt das sauber - lsass/svchost/spoolsv/TeamViewer stehen
  // in 0, waehrend Docker, WSL und die eigenen Dev-Server in Sitzung 1 liegen. Ein
  // allgemeines Kriterium ist hier mehr wert als eine Namensliste, die nie
  // vollstaendig wird. Nur eine ECHTE 0 zaehlt: ein fehlendes Feld darf nicht
  // versehentlich alles schuetzen.
  if (psession(p) === 0) return "Systemdienst (Sitzung 0)";
  const name = pname(p);
  for (const [re, grund] of KEEP) if (re.test(name)) return grund;
  const cmd = pcmd(p);
  if (CLAUDE_PATH.test(cmd)) return "Claude-Session (per Pfad erkannt)";
  if (DECK_CMD.test(cmd)) return "Agent Deck selbst";
  if (o.selfPid && pid(p) === num(o.selfPid)) return "dieses VS-Code-Fenster";
  return null;
}

/**
 * Alle PIDs im Prozessbaum unter (und einschliesslich) den Wurzeln.
 * Zyklenfest ueber `seen` - eine kaputte ParentProcessId darf nicht haengen lassen.
 */
function descendantPids(rootPids, procs) {
  const out = new Set();
  if (!Array.isArray(procs) || !procs.length) return out;
  const kids = new Map();
  for (const p of procs) {
    const pp = ppid(p);
    if (pp == null) continue;
    if (!kids.has(pp)) kids.set(pp, []);
    kids.get(pp).push(p);
  }
  const stack = [];
  for (const r of rootPids || []) {
    const n = num(r);
    if (n != null) stack.push(n);
  }
  while (stack.length) {
    const cur = stack.pop();
    if (out.has(cur)) continue;
    out.add(cur);
    for (const child of kids.get(cur) || []) {
      const c = pid(child);
      if (c != null && !out.has(c)) stack.push(c);
    }
  }
  return out;
}

/** Die PIDs der VS-Code-Hauptprozesse (mehrere Fenster = mehrere Wurzeln). */
function vscodeRoots(procs) {
  const out = [];
  for (const p of procs || []) {
    if (/^Code( - Insiders)?\.exe$/i.test(pname(p))) {
      const n = pid(p);
      if (n != null) out.push(n);
    }
  }
  return out;
}

/**
 * Die Kandidaten-Liste: Prozesse, die einen TCP-Port halten - standardmaessig nur
 * die im Baum unter VS Code.
 *
 * scan   = { procs: [Win32_Process...], listen: [{LocalPort, OwningProcess}...] }
 * opts   = { all: auch ausserhalb von VS Code,
 *            skipPorts: Ports, die nie angeboten werden (Broker!),
 *            selfPid: PID dieses Extension-Hosts }
 *
 * Rueckgabe je Prozess EIN Eintrag mit ALLEN seinen Ports - ein Dev-Server, der auf
 * v4 und v6 lauscht, ist ein Prozess und soll nicht zweimal in der Liste stehen.
 */
function portBlockers(scan, opts) {
  const o = opts || {};
  const procs = (scan && scan.procs) || [];
  const listen = (scan && scan.listen) || [];
  if (!procs.length || !listen.length) return [];

  const skip = new Set((o.skipPorts || []).map(Number));
  const byPid = new Map();
  for (const p of procs) {
    const n = pid(p);
    if (n != null && !byPid.has(n)) byPid.set(n, p);
  }
  const inTree = o.all ? null : descendantPids(vscodeRoots(procs), procs);

  const found = new Map(); // pid -> Eintrag
  for (const l of listen) {
    const owner = num(l && (l.OwningProcess != null ? l.OwningProcess : l.owningProcess));
    const port = num(l && (l.LocalPort != null ? l.LocalPort : l.localPort));
    if (owner == null || port == null) continue;
    if (skip.has(port)) continue;             // Broker-Port: nie anbieten
    if (owner <= 4) continue;                 // System/Idle halten die SMB-Ports
    if (inTree && !inTree.has(owner)) continue;
    const p = byPid.get(owner);
    if (!p) continue;                         // Prozess zwischen den zwei Abfragen weg
    if (keepReason(p, o)) continue;

    let e = found.get(owner);
    if (!e) {
      e = { pid: owner, name: pname(p), cmd: pcmd(p), start: pstart(p), ports: [] };
      found.set(owner, e);
    }
    if (!e.ports.includes(port)) e.ports.push(port);
  }

  const out = [...found.values()];
  for (const e of out) e.ports.sort((a, b) => a - b);
  // Nach kleinstem Port sortieren: der Nutzer sucht "3001", nicht "node.exe".
  out.sort((a, b) => (a.ports[0] || 0) - (b.ports[0] || 0) || a.pid - b.pid);
  return out;
}

/** Kommandozeile auf Anzeigelaenge bringen, ohne den Anfang zu verlieren. */
function shortCmd(cmd, max) {
  const s = String(cmd || "").replace(/\s+/g, " ").trim();
  const n = max || 120;
  return s.length <= n ? s : s.slice(0, n - 1) + "…";
}

module.exports = {
  PS_SCAN, KEEP, DECK_CMD, CLAUDE_PATH,
  keepReason, descendantPids, vscodeRoots, portBlockers, shortCmd,
};
