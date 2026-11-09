// Wie sich dieses Fenster beim Panel ausweist: Name (mit Remote-Anhaengsel) und der
// Ordner, unter dem es sich wieder oeffnen laesst.
//
// Rein: die Werte aus vscode.env/vscode.workspace kommen als Argumente herein, damit
// die Regeln ohne VS Code pruefbar sind. `remote` ist vscode.env.remoteName oder null.

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

function remoteTag(remote, uri) {
  if (!remote) return "";
  const label = REMOTE_LABELS[remote] || "Remote";
  // "ssh-remote+10.8.0.10" -> "10.8.0.10". Bei Dev Containers steckt dahinter
  // hex-kodiertes JSON: dann lieber nur das Label als eine Zeichenwueste. Weicht
  // der Text damit vom echten Titel ab, ist das unschaedlich (siehe oben).
  const rest = String((uri && uri.authority) || "").split("+").slice(1).join("+");
  const host = /^[0-9a-f]{16,}$/i.test(rest) ? "" : rest;
  return host ? ` [${label}: ${host}]` : ` [${label}]`;
}

// folders = vscode.workspace.workspaceFolders.
function workspaceName(remote, folders) {
  const f = folders;
  if (f && f.length) return f[0].name + remoteTag(remote, f[0].uri);
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
// workspaceFile = vscode.workspace.workspaceFile, folders wie oben.
function workspaceFolder(workspaceFile, folders) {
  const wf = workspaceFile;
  if (wf && wf.scheme === "file") return wf.fsPath;
  // Eine .code-workspace-Datei auf der GEGENSTELLE bleibt bewusst aussen vor: zu
  // --folder-uri gibt es kein Workspace-Gegenstueck, und stattdessen ihren ersten
  // Ordner zu oeffnen waere ein ANDERES Fenster als das, das der Neustart zumacht.
  // Lieber ehrlich als uebersprungen gemeldet werden. (Den VS-Code-Neustart gibt es
  // nur noch in der Historie: `git show 2856494:deck/domain/restart_plan.py`.)
  if (wf && wf.scheme === "vscode-remote") return null;
  const f = folders;
  const u = (f && f.length) ? f[0].uri : null;
  if (u && u.scheme === "file") return u.fsPath;
  // Remote: kein Pfad, den DIESER Rechner kennt - aber eine Adresse, unter der VS
  // Code das Fenster wieder aufmacht (`code --folder-uri vscode-remote://…`). Das
  // Panel unterschied Pfad und Adresse am Schema (`restart_plan.open_args`, s. o.).
  if (u && u.scheme === "vscode-remote") return u.toString();
  return null;
}

module.exports = { remoteTag, workspaceName, workspaceFolder };
