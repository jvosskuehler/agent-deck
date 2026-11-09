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
const psworker = require("./psworker");

// Ist dieses Fenster eine REMOTE-Session (SSH, WSL, Dev Container, Tunnel)? Dann
// laeuft zwar diese Extension lokal (package.json: extensionKind "ui"), die
// Terminals darin aber auf der Gegenstelle. Der Unterschied ist an einem Terminal
// nicht zu sehen und zieht sich durch die ganze Datei: eine PID von drueben ist
// hier eine FREMDE Zahl, der Prozessbaum von hier sagt ueber sie nichts, und die
// Shell drueben ist praktisch nie PowerShell. Was daran haengt, steht jeweils an
// Ort und Stelle - Anker ist diese eine Konstante.
// null = lokales Fenster, sonst der Remote-Name ("ssh-remote", "wsl", ...).
const REMOTE = vscode.env.remoteName || null;

let sock = null;
let reconnectTimer = null;
let scanTimer = null;
let myWindow = null; // "A"/"B" - via Panel (assign) oder optionalem Setting

const slots = new Map();       // slotName -> Terminal (was das Panel zeigt)
const termSlot = new Map();    // Terminal   -> slotName (Rueckrichtung, stabil)
const slotPid = new Map();     // slotName -> Claude-PID (fuer die pidmap; Status-Bruecke)
const claudeByExec = new Set(); // Terminals, in denen "claude" gestartet wurde
const procChecked = new Set();  // per Prozess-Scan geprueft + (noch) kein claude -> nicht dauernd neu scannen

// Ordner, in den auch die Hooks (deck-report) schreiben. Hierhin legen wir die
// pidmap-<Fenster>.json, damit deck-report selbst gestartete Sessions einem Slot
// zuordnen kann (siehe crates/deck-hooks/src/env.rs).
function stateDir() {
  return path.join(process.env.LOCALAPPDATA || os.homedir(), "claude-agent-deck", "state");
}

