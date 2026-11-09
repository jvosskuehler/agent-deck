// Die Slot-Buchfuehrung dieses Fensters: welcher Slot-Name (A1, A2, ...) gehoert zu
// welchem Terminal. Gemeldet wird ans Panel genau diese Map, und was darin steht, ist
// eine Kachel.
//
// Ohne vscode: die Liste der lebenden Terminals (vscode.window.terminals) kommt ueber
// `live()` herein, Terminals sind fuer die Buchfuehrung undurchsichtige Referenzen.

const detect = require("./detect");

class SlotBook {
  // opts.live:      () => Terminals, die VS Code gerade fuehrt
  // opts.onPruned:  tote Slots abgeraeumt (die pidmap muss nach)
  // opts.onNewSlot: neuer Slot vergeben (dessen Claude-PID gleich aufloesen)
  constructor(opts) {
    const o = opts || {};
    this._live = o.live || (() => []);
    this._onPruned = o.onPruned || (() => {});
    this._onNewSlot = o.onNewSlot || (() => {});
    this.window = null;              // "A"/"B" - via Panel (assign) oder optionalem Setting
    this.slots = new Map();          // slotName -> Terminal (was das Panel zeigt)
    this.termSlot = new Map();       // Terminal   -> slotName (Rueckrichtung, stabil)
    this.slotPid = new Map();        // slotName -> Claude-PID (fuer die pidmap; Status-Bruecke)
    this.claudeByExec = new Set();   // Terminals, in denen "claude" gestartet wurde
    this.procChecked = new Set();    // per Prozess-Scan geprueft + (noch) kein claude -> nicht dauernd neu scannen
  }

  names() { return [...this.slots.keys()]; }

  // Naechster freier Name in diesem Fenster (hinter dem hoechsten vergebenen Index).
  nextName() { return `${this.window}${detect.nextIndex(this.names(), this.window)}`; }

  // Slots verwerfen, deren Terminal VS Code nicht mehr fuehrt. Der Normalweg ist
  // onDidCloseTerminal - der reicht aber nicht: syncClaude() und createAgent()
  // arbeiten nach langen await-Pausen (Prozess-Scan bis 6 s, Split bis 3 s) mit einer
  // Terminalliste von VORHER und tragen ein inzwischen geschlossenes Terminal sonst
  // wieder ein. Zu dem feuert danach kein Close-Event mehr: die Kachel bliebe fuer
  // immer stehen - und weil ihr Chat weg ist, laesst sie sich auch nicht mehr
  // schliessen. Darum vor JEDER Meldung ans Panel aufraeumen; damit heilt sich ein
  // solcher Rest spaetestens beim naechsten Sync-Tick von selbst.
  prune() {
    const gone = detect.deadSlots([...this.slots], this._live());
    for (const name of gone) {
      const term = this.slots.get(name);
      this.slots.delete(name);
      this.slotPid.delete(name);
      if (term) {
        this.termSlot.delete(term);
        this.claudeByExec.delete(term);
        this.procChecked.delete(term);
      }
    }
    if (gone.length) this._onPruned(); // tote Session auch aus der Status-Bruecke nehmen
    return gone.length;
  }

  // Gibt den Slot-Namen zurueck - oder null, wenn es nichts mehr zu benennen gibt.
  ensure(term, preferred) {
    if (this.termSlot.has(term)) return this.termSlot.get(term);
    // Zwischen dem Einfangen des Terminals und diesem Aufruf liegen await-Pausen.
    // Wer in der Zwischenzeit geschlossen wurde, bekommt KEINE Kachel mehr: sie
    // waere eine Leiche, die kein Close-Event je wieder abraeumt (prune).
    if (!this._live().includes(term)) return null;
    this.prune();   // erst aufraeumen, dann fragen, welcher Name frei ist
    // Einen SCHON VERGEBENEN Namen nicht ueberschreiben: slots.set wuerde das andere
    // Terminal aus der Map draengen, waehrend termSlot es weiter kennt - es haette
    // damit keine Kachel mehr, wuerde von syncClaude nie wieder gefunden (es gilt als
    // bekannt) und sein Schliessen loeschte die Kachel des Verdraengers.
    let name = preferred;
    if (!name || this.slots.has(name)) name = this.nextName();
    this.slots.set(name, term);
    this.termSlot.set(term, name);
    this._onNewSlot();
    return name;
  }

  // Terminal geschlossen (onDidCloseTerminal): alles vergessen, was an ihm hing.
  forget(term) {
    const name = this.termSlot.get(term);
    if (name) { this.slots.delete(name); this.slotPid.delete(name); }
    this.termSlot.delete(term);
    this.claudeByExec.delete(term);
    this.procChecked.delete(term);
  }

  // Deck-eigenen Namen (A1, A2, ...) bevorzugen, sonst automatisch vergeben.
  preferredName(term) {
    const re = new RegExp(`^${this.window}\\d+$`, "i");
    return term.name && re.test(term.name) ? term.name : null;
  }

  // Wunschname aus dem Panel (Neustart, damit die Kacheln wieder dieselben Chats
  // zeigen). Gilt nur, wenn er zu DIESEM Fenster gehoert und noch frei ist; sonst
  // null -> die Extension zaehlt selbst weiter. Ein von VS Code doch
  // wiederhergestelltes Terminal fuehrt so nicht zu zwei Kacheln mit demselben Namen.
  wantedSlot(name) {
    if (typeof name !== "string" || !/^[A-Za-z]\d+$/.test(name)) return null;
    const up = name.toUpperCase();
    if (up[0] !== String(this.window).toUpperCase()) return null;
    return this.slots.has(up) ? null : up;
  }
}

module.exports = { SlotBook };
