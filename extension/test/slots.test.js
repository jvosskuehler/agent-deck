// Die Slot-Buchfuehrung (slots.js): welcher Name zu welchem Terminal gehoert. Was in
// der Map steht, ist eine Kachel im Panel; ein Fehler hier ist eine Kachel ohne Chat
// oder ein Chat ohne Kachel.

const test = require("node:test");
const assert = require("node:assert/strict");
const { SlotBook } = require("../slots");

// Terminals sind fuer die Buchfuehrung undurchsichtig; ein Objekt mit Namen genuegt.
const term = (name) => ({ name });

function rig(window = "A") {
  const live = [];
  const calls = { pruned: 0, newSlot: 0 };
  const book = new SlotBook({
    live: () => live,
    onPruned: () => calls.pruned++,
    onNewSlot: () => calls.newSlot++,
  });
  book.window = window;
  return { book, live, calls };
}

test("a live terminal gets its preferred name", () => {
  const { book, live, calls } = rig();
  const t = term("A1");
  live.push(t);
  assert.equal(book.ensure(t, "A1"), "A1");
  assert.equal(book.termSlot.get(t), "A1");
  assert.equal(calls.newSlot, 1);
});

test("a known terminal keeps its name", () => {
  const { book, live, calls } = rig();
  const t = term("x");
  live.push(t);
  const first = book.ensure(t, null);
  assert.equal(book.ensure(t, "A9"), first);
  assert.equal(calls.newSlot, 1);
});

test("a terminal closed during the wait gets no tile", () => {
  // Zwischen Einfangen und ensure liegen await-Pausen; eine Kachel fuer ein schon
  // geschlossenes Terminal raeumte kein Close-Event je wieder ab.
  const { book } = rig();
  assert.equal(book.ensure(term("A1"), "A1"), null);
  assert.equal(book.slots.size, 0);
});

test("a taken name is not overwritten, the newcomer counts on", () => {
  const { book, live } = rig();
  const a = term("A1"), b = term("A1");
  live.push(a, b);
  book.ensure(a, "A1");
  assert.equal(book.ensure(b, "A1"), "A2");
  assert.equal(book.slots.get("A1"), a);
});

test("dead slots are pruned before a name is chosen", () => {
  const { book, live, calls } = rig();
  const a = term("A1"), b = term("A2");
  live.push(a);
  book.ensure(a, "A1");
  book.slotPid.set("A1", 4711);
  live.length = 0;          // a geschlossen, ohne Close-Event
  live.push(b);
  assert.equal(book.ensure(b, "A1"), "A1");
  assert.equal(book.slotPid.has("A1"), false, "die PID der Leiche geht mit");
  assert.equal(book.termSlot.has(a), false);
  assert.equal(calls.pruned, 1);
});

test("pruning nothing does not touch the pidmap", () => {
  const { book, live, calls } = rig();
  const a = term("A1");
  live.push(a);
  book.ensure(a, "A1");
  assert.equal(book.prune(), 0);
  assert.equal(calls.pruned, 0);
});

test("a closed terminal is forgotten everywhere", () => {
  const { book, live } = rig();
  const a = term("A1");
  live.push(a);
  book.ensure(a, "A1");
  book.slotPid.set("A1", 1);
  book.claudeByExec.add(a);
  book.procChecked.add(a);
  book.forget(a);
  assert.equal(book.slots.size + book.termSlot.size + book.slotPid.size, 0);
  assert.equal(book.claudeByExec.size + book.procChecked.size, 0);
});

test("only deck names of this window are preferred", () => {
  const { book } = rig("B");
  assert.equal(book.preferredName(term("b4")), "b4");
  assert.equal(book.preferredName(term("A4")), null);
  assert.equal(book.preferredName(term("claude")), null);
});

test("a wanted slot from the panel holds only when it is ours and free", () => {
  const { book, live } = rig();
  const a = term("A1");
  live.push(a);
  book.ensure(a, "A1");
  assert.equal(book.wantedSlot("a3"), "A3");
  assert.equal(book.wantedSlot("A1"), null, "belegt");
  assert.equal(book.wantedSlot("B3"), null, "fremdes Fenster");
  assert.equal(book.wantedSlot("A"), null);
  assert.equal(book.wantedSlot(undefined), null);
});

test("the next name follows the highest index", () => {
  const { book, live } = rig();
  const a = term("A5");
  live.push(a);
  book.ensure(a, "A5");
  assert.equal(book.nextName(), "A6");
  assert.deepEqual(book.names(), ["A5"]);
});
