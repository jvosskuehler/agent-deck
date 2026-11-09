// Kommando des Panels -> Wirkung (commands.js), mit Attrappen statt VS Code. Dazu der
// Abgleich mit dem Vertrag: jedes CMD_* aus crates/deck-core/src/protocol.rs muss
// hier eine Wirkung haben, sonst verpufft es still (die Extension kann die Rust-Datei
// nicht importieren und spiegelt die Strings von Hand).

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("fs");
const path = require("path");
const { handleMessage } = require("../commands");
const { SlotBook } = require("../slots");

function fakeTerm(name) {
  return {
    name, log: [], disposed: false,
    show(preserve) { this.log.push(["show", preserve]); },
    sendText(text, execute) { this.log.push(["send", text, execute]); },
    dispose() { this.disposed = true; },
  };
}

function rig() {
  const a1 = fakeTerm("A1"), a2 = fakeTerm("A2");
  const live = [a1, a2];
  const book = new SlotBook({ live: () => live });
  book.window = "A";
  book.ensure(a1, "A1");
  book.ensure(a2, "A2");
  const calls = [];
  const deck = {
    book,
    syncClaude: () => calls.push(["sync", book.window]),
    removePidMap: () => calls.push(["removePidMap", book.window]),
    createAgent: (m) => calls.push(["createAgent", m]),
    createAgents: (model) => calls.push(["createAgents", model]),
    announce: () => calls.push(["announce"]),
    runCommand: (id) => calls.push(["run", id]),
  };
  return { a1, a2, book, deck, calls };
}

test("assign takes the letter, then looks for sessions", () => {
  const { book, deck, calls } = rig();
  handleMessage({ cmd: "assign", window: "B" }, deck);
  assert.equal(book.window, "B");
  assert.deepEqual(calls, [["sync", "B"]]);
});

test("unassign removes the pidmap of the OLD letter before forgetting it", () => {
  const { book, deck, calls } = rig();
  handleMessage({ cmd: "unassign" }, deck);
  assert.deepEqual(calls, [["removePidMap", "A"]]);
  assert.equal(book.window, null);
});

test("focusPane takes the focus to exactly that pane", () => {
  const { a1, a2, deck } = rig();
  handleMessage({ cmd: "focusPane", slot: "A2" }, deck);
  assert.deepEqual(a2.log, [["show", false]]);
  assert.deepEqual(a1.log, []);
});

test("send with submit writes a bracketed paste, then a separate enter", async () => {
  const { a1, deck } = rig();
  handleMessage({ cmd: "send", slot: "A1", text: "hallo", submit: true }, deck);
  await new Promise((r) => setTimeout(r, 700));
  assert.deepEqual(a1.log, [["show", false], ["send", "\x1b[200~hallo\x1b[201~", false], ["send", "\r", false]]);
});

test("send without submit honours execute", async () => {
  const { a1, deck } = rig();
  handleMessage({ cmd: "send", slot: "A1", text: "/clear" }, deck);
  handleMessage({ cmd: "send", slot: "A1", text: "x", execute: false }, deck);
  await new Promise((r) => setTimeout(r, 150));
  assert.deepEqual(a1.log.filter((e) => e[0] === "send"), [["send", "/clear", true], ["send", "x", false]]);
});

test("key sends the mapped sequence, repeated", async () => {
  const { a1, deck } = rig();
  handleMessage({ cmd: "key", slot: "A1", key: "shift-tab", repeat: 2 }, deck);
  await new Promise((r) => setTimeout(r, 300));
  assert.deepEqual(a1.log, [["show", false], ["send", "\x1b[Z", false], ["send", "\x1b[Z", false]]);
});

test("an unknown key goes through raw", async () => {
  const { a1, deck } = rig();
  handleMessage({ cmd: "key", slot: "A1", key: "y" }, deck);
  await new Promise((r) => setTimeout(r, 150));
  assert.deepEqual(a1.log.at(-1), ["send", "y", false]);
});

test("broadcast reaches every slot of this window", () => {
  const { a1, a2, deck } = rig();
  handleMessage({ cmd: "broadcast", text: "hi" }, deck);
  assert.deepEqual([a1.log, a2.log], [[["send", "hi", true]], [["send", "hi", true]]]);
});

test("closeAgent closes the terminal and always reports back", () => {
  const { a1, deck, calls } = rig();
  handleMessage({ cmd: "closeAgent", slot: "A1" }, deck);
  assert.ok(a1.disposed);
  assert.deepEqual(calls, [["announce"]]);
  // Auch fuer einen unbekannten Slot: die Kachel ohne Chat muss weg.
  handleMessage({ cmd: "closeAgent", slot: "A9" }, deck);
  assert.deepEqual(calls, [["announce"], ["announce"]]);
});

test("window commands map to the VS Code commands", () => {
  const { deck, calls } = rig();
  for (const cmd of ["saveAll", "reload", "closeWindow"]) handleMessage({ cmd }, deck);
  assert.deepEqual(calls, [
    ["run", "workbench.action.files.saveAll"],
    ["run", "workbench.action.reloadWindow"],
    ["run", "workbench.action.closeWindow"],
  ]);
});

test("createAgent hands the whole message on", () => {
  const { deck, calls } = rig();
  const msg = { cmd: "createAgent", model: "opus", args: "--effort high", cwd: "C:\\r", slot: "A3" };
  handleMessage(msg, deck);
  handleMessage({ cmd: "createAgents", model: "sonnet" }, deck);
  assert.deepEqual(calls, [["createAgent", msg], ["createAgents", "sonnet"]]);
});

test("a command for an unknown slot or an unknown command does nothing", () => {
  const { a1, a2, deck, calls } = rig();
  handleMessage({ cmd: "focusPane", slot: "A9" }, deck);
  handleMessage({ cmd: "send", slot: "A9", text: "x" }, deck);
  handleMessage({ cmd: "gibtsnicht", slot: "A1" }, deck);
  assert.deepEqual([a1.log, a2.log, calls], [[], [], []]);
});

// ── Vertrag mit deck-core::protocol ─────────────────────────────────────────

const PROTOCOL = path.join(__dirname, "..", "..", "crates", "deck-core", "src", "protocol.rs");
const consts = (prefix) => [...fs.readFileSync(PROTOCOL, "utf8")
  .matchAll(new RegExp(`pub const ${prefix}_\\w+: &str = "([^"]+)"`, "g"))].map((m) => m[1]);

test("every command of the protocol has an effect here", async () => {
  const cmds = consts("CMD");
  assert.ok(cmds.length >= 10, `zu wenige CMD_* gelesen: ${cmds}`);
  for (const cmd of cmds) {
    const { a1, a2, book, deck, calls } = rig();
    handleMessage({ cmd, slot: "A1", window: "B", text: "t", key: "enter" }, deck);
    await new Promise((r) => setTimeout(r, 120));
    const effect = calls.length > 0 || a1.log.length > 0 || a2.log.length > 0 || a1.disposed || book.window !== "A";
    assert.ok(effect, `"${cmd}" aus protocol.rs verpufft in commands.js`);
  }
});

test("every message type of the protocol is sent by the extension", () => {
  const src = ["extension.js", "broker.js", "commands.js"]
    .map((f) => fs.readFileSync(path.join(__dirname, "..", f), "utf8")).join("\n");
  for (const type of consts("TYPE")) {
    assert.ok(src.includes(`type: "${type}"`), `"${type}" aus protocol.rs wird nie gesendet`);
  }
});
