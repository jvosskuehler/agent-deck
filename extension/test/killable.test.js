// Wen der Abraeum-Befehl (Ctrl+Alt+K) anbieten darf - und wen NIE.
//
// Die Regeln entscheiden, ob ein Klick einen haengengebliebenen Dev-Server beendet
// oder die eigene Claude-Session, und diese Frage darf nicht am Handbetrieb haengen.
// Der Schwerpunkt liegt auf dem NICHT-Anbieten: unter einem VS-Code-Fenster haengen
// schnell 100 Prozesse, darunter die Agenten des Decks. Ein zu enger Filter kostet
// nur einen Blick in den Task-Manager. Portiert aus der Python-Suite
// (`git show 2856494:tests/test_extension_killable.py`), dazu die Programme des Decks.
//
// Reine Logik: hier wird nichts gescannt und nichts beendet.

const test = require("node:test");
const assert = require("node:assert/strict");
const k = require("../killable");

// sess=1 ist die Nutzersitzung - dort laeuft alles, was ein Entwickler startet.
function proc(pid, ppid, name, cmd = "", start = null, sess = 1) {
  return { ProcessId: pid, ParentProcessId: ppid, Name: name, CommandLine: cmd, Start: start, SessionId: sess };
}
const listener = (port, pid) => ({ LocalPort: port, OwningProcess: pid });
const pids = (list) => list.map((e) => e.pid);

// Ein VS-Code-Fenster, wie es wirklich aussieht: die Shell des Terminals, darunter
// der Dev-Server, daneben die Claude-Session. Dazu zwei Prozesse AUSSERHALB des
// Baums - ein fremder Server und das Deck selbst auf dem Broker-Port.
const CODE = 100, SHELL = 200;
function scene() {
  return {
    procs: [
      proc(1, 0, "explorer.exe"),
      proc(CODE, 1, "Code.exe"),
      proc(SHELL, CODE, "pwsh.exe"),
      proc(300, SHELL, "node.exe", "node C:\\proj\\fe\\server.js", "2026-08-03T09:09:16"),
      proc(400, SHELL, "claude.exe", "claude --model opus"),
      proc(500, SHELL, "dotnet.exe", "dotnet run --project C:\\proj\\be"),
      proc(600, 1, "node.exe", "node C:\\anderes\\ding.js"),
      proc(700, 1, "agent-deck.exe", "C:\\Users\\x\\AppData\\Local\\claude-agent-deck\\bin\\agent-deck.exe"),
    ],
    listen: [
      listener(3001, 300),
      listener(5000, 500), listener(5000, 500), listener(5001, 500), // v4 + v6 + zweiter Port
      listener(4200, 600),
      listener(8765, 700),
      listener(445, 4),                                              // System haelt SMB
    ],
  };
}
const OPTS = { skipPorts: [8765] };
const ALL = { ...OPTS, all: true };

// ── Was angeboten wird ──────────────────────────────────────────────────────

test("a dev server under VS Code is offered", () => {
  assert.ok(pids(k.portBlockers(scene(), OPTS)).includes(300));
});

test("a process with several ports is listed once with all its ports", () => {
  const hits = k.portBlockers(scene(), OPTS).filter((e) => e.pid === 500);
  assert.equal(hits.length, 1);
  assert.deepEqual(hits[0].ports, [5000, 5001]);
});

test("the list is sorted by the lowest port", () => {
  const ports = k.portBlockers(scene(), ALL).map((e) => e.ports[0]);
  assert.deepEqual(ports, [...ports].sort((a, b) => a - b));
});

test("a dev server in the user session stays killable", () => {
  // Die Gegenprobe: die Sitzungs-Regel darf nicht die ganze Liste leerraeumen.
  assert.equal(k.keepReason(proc(804, 1, "node.exe", "node server.js"), {}), null);
});

// ── Claude: niemals ─────────────────────────────────────────────────────────

test("a Claude session is never offered, even when it holds a port", () => {
  const s = scene();
  s.listen.push(listener(9999, 400));
  assert.ok(!pids(k.portBlockers(s, ALL)).includes(400));
});

test("Claude is kept under every way it can be started", () => {
  for (const [name, cmd] of [
    ["claude.exe", ""],
    ["claude.cmd", ""],
    ["node.exe", "node C:\\Users\\x\\AppData\\Roaming\\npm\\claude.cmd --resume"],
    ["node.exe", "npx claude"],
    ["pwsh.exe", "pwsh -c claude"],
  ]) {
    const reason = k.keepReason(proc(810, SHELL, name, cmd), {});
    assert.ok(reason, `${name} ${cmd} muss geschuetzt sein`);
  }
});

