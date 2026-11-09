// Die Verdrahtung in extension.js, einmal durch: aktivieren, beim Wegwerf-Broker
// melden, ein Kommando empfangen, ein Terminal anlegen, Schliessen melden.
//
// Mit einer vscode-Attrappe als REMOTE-Fenster: so startet die Erkennung keinen
// PowerShell-Scan und schreibt keine pidmap. LOCALAPPDATA zeigt trotzdem in einen
// Wegwerf-Ordner, denn deactivate() raeumt die pidmap des Fensters weg, und eine echte
// pidmap-A.json gehoert womoeglich einem offenen VS Code. Kein Port 8765.

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("fs");
const os = require("os");
const path = require("path");
const net = require("net");
const Module = require("module");

function fakeVscode(port) {
  const on = { open: [], close: [], active: [] };
  const sub = (list) => (f) => { list.push(f); return { dispose() { list.splice(list.indexOf(f), 1); } }; };
  const terminals = [];
  const commands = {};
  const config = { port, window: "A", autostartCommand: "claude" };
  const vscode = {
    env: { remoteName: "ssh-remote" },
    workspace: {
      getConfiguration: () => ({ get: (k, d) => (k in config ? config[k] : d) }),
      workspaceFolders: [{ name: "proj", uri: { scheme: "vscode-remote", authority: "ssh-remote+box",
        toString: () => "vscode-remote://ssh-remote+box/home/x/proj" } }],
      workspaceFile: undefined,
    },
    window: {
      terminals,
      createTerminal(opts) {
        const t = { name: opts.name, opts, log: [], processId: Promise.resolve(undefined),
          show() { this.log.push("show"); }, sendText(s, ex) { this.log.push([s, ex]); }, dispose() {} };
        terminals.push(t);
        on.open.forEach((f) => f(t));
        return t;
      },
      onDidOpenTerminal: sub(on.open),
      onDidCloseTerminal: sub(on.close),
      onDidChangeActiveTerminal: sub(on.active),
      showWarningMessage() {}, showInformationMessage() {}, showErrorMessage() {},
    },
    commands: {
      registerCommand: (id, fn) => { commands[id] = fn; return { dispose() {} }; },
      executeCommand: async () => {},
    },
  };
  return { vscode, on, terminals, commands };
}

// Zeilen, die der Wegwerf-Broker empfaengt, mit Warten auf eine bestimmte.
function lineReader(sock) {
  const lines = [];
  const waiters = [];
  let buf = "";
  sock.setEncoding("utf8");
  sock.on("data", (d) => {
    buf += d;
    let i;
    while ((i = buf.indexOf("\n")) >= 0) {
      lines.push(JSON.parse(buf.slice(0, i)));
      buf = buf.slice(i + 1);
    }
    for (const w of [...waiters]) w();
  });
  return (pred) => new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`nicht empfangen: ${JSON.stringify(lines)}`)), 5000);
    const check = () => {
      const hit = lines.find(pred);
      if (!hit) return;
      lines.splice(lines.indexOf(hit), 1);
      waiters.splice(waiters.indexOf(check), 1);
      clearTimeout(timer);
      resolve(hit);
    };
    waiters.push(check);
    check();
  });
}

test("the extension reports, creates an agent on command and reports its close", async (t) => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "deck-ext-"));
  const oldLocal = process.env.LOCALAPPDATA;
  process.env.LOCALAPPDATA = tmp;
  const server = net.createServer();
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  const fake = fakeVscode(server.address().port);
  const origLoad = Module._load;
  Module._load = function (req, ...rest) { return req === "vscode" ? fake.vscode : origLoad.call(this, req, ...rest); };

  const ext = require("../extension");
  const ctx = { subscriptions: [] };
  t.after(() => {
    ext.deactivate();
    ctx.subscriptions.forEach((s) => s.dispose());
    server.close();
    Module._load = origLoad;
    process.env.LOCALAPPDATA = oldLocal;
    fs.rmSync(tmp, { recursive: true, force: true });
  });

  const conn = new Promise((r) => server.once("connection", r));
  ext.activate(ctx);
  const sock = await conn;
  const next = lineReader(sock);

  const hello = await next((m) => m.type === "hello");
  assert.deepEqual(hello, { type: "hello", workspace: "proj [SSH: box]",
    folder: "vscode-remote://ssh-remote+box/home/x/proj", window: "A", remote: "ssh-remote", slots: [] });
  assert.deepEqual(Object.keys(fake.commands).sort(),
    ["agentDeck.createAgent", "agentDeck.createAgents", "agentDeck.killPortBlockers"]);

  sock.write(JSON.stringify({ cmd: "createAgent", model: "opus", cwd: "/home/x/proj" }) + "\n");
  const after = await next((m) => m.type === "terminals" && m.slots.length === 1);
  assert.deepEqual(after.slots, ["A1"]);
  const term = fake.terminals[0];
  assert.deepEqual(term.opts.env, { AGENT_SLOT: "A1" });
  // claude startet erst nach der Pause fuer den pty (600 ms).
  for (let i = 0; i < 40 && term.log.length < 2; i++) await new Promise((r) => setTimeout(r, 50));
  assert.deepEqual(term.log, ["show", ['claude --model "opus"', true]]);

  // Pane in VS Code angeklickt -> "seen" fuer genau diesen Slot.
  fake.on.active.forEach((f) => f(term));
  assert.deepEqual(await next((m) => m.type === "seen"), { type: "seen", window: "A", slot: "A1" });

  fake.terminals.splice(0, 1);
  fake.on.close.forEach((f) => f(term));
  const closed = await next((m) => m.type === "terminals" && m.slots.length === 0);
  assert.equal(closed.remote, "ssh-remote");
  assert.deepEqual(fs.readdirSync(tmp), [], "ein Remote-Fenster schreibt keine pidmap");
});
