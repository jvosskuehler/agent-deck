// Agent Deck Bridge - reine JS-VS-Code-Extension (kein Build-Schritt).
//
// Verbindet sich mit dem Agent-Deck-Panel (TCP, newline-getrenntes JSON) und
// steuert die Claude-Code-Terminals DIESES Fensters:
//   - focusPane: exakten Split-Pane fokussieren  (terminal.show)
//   - send/key:  Text/Steuerzeichen in den pty schreiben  (terminal.sendText)
//   - broadcast: an alle Terminals dieses Fensters
//   - createAgent(s): neue Claude-Terminals anlegen (mit AGENT_SLOT)
//   - assign:    das Panel weist diesem Fenster den Buchstaben A/B zu
//
// Dazu ein Befehl, der ohne das Panel auskommt (Ctrl+Alt+K): Port-Blocker
// abraeumen - die Dev-Server unter diesem VS Code, die noch einen Port halten.
//
// ERKENNUNG offener Claude-Sessions (nicht nur Deck-eigene): ein Terminal gilt
// als Claude-Terminal, wenn
//   1) in ihm per Shell-Integration "claude" gestartet wurde, ODER
//   2) unter seinem Shell-Prozess ein claude(.exe)-Prozess laeuft (Prozess-Scan), ODER
//   3) sein Name danach aussieht (A1.. oder enthaelt "claude").
// So tauchen auch Sessions auf, die NICHT ueber das Deck gestartet wurden.
//
// Warum eine Extension? Ein VS-Code-Fenster ist EIN Chromium-Fenster ohne
// per-Pane-HWNDs - Win32/SendInput kann einen einzelnen Split-Pane nicht
// treffen. sendText schreibt direkt in den pty des richtigen Panes, ohne Fokus
// zu klauen.

const vscode = require("vscode");
const net = require("net");
const cp = require("child_process");
const fs = require("fs");
const path = require("path");
const os = require("os");
const detect = require("./detect");
const killable = require("./killable");

let sock = null;
let reconnectTimer = null;
let scanTimer = null;
let myWindow = null; // "A"/"B" - via Panel (assign) oder optionalem Setting

const slots = new Map();       // slotName -> Terminal (was das Panel zeigt)
const termSlot = new Map();    // Terminal   -> slotName (Rueckrichtung, stabil)
const slotPid = new Map();     // slotName -> Claude-PID (fuer die pidmap; Status-Bruecke)
const claudeByExec = new Set(); // Terminals, in denen "claude" gestartet wurde
const procChecked = new Set();  // per Prozess-Scan geprueft + (noch) kein claude -> nicht dauernd neu scannen

// Ordner, in den auch die Hooks (report.py) schreiben. Hierhin legen wir die
// pidmap-<Fenster>.json, damit report.py selbst gestartete Sessions einem Slot
// zuordnen kann (siehe report.py / deck_common.py).
function stateDir() {
  return path.join(process.env.LOCALAPPDATA || os.homedir(), "claude-agent-deck", "state");
}

// pidmap dieses Fensters atomar schreiben: { "<claudePid>": "A1", ... }.
function writePidMap() {
  if (!myWindow) return;
  try {
    const dir = stateDir();
    fs.mkdirSync(dir, { recursive: true });
    const map = {};
    for (const [name, pid] of slotPid) if (pid) map[String(pid)] = name;
    const dst = path.join(dir, `pidmap-${myWindow}.json`);
    fs.writeFileSync(dst + ".tmp", JSON.stringify(map));
    fs.renameSync(dst + ".tmp", dst);
  } catch (e) { /* darf nie stoeren */ }
}

function removePidMap() {
  if (!myWindow) return;
  try { fs.unlinkSync(path.join(stateDir(), `pidmap-${myWindow}.json`)); } catch (e) { /* egal */ }
}

