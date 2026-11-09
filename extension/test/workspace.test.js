// Wie sich ein Fenster beim Panel ausweist (workspace.js): Name mit Remote-Anhaengsel
// und der Ordner, unter dem es sich wieder oeffnen laesst.

const test = require("node:test");
const assert = require("node:assert/strict");
const ws = require("../workspace");

// Nachbau der vscode.Uri-Felder, die gelesen werden.
const uri = (scheme, rest) => ({ scheme, authority: rest.authority || "", fsPath: rest.fsPath, toString: () => rest.str });
const folder = (name, u) => ({ name, uri: u });

test("a local window is named after its first folder", () => {
  const f = [folder("agent-deck", uri("file", { fsPath: "C:\\r\\agent-deck" }))];
  assert.equal(ws.workspaceName(null, f), "agent-deck");
});

test("a window without a folder has no name", () => {
  // Sonst wuerde "Untitled (Workspace)" als Phantom-Kachel gebunden.
  assert.equal(ws.workspaceName(null, undefined), null);
  assert.equal(ws.workspaceName(null, []), null);
});

test("a remote window carries its host like the title bar", () => {
  const f = [folder("jorrit", uri("vscode-remote", { authority: "ssh-remote+10.8.0.10" }))];
  assert.equal(ws.workspaceName("ssh-remote", f), "jorrit [SSH: 10.8.0.10]");
});

test("a dev container shows only its label, not the hex blob", () => {
  const f = [folder("app", uri("vscode-remote", { authority: "dev-container+7b22686f737450617468223a22" }))];
  assert.equal(ws.workspaceName("dev-container", f), "app [Dev Container]");
});

test("an unknown remote kind is still marked as remote", () => {
  assert.equal(ws.remoteTag("exotic", { authority: "exotic+box" }), " [Remote: box]");
  assert.equal(ws.remoteTag(null, { authority: "ssh-remote+x" }), "");
});

test("a workspace file wins over its first folder", () => {
  const wf = uri("file", { fsPath: "C:\\r\\all.code-workspace" });
  const f = [folder("a", uri("file", { fsPath: "C:\\r\\a" }))];
  assert.equal(ws.workspaceFolder(wf, f), "C:\\r\\all.code-workspace");
});

test("a remote workspace file is not reopened through its first folder", () => {
  const wf = uri("vscode-remote", { str: "vscode-remote://ssh-remote+x/a.code-workspace" });
  const f = [folder("a", uri("vscode-remote", { str: "vscode-remote://ssh-remote+x/a" }))];
  assert.equal(ws.workspaceFolder(wf, f), null);
});

test("a local folder gives its path, a remote one its address", () => {
  assert.equal(ws.workspaceFolder(undefined, [folder("a", uri("file", { fsPath: "C:\\r\\a" }))]), "C:\\r\\a");
  const remote = [folder("a", uri("vscode-remote", { str: "vscode-remote://wsl+Ubuntu/home/a" }))];
  assert.equal(ws.workspaceFolder(undefined, remote), "vscode-remote://wsl+Ubuntu/home/a");
  assert.equal(ws.workspaceFolder(undefined, undefined), null);
});
