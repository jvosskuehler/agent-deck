// Port-Blocker abraeumen (Ctrl+Alt+K): Auswahldialog, taskkill, Nachweis.
//
// Wer angeboten werden DARF, entscheidet killable.js (rein, getestet); hier stehen
// nur Abfrage, Dialog und der Eingriff. Braucht das Panel NICHT: der Befehl steht
// auch dann bereit, wenn das Deck gar nicht laeuft - er soll ja gerade dann helfen,
// wenn nichts hochkommt.
//
// vscode kommt ueber `deck` herein (siehe extension.js). Bewusst ohne Test: alles
// hier beendet echte Prozesse (CLAUDE.md, Fallen).
// deck = { vscode, remote, cfg() }

const cp = require("child_process");
const killable = require("./killable");

function ps(script, timeout) {
  return new Promise((resolve) => {
    cp.execFile("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", script],
      { maxBuffer: 32 * 1024 * 1024, windowsHide: true, timeout: timeout || 8000 },
      (err, stdout) => resolve(err ? null : String(stdout)));
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

function createPortKill(deck) {
  const { vscode, remote } = deck;

  // Er sieht immer nur DIESEN Rechner (der Scan laeuft ueber lokales PowerShell). In
  // einem Remote-Fenster ist das nicht falsch, aber leicht misszuverstehen: wer dort
  // einen belegten Port sucht, meint den der Gegenstelle - und die Liste zeigte ihm
  // stattdessen seinen eigenen Dev-Server zum Abschiessen. Also dazusagen, worauf
  // man gerade schaut, statt es raten zu lassen.
  const LOKAL_HINWEIS = remote ? ` · nur dieser Rechner, nicht ${remote}` : "";

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
      skipPorts: [deck.cfg().port],
    });
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
        (remote ? ` (Gescannt wurde dieser Rechner – die ${remote}-Gegenstelle sieht der Befehl nicht.)` : ""),
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

  return { killPortBlockers };
}

module.exports = { createPortKill };
