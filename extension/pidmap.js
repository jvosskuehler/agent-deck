// Die pidmap: Bruecke zwischen den Claude-Sessions dieses Fensters und den Hooks.
//
// deck-report (crates/deck-hooks/src/env.rs) sucht in seiner Ahnenreihe den ersten
// Prozess, der in einer `pidmap-<Fenster>.json` steht, und meldet seinen Status auf
// dessen Slot. So bekommen auch Sessions eine Kachel-Farbe, die NICHT ueber das Deck
// gestartet wurden (ohne AGENT_SLOT in der Umgebung).
//
// Ohne vscode, und der Ordner kommt als Argument herein: so laesst sich das Format
// gegen einen Wegwerf-Ordner pruefen, ohne das echte LOCALAPPDATA anzufassen.

const fs = require("fs");
const path = require("path");
const os = require("os");

// Ordner, in den auch die Hooks schreiben (deck-core::paths, `state`).
function stateDir(env) {
  const e = env || process.env;
  return path.join(e.LOCALAPPDATA || os.homedir(), "claude-agent-deck", "state");
}

function pidmapPath(dir, window) {
  return path.join(dir, `pidmap-${window}.json`);
}

// { "<claudePid>": "A1", ... } aus slotName -> PID. Ein Slot ohne (bisher
// aufgeloeste) PID fehlt; ein Schluessel "undefined" lenkte sonst nichts, waere aber
// fuer den Leser der Datei ein Raetsel.
function pidmapContent(slotPid) {
  const map = {};
  for (const [name, pid] of slotPid || []) if (pid) map[String(pid)] = name;
  return map;
}

// Atomar schreiben (.tmp + umbenennen): deck-report liest ohne Sperre und darf nie
// eine halbe Datei sehen (CLAUDE.md, Daten und Vertraege 1).
function writePidMap(dir, window, slotPid) {
  try {
    fs.mkdirSync(dir, { recursive: true });
    const dst = pidmapPath(dir, window);
    fs.writeFileSync(dst + ".tmp", JSON.stringify(pidmapContent(slotPid)));
    fs.renameSync(dst + ".tmp", dst);
  } catch (e) { /* darf nie stoeren */ }
}

function removePidMap(dir, window) {
  try { fs.unlinkSync(pidmapPath(dir, window)); } catch (e) { /* egal */ }
}

module.exports = { stateDir, pidmapPath, pidmapContent, writePidMap, removePidMap };
