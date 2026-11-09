// Agent Deck Bridge - reine JS-VS-Code-Extension (kein Build-Schritt).
//
// Verbindet sich mit dem Agent-Deck-Panel (TCP, newline-getrenntes JSON) und
// steuert die Claude-Code-Terminals DIESES Fensters:
//   - focusPane: exakten Split-Pane fokussieren  (terminal.show)
//   - send/key:  Text/Steuerzeichen in den pty schreiben  (terminal.sendText)
//   - broadcast: an alle Terminals dieses Fensters
//   - createAgent(s): neue Claude-Terminals anlegen (mit AGENT_SLOT)
//   - assign:    das Panel weist diesem Fenster den Buchstaben A/B zu
// Dazu ein Befehl, der ohne das Panel auskommt (Ctrl+Alt+K): Port-Blocker abraeumen.
//
// Warum eine Extension? Ein VS-Code-Fenster ist EIN Chromium-Fenster ohne
// per-Pane-HWNDs - Win32/SendInput kann einen einzelnen Split-Pane nicht
// treffen. sendText schreibt direkt in den pty des richtigen Panes, ohne Fokus
// zu klauen.
//
// Diese Datei verdrahtet nur: sie ist die EINZIGE, die `vscode` laedt, und reicht es
// den anderen Modulen ueber das `deck`-Objekt weiter. So laedt jedes andere Modul
// auch unter Node allein, und die reinen lassen sich dort pruefen (extension/test).
//   broker.js     Leitung zum Deck (TCP, Wiederverbinden)
//   commands.js   Kommando des Panels -> Wirkung
//   slots.js      Slot-Buchfuehrung (Name <-> Terminal)
//   claudesync.js Claude-Sessions erkennen, Claude-PIDs aufloesen
//   agents.js     neue Agent-Terminals anlegen
//   panes.js      Text und Tasten in einen Pane
//   pidmap.js     Status-Bruecke zu deck-report
//   workspace.js  Name und Ordner dieses Fensters
//   portkill.js   Ctrl+Alt+K (Regeln in killable.js)
// ACHTUNG beim Hinzufuegen eines Moduls: es muss direkt in extension/ liegen.
// deck-services::vscode_ext kopiert nur die Dateien dieser Ebene, und fehlt eine,
// laedt VS Code die Extension gar nicht.

const vscode = require("vscode");
const detect = require("./detect");
const pidmap = require("./pidmap");
const workspace = require("./workspace");
const { SlotBook } = require("./slots");
const { createBroker } = require("./broker");
const { handleMessage } = require("./commands");
const { createClaudeSync } = require("./claudesync");
const { createAgentOps } = require("./agents");
const { createPortKill } = require("./portkill");

// Ist dieses Fenster eine REMOTE-Session (SSH, WSL, Dev Container, Tunnel)? Dann
// laeuft zwar diese Extension lokal (package.json: extensionKind "ui"), die
// Terminals darin aber auf der Gegenstelle. Der Unterschied ist an einem Terminal
// nicht zu sehen und zieht sich durch alle Module: eine PID von drueben ist
// hier eine FREMDE Zahl, der Prozessbaum von hier sagt ueber sie nichts, und die
// Shell drueben ist praktisch nie PowerShell. Was daran haengt, steht jeweils an
// Ort und Stelle - Anker ist diese eine Konstante (als `deck.remote` weitergereicht).
// null = lokales Fenster, sonst der Remote-Name ("ssh-remote", "wsl", ...).
const REMOTE = vscode.env.remoteName || null;

let scanTimer = null;
let sync = null;

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

const book = new SlotBook({
  live: () => vscode.window.terminals,
  onPruned: () => writePidMap(),
  onNewSlot: () => { if (sync) sync.resetPidRefresh(); },
});

// pidmap dieses Fensters schreiben: { "<claudePid>": "A1", ... }.
//
// In einem REMOTE-Fenster gar nicht: die PIDs kaemen von der Gegenstelle, die
// Datei liegt aber im LOCALAPPDATA dieses Rechners, und deck-report liest sie hier.
// Eine Remote-PID darin ist keine harmlose Nullnummer, sondern eine Verwechslung
// mit Ansage - dieselbe Zahl gehoert hier zu einem beliebigen Windows-Prozess, und
// ein LOKALER Hook, dessen Ahnenreihe zufaellig darueber laeuft, meldete seinen
// Status auf die Kachel eines fremden Fensters.
function writePidMap() {
  if (!book.window || REMOTE) return;
  pidmap.writePidMap(pidmap.stateDir(), book.window, book.slotPid);
}