// Fuer jedes bekannte Claude-Terminal die Claude-PID (via Prozess-Scan) aufloesen
// und die pidmap aktualisieren. Gedrosselt, weil der Scan (PowerShell) teuer ist.
let _lastPidRefresh = 0;
async function refreshPids() {
  if (!myWindow || !slots.size) return;
  const now = Date.now();
  if (now - _lastPidRefresh < 8000) return; // hoechstens alle 8s
  _lastPidRefresh = now;
  const { procs } = await getProcs(false); // nutzt einen frischen Scan mit, sonst neu
  if (!procs.length) return;
  let changed = false;
  for (const [name, term] of slots) {
    let pid;
    try { pid = await term.processId; } catch (e) { pid = undefined; }
    const cpid = pid ? detect.claudeDescendantPid(pid, procs) : null;
    if (cpid && slotPid.get(name) !== cpid) { slotPid.set(name, cpid); changed = true; }
  }
  if (changed) writePidMap();
}

function cfg() {
  const c = vscode.workspace.getConfiguration("agentDeck");
  return {
    host: c.get("host", "127.0.0.1"),
    port: c.get("port", 8765),
    window: c.get("window", "") || null, // optionaler Vorbeleg
    autostart: c.get("autostartCommand", "claude"),
    slotEnvCommand: c.get("slotEnvCommand", ""), // Shell-Zeile zum Setzen von AGENT_SLOT ({slot})
  };
}

function workspaceName() {
  const f = vscode.workspace.workspaceFolders;
  if (f && f.length) return f[0].name;
  // Kein Ordner offen (leeres Fenster ODER ein .code-workspace ohne Ordner) ->
  // KEIN echter Projektname. Bewusst NICHT auf vscode.workspace.name zurueckfallen:
  // das liefert sonst "Untitled (Workspace)" o.ae. und wuerde – wie frueher das
  // literale "unknown" – als Phantom-Kachel gebunden und persistiert. null = das
  // Panel ignoriert so ein Fenster (bis ein Ordner geoeffnet wird).
  return null;
}

// Panel-Taste -> Byte-Sequenz an den pty. Bare Keys immer mit sendText(seq,false),
// sonst haengt VS Code ein \r an und submitted zu frueh.
const KEYMAP = {
  enter: "\r", esc: "\x1b", "ctrl-c": "\x03",
  up: "\x1b[A", down: "\x1b[B", right: "\x1b[C", left: "\x1b[D",
  tab: "\t", "shift-tab": "\x1b[Z", // back-tab: Claude Code schaltet damit den Permission-Mode um
  "1": "1", "2": "2", "3": "3", "4": "4", "5": "5",
};

// Tastensequenz (evtl. mehrfach) an den pty schicken. Zwischen Wiederholungen
// kurz warten, damit die Claude-TUI jeden Schritt (z.B. Moduswechsel) registriert.
async function sendKey(term, key, repeat) {
  const seq = (key in KEYMAP) ? KEYMAP[key] : key;
  const n = Math.max(1, repeat || 1);
  for (let i = 0; i < n; i++) {
    term.sendText(seq, false);
    if (i < n - 1) await new Promise((r) => setTimeout(r, 90));
  }
}

// Vor JEDER Eingabe (Text/Taste) den GENAUEN Ziel-Pane aktiv machen. sendText/
// sendKey schreiben zwar direkt in den pty dieses Terminals, aber damit die
// Eingabe garantiert HIER landet und nicht im zuletzt fokussierten Terminal,
// holen wir den Ziel-Pane vorher explizit in den Fokus (preserveFocus=false)
// und geben VS Code einen Moment, den Wechsel zu vollziehen.
async function focusPane(term) {
  try {
    term.show(false);
    await new Promise((r) => setTimeout(r, 80));
  } catch (e) { /* ignore */ }
}

// Verzoegerung (ms) zwischen Text und dem separaten Enter im submit-Pfad. Claude
// Code erkennt einen grossen Input-Burst als PASTE und schluckt ein im selben
// sendText angehaengtes \r in diese Erkennung -> der Prompt bliebe im Eingabefeld
// stehen. Darum: Text OHNE Newline schreiben, kurz warten bis die Paste-Erkennung
// durch ist, DANN ein eigenstaendiges Enter zum Abschicken. Grosszuegig gewaehlt
// (unmerklich fuer den Nutzer), damit auch ein langsam gerenderter pty sicher folgt.
const SUBMIT_ENTER_DELAY_MS = 450;