test("Claude running as a node process is recognised by its path", () => {
  // Je nach Installation heisst der Prozess node.exe und laeuft auf cli.js. Das
  // Uebersehen waere teuer, darum greift hier der Pfad-Blick statt des Befehls.
  for (const cmd of [
    "node C:\\Users\\x\\.claude\\local\\cli.js",
    // Globale npm-Installation, so startet sie der claude.cmd-Schnipsel.
    'node "C:\\Users\\x\\AppData\\Roaming\\npm\\node_modules\\@anthropic-ai\\claude-code\\cli.js" --resume',
    "node /home/x/.claude/local/cli.js",
  ]) {
    // Die npm-Installation erkennt schon detect.isClaudeCommand, die anderen erst der
    // Pfad-Blick; gesperrt sind alle, nur die Begründung unterscheidet sich.
    assert.match(k.keepReason(proc(800, SHELL, "node.exe", cmd), {}), /^Claude-Session/, cmd);
  }
});

test("a project with claude in its name stays killable", () => {
  // Die Gegenprobe zum Pfad-Blick: `claude-experiments` ist ein Projekt, kein
  // Claude. Waere es geschuetzt, liesse sich der Blocker nie abraeumen.
  const p = proc(801, SHELL, "node.exe", "node C:\\dev\\claude-experiments\\fe\\server.js");
  assert.equal(k.keepReason(p, {}), null);
});

// ── Sitzung 0 und Windows ───────────────────────────────────────────────────

test("services in session 0 are never offered", () => {
  // Gemessen am 2026-08-03: der weite Modus bot lsass.exe, services.exe und
  // wininit.exe an - alle halten Ports (RPC), und lsass zu beenden ist ein
  // sofortiger Bluescreen. Sitzung 0 trennt sie sauber von allem, was ein
  // Entwickler startet (Docker und WSL liegen gemessen in Sitzung 1).
  assert.equal(k.keepReason(proc(802, 1, "lsass.exe", "", null, 0), {}), "Systemdienst (Sitzung 0)");
  // Auch ein harmlos klingender Name in Sitzung 0 bleibt gesperrt.
  assert.equal(k.keepReason(proc(805, 1, "node.exe", "node server.js", null, 0), {}), "Systemdienst (Sitzung 0)");
});

test("session 0 as a string from PowerShell counts as session 0", () => {
  assert.equal(k.keepReason(proc(806, 1, "TeamViewer_Service.exe", "", null, "0"), {}), "Systemdienst (Sitzung 0)");
});

test("in the wide mode a session-0 listener is left out of the list", () => {
  const s = scene();
  s.procs.push(proc(950, 1, "svchost.exe", "", null, 0), proc(951, 1, "TeamViewer_Service.exe", "", null, 0));
  s.listen.push(listener(135, 950), listener(5939, 951));
  const got = pids(k.portBlockers(s, ALL));
  assert.ok(!got.includes(950) && !got.includes(951), String(got));
});

test("core processes stay locked even without a session field", () => {
  // Der Guertel zur Sitzungs-Regel: fehlt das Feld, darf lsass trotzdem nicht
  // in der Liste landen. Eine fehlende SessionId schuetzt bewusst NICHT alles.
  for (const name of ["lsass.exe", "services.exe", "wininit.exe", "csrss.exe", "svchost.exe", "System"]) {
    const p = proc(803, 1, name);
    delete p.SessionId;
    assert.equal(k.keepReason(p, {}), "Windows-Kernprozess", name);
  }
});

test("a missing session field does not protect an ordinary process", () => {
  const p = proc(807, 1, "node.exe", "node server.js");
  delete p.SessionId;
  assert.equal(k.keepReason(p, {}), null);
});

test("system and idle pids are left out", () => {
  assert.ok(!pids(k.portBlockers(scene(), ALL)).includes(4));
});

// ── Terminal und VS Code selbst ─────────────────────────────────────────────

test("the shell of a terminal is not offered", () => {
  // Sie zu beenden schliesst den Pane samt Session, gibt aber keinen Port frei.
  const s = scene();
  s.listen.push(listener(9998, SHELL));
  assert.ok(!pids(k.portBlockers(s, OPTS)).includes(SHELL));
});

test("VS Code and its console hosts are kept", () => {
  for (const name of ["Code.exe", "Code - Insiders.exe", "conhost.exe", "OpenConsole.exe"]) {
    assert.ok(k.keepReason(proc(820, 1, name), {}), name);
  }
});

test("this VS Code window is spared", () => {
  const s = scene();
  s.procs.push(proc(950, CODE, "node.exe", "extension host"));
  s.listen.push(listener(9229, 950));
  assert.ok(!pids(k.portBlockers(s, { ...OPTS, selfPid: 950 })).includes(950));
});

// ── Der Broker-Port und das Deck ────────────────────────────────────────────

test("the broker port is never offered", () => {
  // Ihn zu kappen sieht aus wie ein Absturz des Panels - mitten im Gebrauch.
  const s = scene();
  s.procs.push(proc(900, 1, "python.exe", "python -m http.server"));
  s.listen.push(listener(8765, 900));
  assert.ok(!pids(k.portBlockers(s, ALL)).includes(900));
});

