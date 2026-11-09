// Der PowerShell-Worker der Extension: ein Prozess fuer alle Scans, und er heilt sich.
//
// Geprueft wird zweierlei. Erstens das Protokoll ohne Prozess: eine Antwort ist erst
// mit ihrer Endzeile fertig, eine halbe bleibt liegen, und eine Kommandozeile im Scan
// darf keine Antwort vorzeitig beenden. Zweitens ein echter Worker (nur Windows):
// dass er wirklich EIN Prozess bleibt, Umlaute heil liefert, und nach Zeitlimit,
// Absturz oder Fehler im Skript wieder antwortet. Haengt er fest, wuerde die Erkennung
// sonst still nichts mehr finden, und das sieht wie "kein Claude offen" aus.
// Portiert aus `git show 2856494:tests/test_extension_psworker.py`.
//
// Der echte Worker startet nur sein eigenes powershell.exe und beendet nur das; der
// Scan liest die Prozessliste, er greift in keinen fremden Prozess ein.

const test = require("node:test");
const assert = require("node:assert/strict");
const w = require("../psworker");

const M = "<<agent-deck-ps:00112233aabbccdd>>";

// ── Protokoll ohne Prozess ──────────────────────────────────────────────────

test("a response is only complete with its end line", () => {
  const r = w.takeResponses(`[1,2]\n${M} 1 o`, M);
  assert.deepEqual(r.done, []);
  assert.equal(r.rest, `[1,2]\n${M} 1 o`);
});

test("a complete response carries body, id and success", () => {
  const r = w.takeResponses(`[1,2]\n${M} 7 ok\n`, M);
  assert.deepEqual(r.done, [{ id: "7", ok: true, body: "[1,2]" }]);
  assert.equal(r.rest, "");
});

test("two responses in one block both come out", () => {
  const r = w.takeResponses(`a\n${M} 1 ok\nb\n${M} 2 ok\n`, M);
  assert.deepEqual(r.done.map((d) => [d.id, d.body]), [["1", "a"], ["2", "b"]]);
});

test("half a second response waits for the next block", () => {
  const r = w.takeResponses(`a\n${M} 1 ok\n[{"Name":"no`, M);
  assert.deepEqual(r.done.map((d) => d.id), ["1"]);
  assert.equal(r.rest, '[{"Name":"no');
});

test("a foreign mark inside the body ends no response", () => {
  // Eine Kommandozeile im Scan kann beliebigen Text enthalten - nur nicht die
  // Zufallszahl DIESES Worker-Starts.
  const r = w.takeResponses(`x\n<<agent-deck-ps:ffffffffffffffff>> 1 ok\ny`, M);
  assert.deepEqual(r.done, []);
});

test("err in the end line is a failure", () => {
  const r = w.takeResponses(`\n${M} 3 err\n`, M);
  assert.deepEqual(r.done, [{ id: "3", ok: false, body: "" }]);
});

test("a BOM at the start is not part of the response", () => {
  const r = w.takeResponses(`﻿[]\n${M} 1 ok\n`, M);
  assert.equal(r.done[0].body, "[]");
});

test("a request is one line and arrives unbent", () => {
  const script = "$a = \"Voßkühler\"\n'zwei' | Out-String";
  const line = w.encodeRequest("5", script);
  assert.ok(line.endsWith("\n"));
  assert.equal(line.split("\n").length, 2);
  const [id, b64] = line.trimEnd().split(" ");
  assert.equal(id, "5");
  assert.equal(Buffer.from(b64, "base64").toString("utf8"), script);
});

test("the worker loop goes to PowerShell as UTF-16LE base64", () => {
  // -EncodedCommand erwartet genau das; Anfuehrungszeichen im Skript sind dann egal.
  const script = w.workerScript(M);
  assert.equal(Buffer.from(w.encodeCommand(script), "base64").toString("utf16le"), script);
  assert.ok(script.includes(M));
});

// ── Ein echter Worker ───────────────────────────────────────────────────────

// Ein Lauf, viele Fragen: jeder Start von PowerShell kostet Sekunden, also fuehrt EIN
// Worker alle Schritte nacheinander aus und die Tests lesen daraus.
const windows = process.platform === "win32";

test("a real worker answers, heals itself and goes away", { skip: !windows && "nur Windows (powershell.exe)" }, async (t) => {
  const pw = new w.PsWorker({ idleMs: 0 });
  const step = async (script, timeout) => {
    const value = await pw.run(script, timeout || 20000);
    return { value, pid: pw.pid, starts: pw.starts };
  };
  try {
    const a = await step(w.PROC_SCAN);
    const b = await step(w.PROC_SCAN);
    await t.test("two scans run in the same process", () => {
      assert.ok(a.pid && a.pid === b.pid);
      assert.equal(b.starts, 1);
    });
    await t.test("the scan has exactly the four fields of the detection", () => {
      const procs = JSON.parse(b.value);
      assert.ok(procs.length > 10);
      for (const p of procs) {
        assert.deepEqual(Object.keys(p).sort(), ["CommandLine", "Name", "ParentProcessId", "ProcessId"]);
      }
    });
    await t.test("the scan finds the caller by name", () => {
      const self = JSON.parse(b.value).filter((p) => p.ProcessId === process.pid);
      assert.equal(self.length, 1);
      assert.equal(self[0].Name.toLowerCase(), "node.exe");
    });
    await t.test("umlauts come back whole", async () => {
      assert.equal((await step("'Voßkühler'")).value, "Voßkühler");
    });
    await t.test("an error in the script is null and the worker lives on", async () => {
      assert.equal((await step("throw 'kaputt'")).value, null);
      const after = await step("1 + 1");
      assert.equal(after.value, "2");
      assert.equal(after.starts, 1);
    });
    await t.test("after the time limit a fresh worker answers", async () => {
      assert.equal((await step("Start-Sleep -Seconds 10; 'zu spaet'", 1500)).value, null);
      const after = await step("'wieder da'");
      assert.equal(after.value, "wieder da");
      assert.equal(after.starts, 2);
    });
    await t.test("after a crash a fresh worker answers", async () => {
      assert.equal((await step("[Environment]::Exit(3)")).value, null);
      const after = await step("'nach dem Absturz'");
      assert.equal(after.value, "nach dem Absturz");
      assert.equal(after.starts, 3);
    });
  } finally {
    const last = pw.pid;
    pw.dispose();
    await t.test("dispose ends the process", async () => {
      let alive = !!last;
      for (let i = 0; alive && i < 30; i++) {
        await new Promise((r) => setTimeout(r, 100));
        try { process.kill(last, 0); } catch (e) { alive = false; }
      }
      assert.equal(alive, false);
    });
  }
});