async function sendToPane(term, text, execute, submit) {
  await focusPane(term);
  try {
    if (submit) {
      // Prompt zuverlaessig UND VOLLSTAENDIG abschicken. Zwei Fallen bei langem Text:
      //  1) execute=true haengt das \r direkt an -> Claude-Codes Paste-Erkennung
      //     schluckt es -> nichts abgeschickt.
      //  2) rohes sendText eines langen Bursts -> Claude puffert zeitbasiert und
      //     verwirft dabei den Anfang -> nur das ENDE des Prompts kommt an.
      // Loesung: den Text explizit als EINE Bracketed-Paste kapseln (\x1b[200~ …
      // \x1b[201~). Der Ende-Marker sagt Claude eindeutig "Paste zu Ende" -> der
      // ganze Block wird als eine Einheit uebernommen (kein Abschneiden, kein
      // vorzeitiges Absenden). DANACH, nach kurzer Pause, ein separates Enter zum
      // Abschicken. Claude Code hat waehrend der Prompt-Eingabe Bracketed-Paste-Mode
      // aktiv, verarbeitet die Marker also korrekt (statt sie als Text zu zeigen).
      term.sendText("\x1b[200~" + (text || "") + "\x1b[201~", false);
      await new Promise((r) => setTimeout(r, SUBMIT_ENTER_DELAY_MS));
      term.sendText("\r", false);
    } else {
      term.sendText(text || "", execute !== false);
    }
  } catch (e) { /* ignore */ }
}

async function keyToPane(term, key, repeat) {
  await focusPane(term);
  try { await sendKey(term, key, repeat); } catch (e) { /* ignore */ }
}

function send(obj) {
  if (sock && !sock.destroyed) {
    try { sock.write(JSON.stringify(obj) + "\n"); } catch (e) { /* ignore */ }
  }
}

function announce() {
  send({ type: "terminals", window: myWindow, workspace: workspaceName(), slots: [...slots.keys()] });
}

function connect() {
  const { host, port } = cfg();
  sock = net.createConnection({ host, port }, () => {
    send({ type: "hello", workspace: workspaceName(), window: myWindow, slots: [...slots.keys()] });
  });
  let buf = "";
  sock.on("data", (d) => {
    buf += d.toString("utf8");
    let i;
    while ((i = buf.indexOf("\n")) >= 0) {
      const line = buf.slice(0, i);
      buf = buf.slice(i + 1);
      if (line.trim()) {
        try { handle(JSON.parse(line)); } catch (e) { /* kaputte Zeile ignorieren */ }
      }
    }
  });
  sock.on("error", () => { /* Panel evtl. noch nicht da -> reconnect via close */ });
  sock.on("close", scheduleReconnect);
}

function scheduleReconnect() {
  if (reconnectTimer) return;
  reconnectTimer = setTimeout(() => { reconnectTimer = null; connect(); }, 2000);
}

function handle(msg) {
  const t = msg.slot ? slots.get(msg.slot) : null;
  switch (msg.cmd) {
    case "assign":
      myWindow = msg.window;
      syncClaude();
      break;
    case "unassign":
      // Panel hat die Zuordnung geloest -> Buchstabe vergessen, damit dieses Fenster
      // nicht ueber einen alten myWindow-Wert als Phantomkachel zurueckkommt.
      removePidMap();
      myWindow = null;
      break;
    case "focusPane":
      if (t) t.show(false); // false = Fokus nehmen (fuer Diktat)
      break;
    case "send":
      // Ziel-Pane erst fokussieren, DANN Text schicken -> landet garantiert im
      // angeklickten Agent-Chat, nicht im zuletzt fokussierten Terminal. msg.submit
      // (z.B. Ticket-Prompt) -> Text schreiben und per separatem Enter abschicken.
      if (t) sendToPane(t, msg.text, msg.execute, msg.submit);
      break;
    case "key":
      if (t) keyToPane(t, msg.key, msg.repeat);
      break;
    case "broadcast":
      for (const term of slots.values()) term.sendText(msg.text || "", msg.execute !== false);
      break;
    case "createAgent":
      createAgent(msg.model);
      break;
    case "createAgents":
      createAgents(msg.model);
      break;
    case "reload":
      vscode.commands.executeCommand("workbench.action.reloadWindow");
      break;
    case "closeAgent":
      // Einzelnes Agent-Terminal schliessen (beendet den pty -> auch die darin
      // laufende Claude-Session). onDidCloseTerminal raeumt Slot/pidmap auf und
      // meldet die neue Terminalliste ans Panel -> die Kachel verschwindet.
      if (t) t.dispose();
      break;
    case "closeWindow":
      // Ganzes VS-Code-Fenster schliessen (schliesst zwangslaeufig alle Agenten
      // darin mit). Beim letzten Fenster beendet VS Code sich; ungespeicherte
      // Editoren fragt VS Code selbst ab.
      vscode.commands.executeCommand("workbench.action.closeWindow");
      break;
  }
}

