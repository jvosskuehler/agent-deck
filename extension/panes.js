// Eingaben in einen einzelnen Split-Pane: Text, Prompt mit Abschicken, Tasten.
//
// Arbeitet nur auf dem Terminal-Objekt (show, sendText), ohne vscode zu laden. Ein
// VS-Code-Fenster hat keine HWNDs je Pane; sendText schreibt direkt in den pty des
// richtigen Panes, und genau darum gibt es die Extension ueberhaupt.

// Panel-Taste -> Byte-Sequenz an den pty. Bare Keys immer mit sendText(seq,false),
// sonst haengt VS Code ein \r an und submitted zu frueh. Gegenstueck auf der
// Rust-Seite: deck-core::wt (Tasten fuer Windows Terminal).
const KEYMAP = {
  enter: "\r", esc: "\x1b", "ctrl-c": "\x03",
  up: "\x1b[A", down: "\x1b[B", right: "\x1b[C", left: "\x1b[D",
  tab: "\t", "shift-tab": "\x1b[Z", // back-tab: Claude Code schaltet damit den Permission-Mode um
  "1": "1", "2": "2", "3": "3", "4": "4", "5": "5",
};

// Verzoegerung (ms) zwischen Text und dem separaten Enter im submit-Pfad. Claude
// Code erkennt einen grossen Input-Burst als PASTE und schluckt ein im selben
// sendText angehaengtes \r in diese Erkennung -> der Prompt bliebe im Eingabefeld
// stehen. Darum: Text OHNE Newline schreiben, kurz warten bis die Paste-Erkennung
// durch ist, DANN ein eigenstaendiges Enter zum Abschicken. Grosszuegig gewaehlt
// (unmerklich fuer den Nutzer), damit auch ein langsam gerenderter pty sicher folgt.
const SUBMIT_ENTER_DELAY_MS = 450;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// Tastensequenz (evtl. mehrfach) an den pty schicken. Zwischen Wiederholungen
// kurz warten, damit die Claude-TUI jeden Schritt (z.B. Moduswechsel) registriert.
async function sendKey(term, key, repeat) {
  const seq = (key in KEYMAP) ? KEYMAP[key] : key;
  const n = Math.max(1, repeat || 1);
  for (let i = 0; i < n; i++) {
    term.sendText(seq, false);
    if (i < n - 1) await sleep(90);
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
    await sleep(80);
  } catch (e) { /* ignore */ }
}

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
      await sleep(SUBMIT_ENTER_DELAY_MS);
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

module.exports = { sendToPane, keyToPane };
