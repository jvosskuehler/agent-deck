// Offene Claude-Sessions in diesem Fenster finden, auch die, die NICHT ueber das Deck
// gestartet wurden. Ein Terminal gilt als Claude-Terminal, wenn
//   1) in ihm per Shell-Integration "claude" gestartet wurde, ODER
//   2) unter seinem Shell-Prozess ein claude(.exe)-Prozess laeuft (Prozess-Scan), ODER
//   3) sein Name danach aussieht (A1.. oder enthaelt "claude").
// Dazu die Claude-PID je Slot fuer die pidmap (Status-Bruecke zu deck-report).
//
// Laedt vscode nicht selbst; die Terminals kommen ueber `deck` (siehe extension.js).
// deck = { book, remote, liveTerminals(), announce(), writePidMap() }

const detect = require("./detect");
const psworker = require("./psworker");

function createClaudeSync(deck) {
  const { book } = deck;
  let procs = [];
  let lastScan = 0;
  let lastPidRefresh = 0;
  let syncing = false;
  // EIN PowerShell-Prozess fuer alle Scans dieses Fensters, statt einer neuen
  // powershell.exe je Scan (Kosten und Messung in psworker.js). Erst beim ersten
  // Scan gestartet, also nie in einem Remote-Fenster.
  let procWorker = null;

  function scanProcs() {
    // Nur Windows; sonst greifen Name + Shell-Integration.
    // Und NICHT in einem Remote-Fenster: term.processId liefert dort eine PID von der
    // Gegenstelle. Dieselbe Zahl gibt es hier auch - sie gehoert nur einem voellig
    // anderen Prozess. Ein Baum-Vergleich damit beantwortet nicht die gestellte Frage,
    // er beantwortet eine zufaellige. Bleiben Name und Shell-Integration.
    if (deck.remote || process.platform !== "win32") return Promise.resolve([]);
    if (!procWorker) procWorker = new psworker.PsWorker();
    // null = Worker gescheitert oder Zeitlimit: wie frueher ein Fehler von execFile
    // eine leere Liste, also "nichts gefunden" statt eines halben Baums.
    return procWorker.run(psworker.PROC_SCAN, 6000)
      .then((out) => (out === null ? [] : detect.parseProcList(out)));
  }

  async function getProcs(force) {
    const now = Date.now();
    if (!force && now - lastScan < 2500) return { procs, fresh: false }; // Drosseln
    lastScan = now;
    procs = await scanProcs();
    return { procs, fresh: true };
  }

  async function syncClaude() {
    if (!book.window || syncing) return;
    syncing = true;
    try {
      const unknown = deck.liveTerminals().filter((t) => !book.termSlot.has(t));
      // Kandidaten fuer den Prozess-Scan: weder per Name noch per Shell-Integration
      // klar, und noch nicht per FRISCHEM Scan als "kein claude" abgehakt.
      const candidates = unknown.filter((t) =>
        !detect.looksLikeClaudeName(t.name, book.window) && !book.claudeByExec.has(t) && !book.procChecked.has(t));
      let found = [], fresh = false;
      if (candidates.length) ({ procs: found, fresh } = await getProcs(true)); // frischer Scan, wenn es ungeprueftes Terminal gibt
      for (const term of unknown) {
        const byName = detect.looksLikeClaudeName(term.name, book.window);
        const byExec = book.claudeByExec.has(term);
        let byProc = false;
        if (!byName && !byExec) {
          let pid;
          try { pid = await term.processId; } catch (e) { pid = undefined; }
          byProc = pid ? detect.hasClaudeDescendant(pid, found) : false;
        }
        if (byName || byExec || byProc) {
          book.ensure(term, book.preferredName(term));
          book.procChecked.delete(term);
        } else if (fresh && !byName && !byExec) {
          book.procChecked.add(term); // nur nach ECHTEM Scan abhaken -> nicht dauernd neu scannen
        }
      }
      deck.announce();
    } finally {
      syncing = false;
    }
  }

  // Fuer jedes bekannte Claude-Terminal die Claude-PID (via Prozess-Scan) aufloesen
  // und die pidmap aktualisieren. Gedrosselt, weil der Scan (PowerShell) teuer ist.
  // Remote: keine pidmap (Begruendung bei writePidMap in extension.js).
  async function refreshPids() {
    if (!book.window || !book.slots.size || deck.remote) return;
    const now = Date.now();
    if (now - lastPidRefresh < 8000) return; // hoechstens alle 8s
    lastPidRefresh = now;
    const { procs: list } = await getProcs(false); // nutzt einen frischen Scan mit, sonst neu
    if (!list.length) return;
    let changed = false;
    for (const [name, term] of book.slots) {
      let pid;
      try { pid = await term.processId; } catch (e) { pid = undefined; }
      const cpid = pid ? detect.claudeDescendantPid(pid, list) : null;
      if (cpid && book.slotPid.get(name) !== cpid) { book.slotPid.set(name, cpid); changed = true; }
    }
    if (changed) deck.writePidMap();
  }

  // Neues Terminal -> PID gleich beim naechsten Tick aufloesen, nicht erst nach 8 s.
  function resetPidRefresh() { lastPidRefresh = 0; }

  function dispose() {
    if (procWorker) { procWorker.dispose(); procWorker = null; }
  }

  return { syncClaude, refreshPids, resetPidRefresh, dispose };
}

module.exports = { createClaudeSync };
