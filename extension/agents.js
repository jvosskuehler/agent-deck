// Neue Claude-Terminals anlegen: als Split neben die vorhandenen, mit AGENT_SLOT,
// damit die Hooks (deck-report) den Status der richtigen Kachel melden.
//
// vscode kommt ueber `deck` herein (siehe extension.js), so laedt das Modul auch
// ohne VS Code.
// deck = { vscode, book, remote, cfg(), announce() }

function createAgentOps(deck) {
  const { vscode, book } = deck;

  // opts: { model, args, cwd, slot } - siehe crates/deck-core/src/protocol.rs (CMD_CREATE_AGENT). Nimmt auch
  // noch einen nackten Modell-String an: so wurde die Funktion vorher gerufen, und die
  // Kommando-Registrierung reicht bei Aufruf ueber die Palette `undefined` durch.
  async function createAgent(opts) {
    if (!book.window) return warnNotConnected();
    const o = (opts && typeof opts === "object") ? opts : { model: opts };
    const { autostart } = deck.cfg();
    const slot = book.wantedSlot(o.slot) || book.nextName();
    const all = [...book.slots.values()];
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
    if (!book.ensure(term, slot)) {
      // Direkt wieder zugemacht, bevor wir ueberhaupt loslegen konnten. Nichts
      // starten - aber melden, damit das Panel denselben Stand sieht wie wir.
      deck.announce();
      return;
    }
    book.claudeByExec.add(term);
    book.procChecked.delete(term);
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
    deck.announce();
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
    const tmpl = deck.cfg().slotEnvCommand;
    if (tmpl) return tmpl.replace(/\{slot\}/g, slot);
    // Default nach Plattform: Windows -> PowerShell, sonst POSIX-Shell. Gefragt ist
    // die Shell des TERMINALS, nicht die des Extension-Hosts: in einem Remote-Fenster
    // laeuft diese Extension auf Windows (extensionKind "ui"), das Terminal aber
    // drueben - und Gegenstellen sind SSH/WSL/Container, also POSIX. Ein Windows-Host
    // am anderen Ende ist der seltene Fall und wird per agentDeck.slotEnvCommand gesetzt.
    const posix = deck.remote || process.platform !== "win32";
    return posix ? `export AGENT_SLOT='${slot}'` : `$env:AGENT_SLOT='${slot}'`;
  }

  async function createAgents(model) {
    if (!book.window) return warnNotConnected();
    for (let i = 0; i < 4; i++) await createAgent({ model }); // 4x den zuverlaessigen Einzel-Split
  }

  function warnNotConnected() {
    vscode.window.showWarningMessage(
      "Agent Deck: Dieses Fenster erst im Panel verbinden (auf 'Fenster A/B' klicken, dann dieses Fenster anklicken).");
  }

  return { createAgent, createAgents };
}

module.exports = { createAgentOps };
