"""Zwei Hosts, eine Schnittstelle: VS Code (Broker + Extension) und Windows Terminal.

Das Panel fragt seine Fenster an genau einer Stelle (`self.broker`) und sendet ueber
genau eine Fassade (net/commands.py). Dieser Verteiler sitzt dazwischen und gibt jede
Frage an den Host weiter, der den Buchstaben traegt: steht er in Windows Terminal
(ops/wt_host), dorthin, sonst an den Broker. So muss keine Panel-Datei wissen, wo ein
Agent laeuft.

Liegt in ui/, weil er beide Seiten kennt: den Broker (net/) und den WT-Host (ops/).
Der Neustart (ops/vscode_restart) bekommt bewusst den ROHEN Broker - er schliesst
und oeffnet VS-Code-Fenster und hat mit Windows-Terminal-Bloecken nichts zu tun.
"""
from typing import Any


class HostRouter:
    def __init__(self, broker: Any, wt: Any) -> None:
        self.vscode = broker
        self.wt = wt

    def _for(self, win: str) -> Any:
        return self.wt if self.wt.has(win) else self.vscode

    def is_wt(self, win: str | None) -> bool:
        return bool(win) and self.wt.has(win)

    # ── Lebenszyklus ────────────────────────────────────
    def start(self) -> None:
        self.vscode.start()
        self.wt.start()

    def stop(self) -> None:
        self.wt.stop()
        self.vscode.stop()

    # ── je Buchstabe ────────────────────────────────────
    def send_window(self, window: str, obj: dict[str, Any]) -> bool:
        return self._for(window).send_window(window, obj)

    def connected(self, window: str) -> bool:
        return self._for(window).connected(window)

    def remote(self, window: str) -> str | None:
        return self._for(window).remote(window)

    def terminals(self, window: str) -> list[str]:
        return self._for(window).terminals(window)

    def assign(self, workspace: str, window: str) -> bool:
        # Ein WT-Buchstabe wird nie an ein VS-Code-Fenster vergeben - sonst landeten die
        # Kommandos einer Kachel bei zwei Hosts zugleich.
        if self.wt.has(window):
            return False
        return self.vscode.assign(workspace, window)

    def forget(self, window: str) -> bool:
        if self.wt.has(window):
            return self.wt.forget(window)
        return self.vscode.forget(window)

    # ── ueber alle ──────────────────────────────────────
    def drain_seen(self) -> set[str]:
        return set(self.vscode.drain_seen()) | set(self.wt.drain_seen())

    # Die folgenden drei betreffen nur VS-Code-Fenster: Auto-Binden neu verbundener
    # Extensions und der Neustart-Plan.
    def workspaces(self) -> list[str]:
        return self.vscode.workspaces()

    def workspace_slots(self, workspace: str) -> list[str]:
        return self.vscode.workspace_slots(workspace)

    def client_info(self) -> list[dict[str, Any]]:
        return self.vscode.client_info()