// pidmap dieses Fensters atomar schreiben: { "<claudePid>": "A1", ... }.
//
// In einem REMOTE-Fenster gar nicht: die PIDs kaemen von der Gegenstelle, die
// Datei liegt aber im LOCALAPPDATA dieses Rechners, und deck-report liest sie hier.
// Eine Remote-PID darin ist keine harmlose Nullnummer, sondern eine Verwechslung
// mit Ansage - dieselbe Zahl gehoert hier zu einem beliebigen Windows-Prozess, und
// ein LOKALER Hook, dessen Ahnenreihe zufaellig darueber laeuft, meldete seinen
// Status auf die Kachel eines fremden Fensters.
function writePidMap() {
  if (!myWindow || REMOTE) return;
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
  if (!myWindow || !slots.size || REMOTE) return;  // Remote: siehe writePidMap
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

// VS Codes Remote-Anhaengsel im Fenstertitel ("jorrit [SSH: 10.8.0.10]") gibt die
// API nicht als String heraus - wir bauen es aus der Authority nach. Es gehoert IN
// den Namen, weil der Ordnername allein zwei Gegenstellen nicht unterscheidet:
// /home/jorrit heisst auf jedem Server so. Nur Marker verwenden, die
// domain/binding._REMOTE_TAG auch wieder abstreifen kann - der Titel-Abgleich im
// Panel laeuft ueber den NACKTEN Namen, das Tag ist reine Identitaet/Anzeige.
const REMOTE_LABELS = {
  "ssh-remote": "SSH",
  "wsl": "WSL",
  "dev-container": "Dev Container",
  "attached-container": "Attached Container",
  "codespaces": "Codespaces",
  "tunnel": "Tunnel",
};

function remoteTag(uri) {
  if (!REMOTE) return "";
  const label = REMOTE_LABELS[REMOTE] || "Remote";
  // "ssh-remote+10.8.0.10" -> "10.8.0.10". Bei Dev Containers steckt dahinter
  // hex-kodiertes JSON: dann lieber nur das Label als eine Zeichenwueste. Weicht
  // der Text damit vom echten Titel ab, ist das unschaedlich (siehe oben).
  const rest = String((uri && uri.authority) || "").split("+").slice(1).join("+");
  const host = /^[0-9a-f]{16,}$/i.test(rest) ? "" : rest;
  return host ? ` [${label}: ${host}]` : ` [${label}]`;
}

function workspaceName() {
  const f = vscode.workspace.workspaceFolders;
  if (f && f.length) return f[0].name + remoteTag(f[0].uri);
  // Kein Ordner offen (leeres Fenster ODER ein .code-workspace ohne Ordner) ->
  // KEIN echter Projektname. Bewusst NICHT auf vscode.workspace.name zurueckfallen:
  // das liefert sonst "Untitled (Workspace)" o.ae. und wuerde – wie frueher das
  // literale "unknown" – als Phantom-Kachel gebunden und persistiert. null = das
  // Panel ignoriert so ein Fenster (bis ein Ordner geoeffnet wird).
  return null;
}

// Der ORDNERPFAD dieses Fensters - das, was man wieder oeffnet, wenn man das Fenster
// neu aufmachen will (ops/vscode_restart). Der Workspace-NAME identifiziert ein Fenster,
// aber er oeffnet keines: aus "orpheus-client-electron" laesst sich kein Pfad raten.
// Ein .code-workspace hat Vorrang - bei einer Mehr-Ordner-Ansicht ist die Datei das
// Fenster, der erste Ordner darin nur ein Teil davon. null = nichts zu oeffnen.
function workspaceFolder() {
  const wf = vscode.workspace.workspaceFile;
  if (wf && wf.scheme === "file") return wf.fsPath;
  // Eine .code-workspace-Datei auf der GEGENSTELLE bleibt bewusst aussen vor: zu
  // --folder-uri gibt es kein Workspace-Gegenstueck, und stattdessen ihren ersten
  // Ordner zu oeffnen waere ein ANDERES Fenster als das, das der Neustart zumacht.
  // Lieber ehrlich als uebersprungen gemeldet werden. (Den VS-Code-Neustart gibt es
  // nur noch in der Historie: `git show 2856494:deck/domain/restart_plan.py`.)
  if (wf && wf.scheme === "vscode-remote") return null;
  const f = vscode.workspace.workspaceFolders;
  const u = (f && f.length) ? f[0].uri : null;
  if (u && u.scheme === "file") return u.fsPath;
  // Remote: kein Pfad, den DIESER Rechner kennt - aber eine Adresse, unter der VS
  // Code das Fenster wieder aufmacht (`code --folder-uri vscode-remote://…`). Das
  // Panel unterschied Pfad und Adresse am Schema (`restart_plan.open_args`, s. o.).
  if (u && u.scheme === "vscode-remote") return u.toString();
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

// Slots verwerfen, deren Terminal VS Code nicht mehr fuehrt. Der Normalweg ist
// onDidCloseTerminal - der reicht aber nicht: syncClaude() und createAgent()
// arbeiten nach langen await-Pausen (Prozess-Scan bis 6 s, Split bis 3 s) mit einer
// Terminalliste von VORHER und tragen ein inzwischen geschlossenes Terminal sonst
// wieder ein. Zu dem feuert danach kein Close-Event mehr: die Kachel bliebe fuer
// immer stehen - und weil ihr Chat weg ist, laesst sie sich auch nicht mehr
// schliessen. Darum vor JEDER Meldung ans Panel aufraeumen; damit heilt sich ein
// solcher Rest spaetestens beim naechsten Sync-Tick von selbst.
function pruneDeadSlots() {
  const gone = detect.deadSlots([...slots], vscode.window.terminals);
  for (const name of gone) {
    const term = slots.get(name);
    slots.delete(name);
    slotPid.delete(name);
    if (term) {
      termSlot.delete(term);
      claudeByExec.delete(term);
      procChecked.delete(term);
    }
  }
  if (gone.length) writePidMap(); // tote Session auch aus der Status-Bruecke nehmen
  return gone.length;
}

function announce() {
  pruneDeadSlots();
  send({ type: "terminals", window: myWindow, workspace: workspaceName(),
         folder: workspaceFolder(), remote: REMOTE, slots: [...slots.keys()] });
}

function connect() {
  const { host, port } = cfg();
  sock = net.createConnection({ host, port }, () => {
    send({ type: "hello", workspace: workspaceName(), folder: workspaceFolder(),
           window: myWindow, remote: REMOTE, slots: [...slots.keys()] });
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
      createAgent(msg);
      break;
    case "createAgents":
      createAgents(msg.model);
      break;
    case "saveAll":
      // Vor dem Schliessen (Neustart): ungespeicherte Editoren machen aus closeWindow
      // einen modalen Dialog, der einen automatischen Ablauf still anhaelt.
      vscode.commands.executeCommand("workbench.action.files.saveAll");
      break;
    case "reload":
      vscode.commands.executeCommand("workbench.action.reloadWindow");
      break;
    case "closeAgent":
      // Einzelnes Agent-Terminal schliessen (beendet den pty -> auch die darin
      // laufende Claude-Session). onDidCloseTerminal raeumt Slot/pidmap auf und
      // meldet die neue Terminalliste ans Panel -> die Kachel verschwindet.
      if (t) t.dispose();
      // Und in JEDEM Fall neu melden: kennt die Extension den Slot nicht (mehr),
      // zeigt das Panel eine Kachel ohne Chat. Dann gibt es zwar nichts zu
      // schliessen, aber etwas richtigzustellen - sonst bliebe sie stehen,
      // unerreichbar ausgerechnet fuer den Knopf, der sie wegraeumen soll.
      announce();
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
// Gibt den Slot-Namen zurueck - oder null, wenn es nichts mehr zu benennen gibt.
function ensureSlot(term, preferred) {
  if (termSlot.has(term)) return termSlot.get(term);
  // Zwischen dem Einfangen des Terminals und diesem Aufruf liegen await-Pausen.
  // Wer in der Zwischenzeit geschlossen wurde, bekommt KEINE Kachel mehr: sie
  // waere eine Leiche, die kein Close-Event je wieder abraeumt (pruneDeadSlots).
  if (!vscode.window.terminals.includes(term)) return null;
  pruneDeadSlots();   // erst aufraeumen, dann fragen, welcher Name frei ist
  // Einen SCHON VERGEBENEN Namen nicht ueberschreiben: slots.set wuerde das andere
  // Terminal aus der Map draengen, waehrend termSlot es weiter kennt - es haette
  // damit keine Kachel mehr, wuerde von syncClaude nie wieder gefunden (es gilt als
  // bekannt) und sein Schliessen loeschte die Kachel des Verdraengers.
  let name = preferred;
  if (!name || slots.has(name)) {
    name = `${myWindow}${detect.nextIndex([...slots.keys()], myWindow)}`;
  }
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
// opts: { model, args, cwd, slot } - siehe crates/deck-core/src/protocol.rs (CMD_CREATE_AGENT). Nimmt auch
// noch einen nackten Modell-String an: so wurde die Funktion vorher gerufen, und die
// Kommando-Registrierung reicht bei Aufruf ueber die Palette `undefined` durch.
async function createAgent(opts) {
  if (!myWindow) return warnNotConnected();
  const o = (opts && typeof opts === "object") ? opts : { model: opts };
  const { autostart } = cfg();
  const slot = wantedSlot(o.slot) ||
    `${myWindow}${detect.nextIndex([...slots.keys()], myWindow)}`;
  const all = [...slots.values()];
  const anchor = all[all.length - 1]; // LETZTES Panel -> neuer Chat reiht sich hinten ein

  let term = null, viaSplit = false;
  if (anchor) {
    term = await splitBeside(anchor);   // als Split in die Gruppe des ersten Panels
    viaSplit = !!term;
  }
  if (!term) {
    // Erster Chat (kein Anker) oder Split fehlgeschlagen -> eigenstaendiges Terminal.
    term = vscode.window.createTerminal({
      name: slot, env: { AGENT_SLOT: slot }, cwd: o.cwd || undefined });
  }
  if (!ensureSlot(term, slot)) {
    // Direkt wieder zugemacht, bevor wir ueberhaupt loslegen konnten. Nichts
    // starten - aber melden, damit das Panel denselben Stand sieht wie wir.
    announce();
    return;
  }
  claudeByExec.add(term);
  procChecked.delete(term);
  term.show(false);
  if (autostart) {
    await new Promise((r) => setTimeout(r, 600)); // pty kurz Zeit geben
    // Split-Kommando kann kein env uebergeben -> AGENT_SLOT in der Shell setzen,
    // damit die Hooks (deck-report) den Status dieser Kachel melden koennen.
    if (viaSplit) {
      term.sendText(slotEnvCommand(slot));
      // ... und aus demselben Grund auch kein cwd: ein Split erbt das Verzeichnis
      // seines Ankers. Ein wiederaufgenommener Chat muss aber in SEINEM starten -
      // Claude findet eine Session ueber ihr Projektverzeichnis, und ein Ticket-Agent
      // lief im git worktree, nicht in der Repo-Wurzel.
      if (o.cwd) term.sendText(`cd "${o.cwd}"`);
    }
    // Fertige Flag-Zeile vom Panel (`--model "…" --effort …`, gebaut und geprueft in
    // crates/deck-core/src/agent_flags.rs) hat Vorrang. Sonst wie bisher das
    // Wunsch-Modell per CLI-Flag ERZWINGEN: das Flag hat hoechste Prioritaet und
    // schlaegt das zuletzt per /model gewaehlte, in ~/.claude.json gemerkte Modell.
    // Anfuehrungszeichen halten das '[1m]'-Suffix in PowerShell/bash literal.
    const extra = (typeof o.args === "string" && o.args.trim())
      ? ` ${o.args.trim()}`
      : (o.model ? ` --model "${o.model}"` : "");
    term.sendText(autostart + extra, true);
  }
  announce();
}

// Wunschname aus dem Panel (Neustart, damit die Kacheln wieder dieselben Chats zeigen).
// Gilt nur, wenn er zu DIESEM Fenster gehoert und noch frei ist; sonst null -> die
// Extension zaehlt selbst weiter. Ein von VS Code doch wiederhergestelltes Terminal
// fuehrt so nicht zu zwei Kacheln mit demselben Namen.
function wantedSlot(name) {
  if (typeof name !== "string" || !/^[A-Za-z]\d+$/.test(name)) return null;
  const up = name.toUpperCase();
  if (up[0] !== String(myWindow).toUpperCase()) return null;
  return slots.has(up) ? null : up;
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
  // Default nach Plattform: Windows -> PowerShell, sonst POSIX-Shell. Gefragt ist
  // die Shell des TERMINALS, nicht die des Extension-Hosts: in einem Remote-Fenster
  // laeuft diese Extension auf Windows (extensionKind "ui"), das Terminal aber
  // drueben - und Gegenstellen sind SSH/WSL/Container, also POSIX. Ein Windows-Host
  // am anderen Ende ist der seltene Fall und wird per agentDeck.slotEnvCommand gesetzt.
  const posix = REMOTE || process.platform !== "win32";
  return posix ? `export AGENT_SLOT='${slot}'` : `$env:AGENT_SLOT='${slot}'`;
}

async function createAgents(model) {
  if (!myWindow) return warnNotConnected();
  for (let i = 0; i < 4; i++) await createAgent({ model }); // 4x den zuverlaessigen Einzel-Split
}

function warnNotConnected() {
  vscode.window.showWarningMessage(
    "Agent Deck: Dieses Fenster erst im Panel verbinden (auf 'Fenster A/B' klicken, dann dieses Fenster anklicken).");
}

// ── Claude-Sessions erkennen ──────────────────────────────
let _procs = [];
let _lastScan = 0;
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
  if (REMOTE || process.platform !== "win32") return Promise.resolve([]);
  if (!procWorker) procWorker = new psworker.PsWorker();
  // null = Worker gescheitert oder Zeitlimit: wie frueher ein Fehler von execFile
  // eine leere Liste, also "nichts gefunden" statt eines halben Baums.
  return procWorker.run(psworker.PROC_SCAN, 6000)
    .then((out) => (out === null ? [] : detect.parseProcList(out)));
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
//
// Er sieht dabei immer nur DIESEN Rechner (der Scan laeuft ueber lokales
// PowerShell). In einem Remote-Fenster ist das nicht falsch, aber leicht
// misszuverstehen: wer dort einen belegten Port sucht, meint den der Gegenstelle -
// und die Liste zeigte ihm stattdessen seinen eigenen Dev-Server zum Abschiessen.
// Also dazusagen, worauf man gerade schaut, statt es raten zu lassen.
const LOKAL_HINWEIS = REMOTE ? ` · nur dieser Rechner, nicht ${REMOTE}` : "";

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
      "Agent Deck: Unter diesem VS Code haelt kein Prozess einen Port." +
      (REMOTE ? ` (Gescannt wurde dieser Rechner – die ${REMOTE}-Gegenstelle sieht der Befehl nicht.)` : ""),
      "Alle Port-Lauscher zeigen");
    if (wahl) await killPortBlockers(true);
    return;
  }

  const picks = await vscode.window.showQuickPick(liste.map(blockerItem), {
    canPickMany: true,
    matchOnDescription: true,
    matchOnDetail: true,
    title: (all ? "Port-Lauscher beenden (ALLE Prozesse)"
                : "Port-Blocker unter VS Code beenden") + LOKAL_HINWEIS,
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
    // Ueber Palette/Tastenkombi kommt ein VS-Code-eigenes Argument an -> leer aufrufen.
    vscode.commands.registerCommand("agentDeck.createAgent", () => createAgent({})),
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
  if (procWorker) { procWorker.dispose(); procWorker = null; }
  removePidMap(); // pidmap dieses Fensters aufraeumen
  if (sock) sock.destroy();
}

module.exports = { activate, deactivate };
