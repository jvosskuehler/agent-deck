// Ein dauerhaft laufender PowerShell-Prozess fuer die Prozess-Scans der Extension.
//
// Warum: Die Claude-Erkennung braucht den Prozessbaum, und den liefert unter
// Windows nur WMI. Frueher startete dafuer JEDER Scan eine neue powershell.exe,
// und gescannt wird pro Fenster alle 4 bis 8 s (syncClaude, refreshPids).
// Gemessen am 2026-10-14: ein Scan kostete 2,6 s Laufzeit und 1,2 s CPU, fast
// alles fuer den Start von PowerShell selbst, dazu jedes Mal ein conhost. Und
// Defender prueft jeden neuen Prozess, das kam noch obendrauf. Der Worker startet
// einmal und nimmt danach Anfragen ueber stdin entgegen.
//
// Protokoll, je Anfrage eine Zeile hin und ein Rumpf mit Endzeile zurueck:
//   hin:     <id> <base64(utf8-Skript)>\n
//   zurueck: <Ausgabe des Skripts>\n<MARKE> <id> ok|err\n
// Base64, weil ein Skript Zeilenumbrueche und Anfuehrungszeichen enthalten darf,
// genau die Zeichen, die sonst unterwegs verbogen werden (CLAUDE.md, Fallen). Die
// Marke traegt eine Zufallszahl je Worker-Start: eine Kommandozeile im Scan kann
// beliebigen Text enthalten, aber nicht diese Zahl.
//
// Reine Protokoll-Logik (encodeRequest, takeResponses) ist ohne Prozess pruefbar,
// die Klasse selbst braucht nur child_process - beides pruefte in der Python-Fassung
// tests/test_extension_psworker.py ueber node, ohne vscode
// (`git show 2856494:tests/test_extension_psworker.py`).

const cp = require("child_process");
const crypto = require("crypto");

// Die Abfrage fuer die Claude-Erkennung (detect.parseProcList liest sie). Steht
// hier, damit der Test genau dieses Snippet durch einen echten Worker schickt.
// `-Property` laesst WMI nur die vier Spalten holen, statt jedes Prozessobjekt
// voll zu befuellen; Select-Object streift danach die CIM-Metadaten ab, damit das
// JSON dieselbe Form hat wie vorher.
const PROC_SCAN =
  "Get-CimInstance Win32_Process -Property ProcessId,ParentProcessId,Name,CommandLine | " +
  "Select-Object ProcessId,ParentProcessId,Name,CommandLine | ConvertTo-Json -Compress";

// Obergrenze fuer eine Antwort, wie vorher maxBuffer beim Einzelaufruf. Waechst der
// Puffer darueber, stimmt etwas nicht, und ein frischer Worker ist die Heilung.
const MAX_CHARS = 32 * 1024 * 1024;

// Die Schleife im Worker. Sie fuehrt nur aus, was die Extension schickt; ihr
// eigener Zustand ist absichtlich minimal. Kommt das Ende von stdin (Extension
// beendet oder Fenster zu), endet sie, so bleibt kein Waisenprozess zurueck.
//
// UTF-8 OHNE BOM: ohne die OutputEncoding-Zeile schreibt Windows PowerShell 5.1 in
// der Konsolen-Codepage, und aus den Umlauten im Benutzerpfad wird Mojibake
// (CLAUDE.md, Falle zur Codepage). [Text.Encoding]::UTF8 setzte ein BOM davor.
function workerScript(mark) {
  return [
    "$ProgressPreference = 'SilentlyContinue'",
    "$utf8 = New-Object System.Text.UTF8Encoding $false",
    "[Console]::OutputEncoding = $utf8",
    "$in = [Console]::In",
    "while ($true) {",
    "  $line = $in.ReadLine()",
    "  if ($null -eq $line) { break }",
    "  $sp = $line.IndexOf(' ')",
    "  if ($sp -lt 1) { continue }",
    "  $id = $line.Substring(0, $sp)",
    "  $status = 'ok'",
    "  $text = ''",
    "  try {",
    "    $code = $utf8.GetString([Convert]::FromBase64String($line.Substring($sp + 1)))",
    "    $text = (& ([scriptblock]::Create($code)) | ForEach-Object { [string]$_ }) -join \"`n\"",
    "  } catch {",
    "    $status = 'err'",
    "    $text = ''",
    "  }",
    "  $Error.Clear()",
    "  $out = [Console]::Out",
    "  $out.Write($text)",
    `  $out.Write("\`n" + '${mark}' + ' ' + $id + ' ' + $status + "\`n")`,
    "  $out.Flush()",
    "}",
  ].join("\n");
}

// -EncodedCommand erwartet UTF-16LE in Base64. So kommt die Schleife ohne ein
// einziges Quoting-Problem in die Kommandozeile.
function encodeCommand(script) {
  return Buffer.from(String(script), "utf16le").toString("base64");
}

function encodeRequest(id, script) {
  return `${id} ${Buffer.from(String(script), "utf8").toString("base64")}\n`;
}