// ── Slot-Zuordnung ────────────────────────────────────────
function ensureSlot(term, preferred) {
  if (termSlot.has(term)) return termSlot.get(term);
  const name = preferred || `${myWindow}${detect.nextIndex([...slots.keys()], myWindow)}`;
  slots.set(name, term);
  termSlot.set(term, name);
  _lastPidRefresh = 0; // neues Terminal -> PID gleich beim naechsten Tick aufloesen
  return name;
}

// Deck-eigenen Namen (A1, A2, ...) bevorzugen, sonst automatisch vergeben.
function preferredName(term) {
  const re = new RegExp(`^${myWindow}\\d+$`, "i");
  return term.name && re.test(term.name) ? term.name : null;
}

// ── Terminals anlegen ─────────────────────────────────────
async function createAgent(model) {
  if (!myWindow) return warnNotConnected();
  const { autostart } = cfg();
  const slot = `${myWindow}${detect.nextIndex([...slots.keys()], myWindow)}`;
  const all = [...slots.values()];
  const anchor = all[all.length - 1]; // LETZTES Panel -> neuer Chat reiht sich hinten ein

  let term = null, viaSplit = false;
  if (anchor) {
    term = await splitBeside(anchor);   // als Split in die Gruppe des ersten Panels
    viaSplit = !!term;
  }
  if (!term) {
    // Erster Chat (kein Anker) oder Split fehlgeschlagen -> eigenstaendiges Terminal.
    term = vscode.window.createTerminal({ name: slot, env: { AGENT_SLOT: slot } });
  }
  ensureSlot(term, slot);
  claudeByExec.add(term);
  procChecked.delete(term);
  term.show(false);
  if (autostart) {
    await new Promise((r) => setTimeout(r, 600)); // pty kurz Zeit geben
    // Split-Kommando kann kein env uebergeben -> AGENT_SLOT in der Shell setzen,
    // damit die Hooks (report.py) den Status dieser Kachel melden koennen.
    if (viaSplit) term.sendText(slotEnvCommand(slot));
    // Wunsch-Modell per CLI-Flag ERZWINGEN (`claude --model "<model>"`): das Flag hat
    // hoechste Prioritaet und schlaegt das zuletzt per /model gewaehlte, in
    // ~/.claude.json gemerkte Modell. Ohne Flag -> das "zuletzt verwendete Modell".
    // Anfuehrungszeichen halten das '[1m]'-Suffix in PowerShell/bash literal.
    const cmd = model ? `${autostart} --model "${model}"` : autostart;
    term.sendText(cmd, true);
  }
  announce();
}

// Zuverlaessiger Split (VS-Code-Bug #205254: createTerminal({location:{parentTerminal}})
// splittet gespeicherte/bereits offene Terminals NICHT). Daher: Anker aktiv machen,
// Split-Kommando ausloesen und das neu geoeffnete Terminal einfangen.
async function splitBeside(anchor) {
  try {
    anchor.show(false); // Kommando splittet das AKTIVE Terminal -> Anker aktiv machen
    await new Promise((r) => setTimeout(r, 120));
    return await new Promise((resolve) => {
      const d = vscode.window.onDidOpenTerminal((t) => { d.dispose(); resolve(t); });
      const to = setTimeout(() => { d.dispose(); resolve(null); }, 3000);
      Promise.resolve(vscode.commands.executeCommand("workbench.action.terminal.split"))
        .catch(() => { clearTimeout(to); d.dispose(); resolve(null); });
    });
  } catch (e) {
    return null;
  }
}

function slotEnvCommand(slot) {
  const tmpl = cfg().slotEnvCommand;
  if (tmpl) return tmpl.replace(/\{slot\}/g, slot);
  // Default nach Plattform: Windows -> PowerShell, sonst POSIX-Shell.
  return process.platform === "win32" ? `$env:AGENT_SLOT='${slot}'` : `export AGENT_SLOT='${slot}'`;
}

