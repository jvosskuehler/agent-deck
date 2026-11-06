"""Duenne, typisierte Fassade ueber das Wire-Protokoll des Brokers.

Frueher baute jede Action-Methode der AgentDeck-Klasse ihr Kommando-Dict selbst
und rief broker.send_window – acht fast gleiche Stellen mit nackten Protokoll-
Feldern. Hier gebuendelt: jede Methode baut ihr Dict aus protocol-Konstanten,
kapselt den slot[0]==Fenster-Vertrag und gibt bool zurueck (True = an eine
verbundene Extension geschickt). Die App-Logik (aktive Kachel, gemerktes Effort,
Auswahl zuruecksetzen) bleibt bewusst im Panel – hier steckt NUR das Senden.
"""
from typing import TYPE_CHECKING, Any

from deck.domain import protocol

if TYPE_CHECKING:                       # nur fuer die Typpruefung - zur Laufzeit
    from deck.net.broker import Broker  # braucht die Fassade den Broker nicht zu kennen


class BrokerCommands:
    def __init__(self, broker: "Broker") -> None:
        self.broker = broker

    def focus_pane(self, slot: str) -> bool:
        """Ein bestimmtes Terminal (Slot) im zugehoerigen Fenster fokussieren."""
        return self.broker.send_window(slot[0], {"cmd": protocol.CMD_FOCUS_PANE, "slot": slot})

    def send_text(self, slot: str, text: str, execute: bool = True,
                  submit: bool = False) -> bool:
        """Text/Slash-Kommando an den Agent in diesem Slot schicken. submit=True ->
        die Extension schreibt den Text und schickt ihn per SEPARATEM Enter ab (fuer
        lange Prompts wie den Ticket-Prompt, die Claude Code sonst als Paste erkennt
        und NICHT absendet). Bei submit=True darf der Text kein eigenes \\r tragen ->
        execute wird dann zwingend False."""
        payload = {"cmd": protocol.CMD_SEND, "slot": slot, "text": text,
                   "execute": False if submit else execute}
        if submit:
            payload["submit"] = True
        return self.broker.send_window(slot[0], payload)

    def send_key(self, slot: str, key: str, repeat: int = 1) -> bool:
        """Einzelne Taste an den Agent (repeat>1 = mehrfach, z.B. 2x Esc / Shift+Tab)."""
        payload: dict[str, Any] = {"cmd": protocol.CMD_KEY, "slot": slot, "key": key}
        if repeat and repeat > 1:
            payload["repeat"] = repeat
        return self.broker.send_window(slot[0], payload)

    def create_agent(self, win: str, model: str | None = None, *,
                     slot: str | None = None, args: str | None = None,
                     cwd: str | None = None) -> bool:
        """Die Extension oeffnet ein weiteres Claude-Terminal im Fenster. model (optional)
        -> die Extension startet `claude --model <model>`, erzwingt also das Wunsch-Modell
        per CLI-Flag (schlaegt das zuletzt in ~/.claude.json gemerkte Modell).

        Die drei Schluesselwort-Felder gehoeren dem Neustart (ops/vscode_restart):
        `args` ist eine FERTIGE Flag-Zeile (domain/restart_plan.flags, enthaelt dann auch
        --model) und hat Vorrang vor `model`; `cwd` ist das Arbeitsverzeichnis des neuen
        Terminals - beim Wiederaufnehmen Pflicht, weil Claude eine Session ueber ihr
        Projektverzeichnis findet und ein Ticket-Agent im worktree lief; `slot` ist ein
        Wunschname, damit die Kacheln nach dem Neustart wieder dieselben Chats zeigen."""
        payload: dict[str, Any] = {"cmd": protocol.CMD_CREATE_AGENT}
        if model:
            payload["model"] = model
        if args:
            payload["args"] = args
        if cwd:
            payload["cwd"] = cwd
        if slot:
            payload["slot"] = slot
        return self.broker.send_window(win, payload)

    def reload(self, win: str) -> bool:
        """'Developer: Reload Window' im Fenster ausloesen."""
        return self.broker.send_window(win, {"cmd": protocol.CMD_RELOAD})

    def close_agent(self, slot: str) -> bool:
        """Ein einzelnes Agent-Terminal schliessen."""
        return self.broker.send_window(slot[0], {"cmd": protocol.CMD_CLOSE_AGENT, "slot": slot})

    def close_window(self, win: str) -> bool:
        """Das ganze VS-Code-Fenster schliessen (inkl. aller Agenten darin)."""
        return self.broker.send_window(win, {"cmd": protocol.CMD_CLOSE_WINDOW})

    def save_all(self, win: str) -> bool:
        """Alle Editoren dieses Fensters speichern. Vor dem Schliessen: eine
        ungespeicherte Datei verwandelt closeWindow in einen modalen Dialog, und der
        haelt einen automatischen Ablauf still an, bis jemand hinsieht."""
        return self.broker.send_window(win, {"cmd": protocol.CMD_SAVE_ALL})