test("the broker port owner is not offered through another port either", () => {
  const s = scene();
  s.procs.push(proc(901, 1, "deckhost.exe", "deckhost --serve"));
  s.listen.push(listener(8765, 901), listener(1420, 901));
  assert.ok(!pids(k.portBlockers(s, ALL)).includes(901));
});

test("the deck itself is not offered", () => {
  const s = scene();
  s.listen.push(listener(1421, 700)); // auch mit einem zweiten Port
  assert.ok(!pids(k.portBlockers(s, ALL)).includes(700));
});

test("every program of the deck is kept, by command line and by name", () => {
  const bin = "C:\\Users\\x\\AppData\\Local\\claude-agent-deck\\bin";
  for (const exe of ["agent-deck", "deck-watchdog", "deck-report", "deck-statusline", "deck-console"]) {
    assert.equal(k.keepReason(proc(830, 1, `${exe}.exe`, `${bin}\\${exe}.exe`), {}), "Agent Deck selbst", exe);
    // Ohne Kommandozeile (anderes Konto, erhoeht): der Name allein reicht.
    assert.equal(k.keepReason(proc(831, 1, `${exe}.exe`, ""), {}), "Agent Deck selbst", `${exe} ohne Kommandozeile`);
    // Aus einem Terminal gestartet, mit relativem Pfad.
    assert.equal(k.keepReason(proc(832, SHELL, `${exe}.exe`, `.\\target\\release\\${exe}.exe --x`), {}), "Agent Deck selbst");
  }
});

test("anything run from the claude-agent-deck folder is kept", () => {
  const helper = proc(834, 1, "node.exe", "node C:\\Users\\x\\AppData\\Local\\claude-agent-deck\\helper.js");
  assert.equal(k.keepReason(helper, {}), "Agent Deck selbst");
});

test("the deck programs never reach the list, not even in the wide mode", () => {
  const s = scene();
  const names = ["agent-deck", "deck-watchdog", "deck-report", "deck-statusline", "deck-console"];
  names.forEach((exe, i) => {
    s.procs.push(proc(1000 + i, SHELL, `${exe}.exe`, `${exe}.exe`));
    s.listen.push(listener(7000 + i, 1000 + i));
  });
  for (const opts of [OPTS, ALL]) {
    const got = pids(k.portBlockers(s, opts));
    assert.ok(names.every((_, i) => !got.includes(1000 + i)), String(got));
  }
});

// ── Der Zuschnitt: eng vs. weit ─────────────────────────────────────────────

test("without all everything outside VS Code stays out", () => {
  assert.deepEqual(pids(k.portBlockers(scene(), OPTS)), [300, 500]);
});

test("with all the foreign server joins but no protected process", () => {
  // 300 (:3001), 600 (:4200), 500 (:5000) - nach kleinstem Port sortiert.
  assert.deepEqual(pids(k.portBlockers(scene(), ALL)), [300, 600, 500]);
});

// ── Robustheit ──────────────────────────────────────────────────────────────

test("a listener without a process does not crash the list", () => {
  // Zwischen den zwei Abfragen kann ein Prozess sterben - dann steht ein Port
  // ohne Besitzer da. Er faellt weg, der Rest der Liste bleibt.
  const s = scene();
  s.listen.push(listener(7777, 4242));
  assert.deepEqual(pids(k.portBlockers(s, OPTS)), [300, 500]);
});

test("a cycle in the parent chain does not hang the tree walk", () => {
  // Eine ParentProcessId, die im Kreis zeigt, gibt es real (recycelte PIDs).
  const procs = [proc(10, 11, "a.exe"), proc(11, 10, "b.exe")];
  assert.deepEqual([...k.descendantPids([10], procs)].sort((a, b) => a - b), [10, 11]);
});

test("without listeners there is nothing to clear", () => {
  assert.deepEqual(k.portBlockers({ procs: scene().procs, listen: [] }, OPTS), []);
  assert.deepEqual(k.portBlockers(null, OPTS), []);
});

test("lower-case field names from tests or other sources work the same", () => {
  const s = {
    procs: [{ processId: 100, parentProcessId: 1, name: "Code.exe" },
            { processId: 300, parentProcessId: 100, name: "node.exe", commandLine: "node s.js", sessionId: 1 }],
    listen: [{ localPort: 3000, owningProcess: 300 }],
  };
  assert.deepEqual(pids(k.portBlockers(s, OPTS)), [300]);
});

test("a long command line is shortened without losing its start", () => {
  const long = "node " + "x".repeat(300);
  const short = k.shortCmd(long, 50);
  assert.equal(short.length, 50);
  assert.ok(short.startsWith("node x") && short.endsWith("…"));
  assert.equal(k.shortCmd("  a \n  b  "), "a b");
});

test("the scan query has no double quotes", () => {
  // Windows PowerShell 5.1 entfernt sie beim Weiterreichen an ein natives Programm.
  assert.ok(!k.PS_SCAN.includes('"'));
});