async function createAgents(model) {
  if (!myWindow) return warnNotConnected();
  for (let i = 0; i < 4; i++) await createAgent(model); // 4x den zuverlaessigen Einzel-Split
}

function warnNotConnected() {
  vscode.window.showWarningMessage(
    "Agent Deck: Dieses Fenster erst im Panel verbinden (auf 'Fenster A/B' klicken, dann dieses Fenster anklicken).");
}

// ── Claude-Sessions erkennen ──────────────────────────────
let _procs = [];
let _lastScan = 0;

function scanProcs() {
  // Nur Windows; sonst greifen Name + Shell-Integration.
  if (process.platform !== "win32") return Promise.resolve([]);
  return new Promise((resolve) => {
    const psCmd = "Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,Name,CommandLine | ConvertTo-Json -Compress";
    cp.execFile("powershell.exe",
      ["-NoProfile", "-NonInteractive", "-Command", psCmd],
      { maxBuffer: 32 * 1024 * 1024, windowsHide: true, timeout: 6000 },
      (err, stdout) => resolve(err ? [] : detect.parseProcList(stdout)));
  });
}

async function getProcs(force) {
  const now = Date.now();
  if (!force && now - _lastScan < 2500) return { procs: _procs, fresh: false }; // Drosseln
  _lastScan = now;
  _procs = await scanProcs();
  return { procs: _procs, fresh: true };
}

let _syncing = false;
async function syncClaude() {
  if (!myWindow || _syncing) return;
  _syncing = true;
  try {
    const unknown = vscode.window.terminals.filter((t) => !termSlot.has(t));
    // Kandidaten fuer den Prozess-Scan: weder per Name noch per Shell-Integration
    // klar, und noch nicht per FRISCHEM Scan als "kein claude" abgehakt.
    const candidates = unknown.filter((t) =>
      !detect.looksLikeClaudeName(t.name, myWindow) && !claudeByExec.has(t) && !procChecked.has(t));
    let procs = [], fresh = false;
    if (candidates.length) ({ procs, fresh } = await getProcs(true)); // frischer Scan, wenn es ungeprueftes Terminal gibt
    for (const term of unknown) {
      const byName = detect.looksLikeClaudeName(term.name, myWindow);
      const byExec = claudeByExec.has(term);
      let byProc = false;
      if (!byName && !byExec) {
        let pid;
        try { pid = await term.processId; } catch (e) { pid = undefined; }
        byProc = pid ? detect.hasClaudeDescendant(pid, procs) : false;
      }
      if (byName || byExec || byProc) {
        ensureSlot(term, preferredName(term));
        procChecked.delete(term);
      } else if (fresh && !byName && !byExec) {
        procChecked.add(term); // nur nach ECHTEM Scan abhaken -> nicht dauernd neu scannen
      }
    }
    announce();
  } finally {
    _syncing = false;
  }
}

// ── Port-Blocker abraeumen (Ctrl+Alt+K) ───────────────────
// Braucht das Panel NICHT: der Befehl steht auch dann bereit, wenn das Deck gar
// nicht laeuft - er soll ja gerade dann helfen, wenn nichts hochkommt.

function ps(script, timeout) {
  return new Promise((resolve) => {
    cp.execFile("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", script],
      { maxBuffer: 32 * 1024 * 1024, windowsHide: true, timeout: timeout || 8000 },
      (err, stdout) => resolve(err ? null : String(stdout)));
  });
}

async function scanBlockers(all) {
  const raw = await ps(killable.PS_SCAN);
  if (raw === null) return null; // Scan selbst fehlgeschlagen != nichts gefunden
  let scan;
  try { scan = JSON.parse(raw); } catch (e) { return null; }
  return killable.portBlockers(scan, {
    all,
    selfPid: process.pid,
    // Der Broker gehoert dem Panel. Ihn abzuschiessen kappt die Bruecke, ueber die
    // dieses Fenster gerade gesteuert wird - und sieht wie ein Absturz aus.
    skipPorts: [cfg().port],
  });
}

