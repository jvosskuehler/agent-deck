// Die Leitung zum Deck (broker.js): Zeilen schneiden, Erstmeldung, Empfang.
//
// Der echte Teil laeuft gegen einen Wegwerf-Server auf 127.0.0.1 mit einem freien
// Port vom Betriebssystem, NIE gegen 8765: dort lauscht womoeglich das laufende Deck.

const test = require("node:test");
const assert = require("node:assert/strict");
const net = require("net");
const { createBroker, splitLines } = require("../broker");

test("only complete lines are taken, the rest waits", () => {
  assert.deepEqual(splitLines('{"a":1}\n{"b":2}\n{"c"'), { lines: ['{"a":1}', '{"b":2}'], rest: '{"c"' });
  assert.deepEqual(splitLines(""), { lines: [], rest: "" });
});

test("hello goes out on connect, commands come in line by line", async (t) => {
  const server = net.createServer();
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  t.after(() => server.close());
  const port = server.address().port;

  const got = [];
  let gotAll;
  const allIn = new Promise((r) => { gotAll = r; });
  const broker = createBroker({
    address: () => ({ host: "127.0.0.1", port }),
    hello: () => ({ type: "hello", workspace: "w", folder: null, window: "A", remote: null, slots: ["A1"] }),
    onMessage: (m) => { got.push(m); if (got.length === 3) gotAll(); },
  });
  t.after(() => broker.dispose());

  const conn = new Promise((r) => server.once("connection", r));
  broker.connect();
  const sock = await conn;
  sock.setEncoding("utf8");
  const hello = await new Promise((r) => sock.once("data", r));
  // Feldreihenfolge wie bisher: die App liest per serde, aber Mitschnitte sollen
  // sich vergleichen lassen.
  assert.equal(hello, '{"type":"hello","workspace":"w","folder":null,"window":"A","remote":null,"slots":["A1"]}\n');

  // Zwei Zeilen in einem Block, eine kaputte, eine leere, eine auf zwei Bloecke verteilt.
  sock.write('{"cmd":"assign","window":"A"}\nkaputt\n\n{"cmd":"focusPane",');
  await new Promise((r) => setTimeout(r, 30));
  sock.write('"slot":"A1"}\n{"cmd":"reload"}\n');
  await allIn;
  assert.deepEqual(got, [{ cmd: "assign", window: "A" }, { cmd: "focusPane", slot: "A1" }, { cmd: "reload" }]);

  broker.send({ type: "seen", window: "A", slot: "A1" });
  const seen = await new Promise((r) => sock.once("data", r));
  assert.equal(seen, '{"type":"seen","window":"A","slot":"A1"}\n');
});

test("a handler that throws does not stop the following lines", async (t) => {
  const server = net.createServer();
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  t.after(() => server.close());
  const seen = [];
  let done;
  const finished = new Promise((r) => { done = r; });
  const broker = createBroker({
    address: () => ({ host: "127.0.0.1", port: server.address().port }),
    hello: () => ({ type: "hello" }),
    onMessage: (m) => { seen.push(m.n); if (m.n === 1) throw new Error("x"); done(); },
  });
  t.after(() => broker.dispose());
  server.once("connection", (s) => s.write('{"n":1}\n{"n":2}\n'));
  broker.connect();
  await finished;
  assert.deepEqual(seen, [1, 2]);
});

test("sending without a connection is silent", () => {
  const broker = createBroker({ address: () => ({}), hello: () => ({}), onMessage: () => {} });
  assert.doesNotThrow(() => broker.send({ type: "terminals" }));
  broker.dispose();
});
