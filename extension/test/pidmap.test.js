// Die pidmap (pidmap.js): das Format, das deck-report liest
// (crates/deck-hooks/src/env.rs, `load_pidmap`). Geschrieben wird nur in einen
// Wegwerf-Ordner, nie ins echte LOCALAPPDATA.

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("fs");
const os = require("os");
const path = require("path");
const pm = require("../pidmap");

function tmp(t) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "deck-pidmap-"));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  return dir;
}

test("the state folder is the one the hooks write to", () => {
  assert.equal(pm.stateDir({ LOCALAPPDATA: "C:\\L" }), path.join("C:\\L", "claude-agent-deck", "state"));
});

test("the file is named after the window", () => {
  assert.equal(path.basename(pm.pidmapPath("d", "B")), "pidmap-B.json");
});

test("the content maps claude pid to slot and skips unresolved slots", () => {
  const m = new Map([["A1", 4711], ["A2", null], ["A3", 12]]);
  assert.deepEqual(pm.pidmapContent(m), { 4711: "A1", 12: "A3" });
});

test("writing creates the folder and leaves no temp file", (t) => {
  const dir = path.join(tmp(t), "state");
  pm.writePidMap(dir, "A", new Map([["A1", 4711]]));
  assert.deepEqual(JSON.parse(fs.readFileSync(path.join(dir, "pidmap-A.json"), "utf8")), { 4711: "A1" });
  assert.deepEqual(fs.readdirSync(dir), ["pidmap-A.json"]);
});

test("writing again replaces the old map", (t) => {
  const dir = tmp(t);
  pm.writePidMap(dir, "A", new Map([["A1", 1]]));
  pm.writePidMap(dir, "A", new Map([["A2", 2]]));
  assert.deepEqual(JSON.parse(fs.readFileSync(path.join(dir, "pidmap-A.json"), "utf8")), { 2: "A2" });
});

test("a write that fails never throws", (t) => {
  // Die Extension laeuft im Extension-Host: eine Ausnahme hier risse die Erkennung mit.
  const file = path.join(tmp(t), "kein-ordner");
  fs.writeFileSync(file, "");
  assert.doesNotThrow(() => pm.writePidMap(file, "A", new Map([["A1", 1]])));
});

test("removing deletes the map and a missing map is fine", (t) => {
  const dir = tmp(t);
  pm.writePidMap(dir, "A", new Map([["A1", 1]]));
  pm.removePidMap(dir, "A");
  assert.deepEqual(fs.readdirSync(dir), []);
  assert.doesNotThrow(() => pm.removePidMap(dir, "A"));
});