// Noch belegte Ports zurueckmelden. Der Beweis nach dem Kill: taskkill meldet
// Erfolg, sobald es das Signal abgesetzt hat - ob der Port wirklich frei ist,
// steht damit nicht fest (der Socket kann in TIME_WAIT haengen oder ein
// Elternprozess startet das Kind neu).
async function stillListening(ports) {
  const list = ports.map(Number).filter(Number.isFinite);
  if (!list.length) return [];
  const raw = await ps(
    `$ports = ${list.join(",")}; @(Get-NetTCPConnection -State Listen -ErrorAction ` +
    "SilentlyContinue | Where-Object { $ports -contains $_.LocalPort } | " +
    "Select-Object -ExpandProperty LocalPort -Unique) | ConvertTo-Json -Compress", 5000);
  if (raw === null) return [];
  try {
    const d = JSON.parse(raw);
    return (Array.isArray(d) ? d : [d]).map(Number).filter(Number.isFinite);
  } catch (e) { return []; }
}

// /T = mitsamt Kindern: ein Dev-Server haengt oft unter npm/npx, und der Port
// gehoert dem Kind. /F, weil ein hoeflicher Abbruch bei pty-losen Prozessen
// verpufft. Nach OBEN wird nicht gekillt - da saesse die Shell des Terminals.
function killTree(pid) {
  return new Promise((resolve) => {
    cp.execFile("taskkill.exe", ["/PID", String(pid), "/T", "/F"],
      { windowsHide: true, timeout: 8000 }, (err) => resolve(!err));
  });
}

function blockerItem(e) {
  const ports = e.ports.map((p) => `:${p}`).join(" ");
  let seit = "";
  if (e.start) {
    const t = new Date(e.start);
    if (!isNaN(t)) seit = ` · seit ${t.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`;
  }
  return {
    label: `$(plug) ${ports}  ${e.name}`,
    description: `PID ${e.pid}${seit}`,
    detail: killable.shortCmd(e.cmd, 140),
    entry: e,
  };
}

async function killPortBlockers(all) {
  const liste = await scanBlockers(!!all);
  if (liste === null) {
    vscode.window.showErrorMessage(
      "Agent Deck: Prozesse liessen sich nicht auflisten (PowerShell antwortete nicht).");
    return;
  }
  if (!liste.length) {
    if (all) {
      vscode.window.showInformationMessage("Agent Deck: Kein Prozess haelt gerade einen Port.");
      return;
    }
    // Haeufig genug der wahre Fall: den Port haelt etwas AUSSERHALB von VS Code
    // (Docker, ein Dienst, ein Terminal von gestern). Statt „nichts gefunden" also
    // den naechsten Schritt anbieten.
    const wahl = await vscode.window.showInformationMessage(
      "Agent Deck: Unter diesem VS Code haelt kein Prozess einen Port.",
      "Alle Port-Lauscher zeigen");
    if (wahl) await killPortBlockers(true);
    return;
  }

  const picks = await vscode.window.showQuickPick(liste.map(blockerItem), {
    canPickMany: true,
    matchOnDescription: true,
    matchOnDetail: true,
    title: all ? "Port-Lauscher beenden (ALLE Prozesse)" : "Port-Blocker unter VS Code beenden",
    placeHolder: all
      ? "Vorsicht: hier stehen auch Dienste ausserhalb von VS Code"
      : "Mehrfachauswahl mit Leertaste · Claude-Sessions sind ausgenommen",
  });
  if (!picks || !picks.length) return;

  // Im weiten Modus koennen Systemdienste dabei sein (Docker, WSL) - da lohnt die
  // Rueckfrage. Im engen Modus ist die Liste selbst schon die Sicherung.
  if (all) {
    const namen = picks.map((p) => p.entry.name).join(", ");
    const ok = await vscode.window.showWarningMessage(
      `${picks.length} Prozess(e) beenden: ${namen}?`, { modal: true }, "Beenden");
    if (ok !== "Beenden") return;
  }

  const ports = [];
  const fehlgeschlagen = [];
  for (const p of picks) {
    ports.push(...p.entry.ports);
    if (!(await killTree(p.entry.pid))) fehlgeschlagen.push(`${p.entry.name} (PID ${p.entry.pid})`);
  }

  await new Promise((r) => setTimeout(r, 700)); // dem Socket Zeit zum Schliessen
  const belegt = await stillListening(ports);
  const frei = ports.filter((x) => !belegt.includes(x));

  if (fehlgeschlagen.length) {
    vscode.window.showErrorMessage(
      `Agent Deck: nicht beendet — ${fehlgeschlagen.join(", ")}. ` +
      "Laeuft der Prozess unter einem anderen Konto oder erhoeht?");
  } else if (belegt.length) {
    // Kein Erfolg gemeldet, wo keiner ist: der Port ist noch da, obwohl taskkill
    // zufrieden war - meist startet ein Elternprozess (nodemon, npm) das Kind neu.
    vscode.window.showWarningMessage(
      `Agent Deck: ${picks.length} beendet, aber :${belegt.join(", :")} ist weiterhin belegt ` +
      "— haelt ein Watcher den Server am Leben?");
  } else {
    vscode.window.showInformationMessage(
      `Agent Deck: ${picks.length} Prozess(e) beendet, :${[...new Set(frei)].sort((a, b) => a - b).join(", :")} frei.`);
  }
}

