// Die Claude-Erkennung (detect.js): welches Terminal eine Kachel bekommt, und welche
// Kachel verschwindet - und welche NIE von allein.
//
// `deadSlots` steht zwischen zwei Fehlern. Der eine ist die LEICHE: ein geschlossenes
// Terminal, das nach einer langen await-Pause (Prozess-Scan bis 6 s, Split bis 3 s)
// wieder in die Map geraet; zu ihm feuert kein onDidCloseTerminal mehr. Der andere
// ist der teurere: eine Kachel abraeumen, deren Chat noch laeuft. Darum raeumt die
// Regel nur ab, was VS Code selbst nicht mehr fuehrt, und bei ausbleibender Auskunft
// gar nichts. Portiert aus `git show 2856494:tests/test_extension_slots.py`.

const test = require("node:test");
const assert = require("node:assert/strict");
const d = require("../detect");

// Terminal-Objekte sind fuer die Regel undurchsichtige Referenzen - sie werden nur
// verglichen, nie gelesen. Stellvertreter sind darum schlichte Zeichenketten.
const T1 = "term-1", T2 = "term-2", T3 = "term-3";

// ── Die Leiche ──────────────────────────────────────────────────────────────

test("a closed terminal frees its slot", () => {
  assert.deepEqual(d.deadSlots([["A1", T1], ["A2", T2]], [T1]), ["A2"]);
});

test("a window without terminals frees all slots", () => {
  // Das leere Array ist eine ECHTE Auskunft (alles zu), kein Ausfall.
  assert.deepEqual(d.deadSlots([["A1", T1], ["A2", T2]], []), ["A1", "A2"]);
});

test("two slots on the same dead terminal both fall", () => {
  assert.deepEqual(d.deadSlots([["A1", T1], ["A2", T1]], [T2]), ["A1", "A2"]);
});

// ── Der teurere Gegenfehler ─────────────────────────────────────────────────

test("running chats are never cleared", () => {
  assert.deepEqual(d.deadSlots([["A1", T1], ["A2", T2], ["A3", T3]], [T1, T2, T3]), []);
});

test("a terminal without a slot does not disturb", () => {
  // In VS Code steht auch, was dem Deck nicht gehoert (eine gewoehnliche Shell).
  assert.deepEqual(d.deadSlots([["A1", T1]], [T1, T2]), []);
});

test("without an answer nothing is cleared", () => {
  // Kommt statt der Terminalliste nichts Brauchbares, ist das kein "alle tot":
  // lieber eine tote Kachel stehen lassen als eine lebende loeschen.
  assert.deepEqual(d.deadSlots([["A1", T1], ["A2", T2]], null), []);
  assert.deepEqual(d.deadSlots([["A1", T1]], undefined), []);
});

test("an empty book is not an error", () => {
  assert.deepEqual(d.deadSlots([], [T1]), []);
  assert.deepEqual(d.deadSlots(null, [T1]), []);
});

// ── Ist das Claude? ─────────────────────────────────────────────────────────

test("claude counts as a command with path, extension and in a pipeline", () => {
  for (const cmd of ["claude", "claude --resume", "C:\\bin\\claude.exe -c", "./claude.ps1", "cd x; claude", "x && claude"]) {
    assert.ok(d.isClaudeCommand(cmd), cmd);
  }
});

test("claude installed globally with npm counts as the command", () => {
  const cmd = 'node "C:\\Users\\x\\AppData\\Roaming\\npm\\node_modules\\@anthropic-ai\\claude-code\\cli.js" --resume';
  assert.ok(d.isClaudeCommand(cmd), cmd);
});

test("words that only contain claude are not the command", () => {
  for (const cmd of ["claudette", "claude-foo", "node C:\\dev\\claude-experiments\\s.js",
    "C:\\Users\\x\\AppData\\Local\\claude-agent-deck\\bin\\deck-report.exe", "", null]) {
    assert.ok(!d.isClaudeCommand(cmd), String(cmd));
  }
});

test("a process is claude by its name or by its command", () => {
  assert.ok(d.isClaudeProc({ Name: "claude.exe" }));
  assert.ok(d.isClaudeProc({ Name: "node.exe", CommandLine: "npx claude" }));
  assert.ok(!d.isClaudeProc({ Name: "node.exe", CommandLine: "node server.js" }));
  assert.ok(!d.isClaudeProc(null));
});

test("deck tiles and terminals named claude look like claude", () => {
  assert.ok(d.looksLikeClaudeName("A3", "A"));
  assert.ok(d.looksLikeClaudeName("Claude Code", "A"));
  assert.ok(!d.looksLikeClaudeName("B3", "A"), "fremdes Fenster");
  assert.ok(!d.looksLikeClaudeName("pwsh", "A"));
});

// ── Prozessbaum ─────────────────────────────────────────────────────────────

const tree = [
  { ProcessId: 10, ParentProcessId: 1, Name: "pwsh.exe" },
  { ProcessId: 11, ParentProcessId: 10, Name: "node.exe", CommandLine: "node build.js" },
  { ProcessId: 12, ParentProcessId: 11, Name: "claude.exe" },
  { ProcessId: 20, ParentProcessId: 1, Name: "pwsh.exe" },
];

test("claude anywhere below the shell is found, with its pid", () => {
  assert.ok(d.hasClaudeDescendant(10, tree));
  assert.equal(d.claudeDescendantPid(10, tree), 12);
});

test("a shell without claude below has none", () => {
  assert.ok(!d.hasClaudeDescendant(20, tree));
  assert.equal(d.claudeDescendantPid(20, tree), null);
  assert.equal(d.claudeDescendantPid(10, []), null);
});

test("the nearest claude wins, so the pidmap names the session itself", () => {
  // Claude startet Unterprozesse, die wieder claude heissen koennen (Subagenten);
  // die Hooks haengen unter der Session direkt unter der Shell.
  const nested = [...tree, { ProcessId: 13, ParentProcessId: 12, Name: "claude.exe" }];
  assert.equal(d.claudeDescendantPid(10, nested), 12);
});

test("a cycle in the parent chain does not hang the search", () => {
  const loop = [{ ProcessId: 1, ParentProcessId: 2, Name: "a" }, { ProcessId: 2, ParentProcessId: 1, Name: "b" }];
  assert.ok(!d.hasClaudeDescendant(1, loop));
  assert.equal(d.claudeDescendantPid(1, loop), null);
});

// ── Kleinkram ───────────────────────────────────────────────────────────────

test("PowerShell JSON always becomes a list", () => {
  assert.deepEqual(d.parseProcList('[{"a":1}]'), [{ a: 1 }]);
  assert.deepEqual(d.parseProcList('{"a":1}'), [{ a: 1 }]);
  assert.deepEqual(d.parseProcList(""), []);
  assert.deepEqual(d.parseProcList("kaputt"), []);
});

test("the next index follows the highest of this window", () => {
  assert.equal(d.nextIndex(["A1", "A3", "B7"], "A"), 4);
  assert.equal(d.nextIndex([], "A"), 1);
});
