// Reine Erkennungs-Helfer fuer Claude-Terminals - OHNE vscode/child_process,
// damit sie sich mit Node allein pruefen lassen.

// Ist der invozierte Befehl "claude"? Nimmt "claude" als eigenstaendiges Token
// (optional mit Pfad-Prefix und .cmd/.exe/.ps1). "claude-foo" oder "claudette"
// zaehlen NICHT (Wortgrenze), ein Pfad wie ...\claude-agent-deck\deck-report.exe auch nicht.
function isClaudeCommand(cmd) {
  if (!cmd) return false;
  const s = String(cmd);
  // Global per npm installiert laeuft Claude als `node ...\@anthropic-ai\claude-code\cli.js`;
  // dort steht kein nacktes "claude" in der Zeile (gleiches Muster wie killable.js).
  if (/[\\/]@anthropic-ai[\\/]claude-code[\\/]/i.test(s)) return true;
  return /(^|[\s|;&(])(?:[^\s|;&"']*[\\/])?claude(?:\.cmd|\.exe|\.ps1)?(?=$|\s|["'])/i.test(s);
}

// Terminal-Name deutet auf Claude hin: Deck-Kacheln (A1, A2, ...) oder "claude".
function looksLikeClaudeName(name, window) {
  if (!name) return false;
  if (window && new RegExp(`^${window}\\d+$`, "i").test(name)) return true;
  return /claude/i.test(name);
}

// Ein einzelner Prozess IST Claude: nativer claude.exe/.cmd/.ps1 oder ein
// anderer Prozess (z.B. node bei npm-Install), dessen Cmdline "claude" aufruft.
function isClaudeProc(p) {
  if (!p) return false;
  const name = String(p.Name || p.name || "");
  if (/^claude(\.exe|\.cmd|\.ps1)?$/i.test(name)) return true;
  return isClaudeCommand(p.CommandLine || p.commandLine || "");
}

// Laeuft irgendwo UNTER rootPid (Prozessbaum) ein Claude-Prozess?
// procs = Liste von {ProcessId, ParentProcessId, Name, CommandLine}.
function hasClaudeDescendant(rootPid, procs) {
  if (!rootPid || !Array.isArray(procs) || !procs.length) return false;
  const kids = new Map(); // ppid -> [proc]
  for (const p of procs) {
    const pp = Number(p.ParentProcessId != null ? p.ParentProcessId : p.parentProcessId);
    if (!kids.has(pp)) kids.set(pp, []);
    kids.get(pp).push(p);
  }
  const stack = [Number(rootPid)];
  const seen = new Set();
  while (stack.length) {
    const pid = stack.pop();
    if (seen.has(pid)) continue;
    seen.add(pid);
    for (const child of kids.get(pid) || []) {
      if (isClaudeProc(child)) return true;
      stack.push(Number(child.ProcessId != null ? child.ProcessId : child.processId));
    }
  }
  return false;
}

// Wie hasClaudeDescendant, gibt aber die PID des naechstgelegenen Claude-Prozesses
// UNTER rootPid zurueck (Breitensuche = die Session direkt unter der Shell), sonst
// null. Diese PID ist zugleich ein Vorfahre der Hook-Prozesse -> Bruecke zu deck-report
// (crates/deck-hooks).
function claudeDescendantPid(rootPid, procs) {
  if (!rootPid || !Array.isArray(procs) || !procs.length) return null;
  const kids = new Map(); // ppid -> [proc]
  for (const p of procs) {
    const pp = Number(p.ParentProcessId != null ? p.ParentProcessId : p.parentProcessId);
    if (!kids.has(pp)) kids.set(pp, []);
    kids.get(pp).push(p);
  }
  const queue = [Number(rootPid)];
  const seen = new Set();
  let head = 0;
  while (head < queue.length) {
    const pid = queue[head++];
    if (seen.has(pid)) continue;
    seen.add(pid);
    for (const child of kids.get(pid) || []) {
      const cpid = Number(child.ProcessId != null ? child.ProcessId : child.processId);
      if (isClaudeProc(child)) return cpid;
      queue.push(cpid);
    }
  }
  return null;
}

// PowerShell-JSON robust zu einem Array machen (ein Objekt -> [obj], leer -> []).
function parseProcList(jsonText) {
  if (!jsonText || !String(jsonText).trim()) return [];
  let data;
  try { data = JSON.parse(jsonText); } catch (e) { return []; }
  if (Array.isArray(data)) return data;
  return data ? [data] : [];
}

// Welche Slot-Namen zeigen auf ein Terminal, das VS Code nicht mehr fuehrt?
// entries = Map-Eintraege [[name, terminal], ...], live = vscode.window.terminals.
//
// Verglichen wird die IDENTITAET des Terminal-Objekts, denn am Objekt selbst ist
// nichts zu sehen: ein geschlossenes Terminal bleibt eine gueltige Referenz, man
// darf es weiter anfassen - es passiert nur nichts mehr. Ob es noch lebt, weiss
// allein VS Codes eigene Liste.
function deadSlots(entries, live) {
  // Keine Auskunft ist kein Todesurteil: kommt statt der Liste etwas anderes
  // herein, wird NICHT abgeraeumt. Dieselbe Vorsicht wie im Panel bei der
  // Fenster-Enumeration - lieber eine tote Kachel stehen lassen als eine
  // lebende faelschlich loeschen. Ein LEERES Array ist dagegen eine echte
  // Auskunft ("dieses Fenster fuehrt kein Terminal mehr") und zaehlt.
  if (!Array.isArray(live)) return [];
  const alive = new Set(live);
  const out = [];
  for (const e of entries || []) {
    if (e && !alive.has(e[1])) out.push(e[0]);
  }
  return out;
}

// Naechster freier Index fuer <window> aus den vorhandenen Slot-Namen (A1, A2, ...).
function nextIndex(slotNames, window) {
  let max = 0;
  for (const n of slotNames || []) {
    if (!n || String(n)[0] !== window) continue;
    const k = parseInt(String(n).slice(1), 10);
    if (!isNaN(k) && k > max) max = k;
  }
  return max + 1;
}

module.exports = {
  isClaudeCommand, looksLikeClaudeName, isClaudeProc,
  hasClaudeDescendant, claudeDescendantPid, parseProcList, nextIndex,
  deadSlots,
};