// ── Lifecycle ─────────────────────────────────────────────
function activate(ctx) {
  myWindow = cfg().window; // optionaler Vorbeleg; sonst kommt's per assign vom Panel
  connect();
  syncClaude();

  ctx.subscriptions.push(
    vscode.commands.registerCommand("agentDeck.createAgent", createAgent),
    vscode.commands.registerCommand("agentDeck.createAgents", createAgents),
    // Ohne Argument aufgerufen (Tastenkombi/Palette) reicht VS Code `undefined`
    // durch -> enger Modus. Der weite kommt nur aus dem Rueckfrage-Button.
    vscode.commands.registerCommand("agentDeck.killPortBlockers", () => killPortBlockers(false)),
    vscode.window.onDidOpenTerminal(() => syncClaude()),
    // Direkt in VS Code in den Pane eines Agenten geklickt -> aktives Terminal
    // wechselt. Ist es ein bekannter Slot, dem Panel als "seen" melden; das schaltet
    // dessen Kachel von "ungelesen" auf "idle" (dieselbe Geste wie ein Kachel-Klick).
    // Feuert nur bei WECHSEL des aktiven Terminals im Fenster (nicht beim blossen
    // Alt-Tab zurueck), und liefert bei "kein Terminal aktiv" undefined -> ignorieren.
    vscode.window.onDidChangeActiveTerminal((term) => {
      if (!myWindow || !term) return;
      const slot = termSlot.get(term);
      if (slot) send({ type: "seen", window: myWindow, slot });
    }),
    vscode.window.onDidCloseTerminal((term) => {
      const name = termSlot.get(term);
      if (name) { slots.delete(name); slotPid.delete(name); }
      termSlot.delete(term);
      claudeByExec.delete(term);
      procChecked.delete(term);
      writePidMap(); // geschlossene Session aus der pidmap nehmen
      announce();
    }),
  );

  // Shell-Integration: sofort erkennen, wenn irgendwo "claude" gestartet wird.
  if (vscode.window.onDidStartTerminalShellExecution) {
    ctx.subscriptions.push(vscode.window.onDidStartTerminalShellExecution((e) => {
      try {
        const cmd = e.execution && e.execution.commandLine ? e.execution.commandLine.value : "";
        if (detect.isClaudeCommand(cmd)) {
          claudeByExec.add(e.terminal);
          procChecked.delete(e.terminal);
          syncClaude();
        }
      } catch (err) { /* ignore */ }
    }));
  }

  // Auffangnetz: alle 4s ein guenstiger Sync; alle ~24s ein tiefer Re-Scan
  // (procChecked leeren), falls doch mal eine Session ohne Shell-Integration
  // nachtraeglich startet.
  let ticks = 0;
  scanTimer = setInterval(() => {
    if (++ticks % 6 === 0) procChecked.clear();
    syncClaude();
    refreshPids(); // Claude-PIDs -> pidmap (fuer den Status selbst gestarteter Sessions)
  }, 4000);
  ctx.subscriptions.push({ dispose: () => clearInterval(scanTimer) });
}

function deactivate() {
  if (reconnectTimer) clearTimeout(reconnectTimer);
  if (scanTimer) clearInterval(scanTimer);
  removePidMap(); // pidmap dieses Fensters aufraeumen
  if (sock) sock.destroy();
}

module.exports = { activate, deactivate };
