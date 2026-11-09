// Die Leitung zum Deck: TCP zum Broker der App, newline-getrenntes JSON in beide
// Richtungen (Vertrag: crates/deck-core/src/protocol.rs, Gegenstelle
// app/src-tauri/src/broker.rs).
//
// Die Extension ist der Client und haelt die Verbindung von sich aus: laeuft das Deck
// noch nicht oder startet es neu, versucht sie es alle 2 s wieder. Beim Verbinden
// meldet sie sich mit `hello`, alles Weitere kommt als Kommando-Zeile herein.
//
// Nur `net`, kein vscode: Adresse, Erstmeldung und Empfaenger kommen als Funktionen
// herein.

const net = require("net");

const RECONNECT_MS = 2000;

// Alle vollstaendigen Zeilen aus dem bisher Gelesenen loesen; der Rest (eine halbe
// Zeile) wartet auf den naechsten Datenblock.
function splitLines(buf) {
  const lines = [];
  let rest = String(buf || "");
  let i;
  while ((i = rest.indexOf("\n")) >= 0) {
    lines.push(rest.slice(0, i));
    rest = rest.slice(i + 1);
  }
  return { lines, rest };
}

// opts.address():   { host, port } - bei jedem Verbindungsversuch neu gelesen
// opts.hello():     die Erstmeldung nach dem Verbinden
// opts.onMessage(m): eine empfangene Zeile, schon als Objekt
function createBroker(opts) {
  let sock = null;
  let reconnectTimer = null;
  let disposed = false;

  function send(obj) {
    if (sock && !sock.destroyed) {
      try { sock.write(JSON.stringify(obj) + "\n"); } catch (e) { /* ignore */ }
    }
  }

  function connect() {
    const { host, port } = opts.address();
    sock = net.createConnection({ host, port }, () => send(opts.hello()));
    let buf = "";
    sock.on("data", (d) => {
      const { lines, rest } = splitLines(buf + d.toString("utf8"));
      buf = rest;
      for (const line of lines) {
        if (!line.trim()) continue;
        try { opts.onMessage(JSON.parse(line)); } catch (e) { /* kaputte Zeile ignorieren */ }
      }
    });
    sock.on("error", () => { /* Panel evtl. noch nicht da -> reconnect via close */ });
    sock.on("close", scheduleReconnect);
  }

  function scheduleReconnect() {
    // Nach dispose() nicht mehr: das Schliessen der eigenen Verbindung loest genau
    // dieses close aus, und ein Wiederverbinden danach hielte einen beendeten
    // Extension-Host (oder einen Testlauf) am Leben.
    if (reconnectTimer || disposed) return;
    reconnectTimer = setTimeout(() => { reconnectTimer = null; connect(); }, RECONNECT_MS);
  }

  function dispose() {
    disposed = true;
    if (reconnectTimer) clearTimeout(reconnectTimer);
    reconnectTimer = null;
    if (sock) sock.destroy();
  }

  return { connect, send, dispose };
}

module.exports = { createBroker, splitLines };