function removePidMap() {
  if (!book.window) return;
  pidmap.removePidMap(pidmap.stateDir(), book.window);
}

const workspaceName = () => workspace.workspaceName(REMOTE, vscode.workspace.workspaceFolders);
const workspaceFolder = () =>
  workspace.workspaceFolder(vscode.workspace.workspaceFile, vscode.workspace.workspaceFolders);

function announce() {
  book.prune();
  broker.send({ type: "terminals", window: book.window, workspace: workspaceName(),
                folder: workspaceFolder(), remote: REMOTE, slots: book.names() });
}

const broker = createBroker({
  address: () => cfg(),
  hello: () => ({ type: "hello", workspace: workspaceName(), folder: workspaceFolder(),
                  window: book.window, remote: REMOTE, slots: book.names() }),
  onMessage: (msg) => handleMessage(msg, deck),
});

// Alles, was die anderen Module von diesem Fenster brauchen.
const deck = {
  vscode, book, cfg, announce, writePidMap, removePidMap,
  remote: REMOTE,
  liveTerminals: () => vscode.window.terminals,
  runCommand: (id) => vscode.commands.executeCommand(id),
  syncClaude: () => sync.syncClaude(),
  createAgent: (msg) => agents.createAgent(msg),
  createAgents: (model) => agents.createAgents(model),
};
sync = createClaudeSync(deck);
const agents = createAgentOps(deck);
const portKill = createPortKill(deck);

// ── Lifecycle ─────────────────────────────────────────────
function activate(ctx) {
  book.window = cfg().window; // optionaler Vorbeleg; sonst kommt's per assign vom Panel
  broker.connect();
  sync.syncClaude();

  ctx.subscriptions.push(
    // Ueber Palette/Tastenkombi kommt ein VS-Code-eigenes Argument an -> leer aufrufen.
    vscode.commands.registerCommand("agentDeck.createAgent", () => agents.createAgent({})),
    vscode.commands.registerCommand("agentDeck.createAgents", agents.createAgents),
    // Ohne Argument aufgerufen (Tastenkombi/Palette) reicht VS Code `undefined`
    // durch -> enger Modus. Der weite kommt nur aus dem Rueckfrage-Button.
    vscode.commands.registerCommand("agentDeck.killPortBlockers", () => portKill.killPortBlockers(false)),
    vscode.window.onDidOpenTerminal(() => sync.syncClaude()),
    // Direkt in VS Code in den Pane eines Agenten geklickt -> aktives Terminal
    // wechselt. Ist es ein bekannter Slot, dem Panel als "seen" melden; das schaltet
    // dessen Kachel von "ungelesen" auf "idle" (dieselbe Geste wie ein Kachel-Klick).
    // Feuert nur bei WECHSEL des aktiven Terminals im Fenster (nicht beim blossen
    // Alt-Tab zurueck), und liefert bei "kein Terminal aktiv" undefined -> ignorieren.
    vscode.window.onDidChangeActiveTerminal((term) => {
      if (!book.window || !term) return;
      const slot = book.termSlot.get(term);
      if (slot) broker.send({ type: "seen", window: book.window, slot });
    }),
    vscode.window.onDidCloseTerminal((term) => {
      book.forget(term);
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
          book.claudeByExec.add(e.terminal);
          book.procChecked.delete(e.terminal);
          sync.syncClaude();
        }
      } catch (err) { /* ignore */ }
    }));
  }

  // Auffangnetz: alle 4s ein guenstiger Sync; alle ~24s ein tiefer Re-Scan
  // (procChecked leeren), falls doch mal eine Session ohne Shell-Integration
  // nachtraeglich startet.
  let ticks = 0;
  scanTimer = setInterval(() => {
    if (++ticks % 6 === 0) book.procChecked.clear();
    sync.syncClaude();
    sync.refreshPids(); // Claude-PIDs -> pidmap (fuer den Status selbst gestarteter Sessions)
  }, 4000);
  ctx.subscriptions.push({ dispose: () => clearInterval(scanTimer) });
}

function deactivate() {
  if (scanTimer) clearInterval(scanTimer);
  sync.dispose();
  removePidMap(); // pidmap dieses Fensters aufraeumen
  broker.dispose();
}

module.exports = { activate, deactivate };
