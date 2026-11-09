// Die Kommandos des Panels: eine Zeile vom Broker -> eine Wirkung in diesem Fenster.
//
// Die Strings sind der Vertrag mit crates/deck-core/src/protocol.rs (CMD_*); wer
// einen aendert, aendert beide. `createAgents` und `broadcast` stehen dort nicht, die
// App schickt sie heute nicht, sie bleiben fuer Handbetrieb und aeltere Panels.
//
// Ohne vscode: alles mit Wirkung kommt ueber `deck` herein (siehe extension.js),
// damit sich die Zuordnung Kommando -> Wirkung mit Attrappen pruefen laesst.
//
// deck = { book, syncClaude(), removePidMap(), createAgent(msg), createAgents(model),
//          announce(), runCommand(id) }

const panes = require("./panes");

function handleMessage(msg, deck) {
  const { book } = deck;
  const t = msg.slot ? book.slots.get(msg.slot) : null;
  switch (msg.cmd) {
    case "assign":
      book.window = msg.window;
      deck.syncClaude();
      break;
    case "unassign":
      // Panel hat die Zuordnung geloest -> Buchstabe vergessen, damit dieses Fenster
      // nicht ueber einen alten Fenster-Wert als Phantomkachel zurueckkommt.
      deck.removePidMap();
      book.window = null;
      break;
    case "focusPane":
      if (t) t.show(false); // false = Fokus nehmen (fuer Diktat)
      break;
    case "send":
      // Ziel-Pane erst fokussieren, DANN Text schicken -> landet garantiert im
      // angeklickten Agent-Chat, nicht im zuletzt fokussierten Terminal. msg.submit
      // (z.B. Ticket-Prompt) -> Text schreiben und per separatem Enter abschicken.
      if (t) panes.sendToPane(t, msg.text, msg.execute, msg.submit);
      break;
    case "key":
      if (t) panes.keyToPane(t, msg.key, msg.repeat);
      break;
    case "broadcast":
      for (const term of book.slots.values()) term.sendText(msg.text || "", msg.execute !== false);
      break;
    case "createAgent":
      deck.createAgent(msg);
      break;
    case "createAgents":
      deck.createAgents(msg.model);
      break;
    case "saveAll":
      // Vor dem Schliessen (Neustart): ungespeicherte Editoren machen aus closeWindow
      // einen modalen Dialog, der einen automatischen Ablauf still anhaelt.
      deck.runCommand("workbench.action.files.saveAll");
      break;
    case "reload":
      deck.runCommand("workbench.action.reloadWindow");
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
      deck.announce();
      break;
    case "closeWindow":
      // Ganzes VS-Code-Fenster schliessen (schliesst zwangslaeufig alle Agenten
      // darin mit). Beim letzten Fenster beendet VS Code sich; ungespeicherte
      // Editoren fragt VS Code selbst ab.
      deck.runCommand("workbench.action.closeWindow");
      break;
  }
}

module.exports = { handleMessage };