// Alle vollstaendigen Antworten aus dem bisher gelesenen Text loesen. Was danach
// uebrig bleibt (eine halbe Antwort), kommt als `rest` zurueck und wartet auf den
// naechsten Datenblock. Ein BOM am Anfang wird verworfen, falls PowerShell doch
// eines schreibt; einen Rumpf, der so beginnt, gibt es nicht.
function takeResponses(buf, mark) {
  let text = String(buf || "");
  if (text.charCodeAt(0) === 0xfeff) text = text.slice(1);
  const tag = `\n${mark} `;
  const done = [];
  let start = 0;
  for (;;) {
    const at = text.indexOf(tag, start);
    if (at < 0) break;
    const eol = text.indexOf("\n", at + tag.length);
    if (eol < 0) break; // Endzeile noch nicht ganz da
    const [id, status] = text.slice(at + tag.length, eol).trim().split(" ");
    done.push({ id, ok: status === "ok", body: text.slice(start, at) });
    start = eol + 1;
  }
  return { done, rest: text.slice(start) };
}

class PsWorker {
  // opts.exe: das PowerShell-Programm (Test: ein anderes); opts.idleMs: nach so
  // langer Ruhe wird der Worker beendet und beim naechsten Scan neu gestartet,
  // damit ein Fenster ohne Scans keinen Prozess festhaelt. 0 = nie.
  constructor(opts) {
    const o = opts || {};
    this._exe = o.exe || "powershell.exe";
    this._idleMs = o.idleMs != null ? o.idleMs : 120000;
    this._proc = null;
    this._mark = null;
    this._buf = "";
    this._wait = null;           // laufende Anfrage: { id, resolve, timer }
    this._seq = 0;
    this._chain = Promise.resolve();
    this._idle = null;
    this.starts = 0;             // wie oft ein Prozess gestartet wurde (Test, Messung)
  }

  get pid() { return this._proc ? this._proc.pid : null; }

  // Skript ausfuehren, Ausgabe als Text. null = fehlgeschlagen oder Zeitlimit,
  // dasselbe Signal wie frueher ein Fehler von execFile. Anfragen laufen strikt
  // nacheinander: der Worker hat nur eine Leitung.
  run(script, timeoutMs) {
    const job = this._chain.then(() => this._runOne(script, timeoutMs || 6000));
    this._chain = job.catch(() => {});
    return job;
  }

  dispose() {
    clearTimeout(this._idle);
    const p = this._proc;
    this._proc = null;
    this._finish(null);
    if (!p) return;
    // Hoeflich: stdin schliessen, dann endet die Schleife von selbst. Haengt sie
    // gerade in einer Abfrage, raeumt der Nachschlag nach einer Sekunde ab.
    try { p.stdin.end(); } catch (e) { /* schon zu */ }
    const t = setTimeout(() => { try { p.kill(); } catch (e) { /* schon weg */ } }, 1000);
    if (t.unref) t.unref();
  }

  _start() {
    this._mark = `<<agent-deck-ps:${crypto.randomBytes(8).toString("hex")}>>`;
    this._buf = "";
    const p = cp.spawn(this._exe,
      ["-NoLogo", "-NoProfile", "-NonInteractive", "-EncodedCommand",
        encodeCommand(workerScript(this._mark))],
      { windowsHide: true, stdio: ["pipe", "pipe", "ignore"] });
    this.starts++;
    p.stdout.setEncoding("utf8"); // haelt Umlaute ganz, die ueber zwei Bloecke reichen
    p.stdout.on("data", (d) => this._onData(p, d));
    p.on("error", () => this._gone(p));
    p.on("exit", () => this._gone(p));
    p.stdin.on("error", () => { /* EPIPE nach Absturz: exit raeumt auf */ });
    this._proc = p;
  }

  _runOne(script, timeoutMs) {
    clearTimeout(this._idle);
    return new Promise((resolve) => {
      if (!this._proc) {
        try { this._start(); } catch (e) { resolve(null); return; }
      }
      const id = String(++this._seq);
      // Zeitlimit = der Worker haengt. Nicht weiter auf ihn warten, sondern ihn
      // beenden; die naechste Anfrage startet einen frischen.
      const timer = setTimeout(() => { this._finish(null); this._kill(); }, timeoutMs);
      this._wait = { id, resolve, timer };
      try {
        this._proc.stdin.write(encodeRequest(id, script));
      } catch (e) {
        this._finish(null);
        this._kill();
      }
    }).then((value) => { this._armIdle(); return value; });
  }

  _onData(p, chunk) {
    if (p !== this._proc) return; // Nachzuegler eines schon ersetzten Workers
    this._buf += chunk;
    if (this._buf.length > MAX_CHARS) { this._finish(null); this._kill(); return; }
    const { done, rest } = takeResponses(this._buf, this._mark);
    this._buf = rest;
    for (const r of done) {
      if (this._wait && r.id === this._wait.id) this._finish(r.ok ? r.body : null);
    }
  }

  _gone(p) {
    if (p !== this._proc) return;
    this._proc = null;
    this._finish(null); // abgestuerzt mitten in einer Anfrage: die bekommt null
  }

  _finish(value) {
    const w = this._wait;
    if (!w) return;
    this._wait = null;
    clearTimeout(w.timer);
    w.resolve(value);
  }

  _kill() {
    const p = this._proc;
    this._proc = null;
    if (p) { try { p.kill(); } catch (e) { /* schon weg */ } }
  }

  _armIdle() {
    if (!this._idleMs || !this._proc) return;
    clearTimeout(this._idle);
    this._idle = setTimeout(() => this.dispose(), this._idleMs);
    if (this._idle.unref) this._idle.unref();
  }
}

module.exports = { PsWorker, PROC_SCAN, workerScript, encodeCommand, encodeRequest, takeResponses };
