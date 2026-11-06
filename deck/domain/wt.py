"""Windows Terminal als zweiter Host neben VS Code: die rechenbaren Teile.

Ein Repo-Block, den das Deck in Windows Terminal oeffnet, ist dort eine Reihe von
Fenstern: jeder Agent bekommt ein EIGENES Fenster `deck-<Slot>`, und das Deck legt sie
gleich gross nebeneinander (Raster in domain/arrange.tile_grid). Nicht ein Fenster mit
geteilten Panes, weil Windows Terminal Panes nur halbieren kann und keinen Befehl zum
Angleichen hat: der dritte Agent bekam ein Viertel, der vierte ein Achtel (gemessen
2026-11-06: 48 / 24 / 12 / 12 % der Flaeche). Was sich ohne Win32 und ohne Prozesse
ausrechnen laesst, steht hier und ist getestet: Slot-Namen vergeben, die Kommandozeile
fuer `wt.exe` bauen, den Fenstertitel lesen, die PID-Datei eines Panes auslegen.
Ausgefuehrt wird es in ops/wt_host.py.

Drei Vertraege, die man nicht raten kann:

  • Der Pane-TITEL ist der Slot-Name. Er wird beim Anlegen gesetzt (`--title A3`) und
    gegen Claude festgenagelt (`--suppressApplicationTitle`). Daran findet die
    UI-Automation das Pane wieder (Name des TermControl), und daran erkennt das Deck,
    welcher Agent gerade vorne ist: der Fenstertitel ist der Titel des aktiven Panes.
  • `AGENT_SLOT` kommt aus dem Startskript im Pane (ops/wt_agent.ps1), nicht aus wt.exe:
    wt kann einer neuen Pane keine Umgebungsvariable mitgeben.
  • Die PID-Datei `state/<slot>.wtpid` schreibt dasselbe Skript, als allererstes. Sie
    nennt die Shell des Panes samt Startzeit; solange dieser Prozess lebt, lebt der
    Slot. Die Startzeit schuetzt vor einer wiederverwendeten PID.
"""
import base64
import json
import re
import shlex
from typing import Any

WINDOW_PREFIX = "deck-"
# Verstecktes Windows-Terminal-Profil mit closeOnExit "always": ein beendeter Agent
# laesst sonst ein Pane mit "[process exited with code 1]" stehen.
PROFILE = "Agent Deck"
PID_SUFFIX = ".wtpid"
# Fensterklasse aller Windows-Terminal-Fenster (Cascadia = der Projektname von WT).
WINDOW_CLASS = "CASCADIA_HOSTING_WINDOW_CLASS"

_SLOT = re.compile(r"^([A-Za-z])(\d+)$")

# Tasten wie in extension/extension.js (KEYMAP) - dieselben Namen, dieselben Folgen,
# damit das Panel nicht wissen muss, welcher Host einen Slot traegt.
KEYMAP = {
    "enter": "\r", "esc": "\x1b", "ctrl-c": "\x03",
    "up": "\x1b[A", "down": "\x1b[B", "right": "\x1b[C", "left": "\x1b[D",
    "tab": "\t", "shift-tab": "\x1b[Z",
    "1": "1", "2": "2", "3": "3", "4": "4", "5": "5",
}
# Pausen wie in der Extension: ein langer Prompt kommt als Paste an, und ein
# mitgeschicktes Enter wuerde darin verschluckt -> Enter getrennt und spaeter.
SUBMIT_ENTER_DELAY_S = 0.45
KEY_REPEAT_DELAY_S = 0.09
PASTE_START, PASTE_END = "\x1b[200~", "\x1b[201~"


def window_name(slot: str) -> str:
    """Name des Windows-Terminal-Fensters eines Agenten (`wt -w deck-A3`). Ein noch
    unbekannter Name oeffnet ein neues Fenster."""
    return WINDOW_PREFIX + slot


def split_slot(name: str) -> tuple[str, int] | None:
    """'A3' -> ('A', 3); alles andere -> None."""
    m = _SLOT.match(name or "")
    return (m.group(1).upper(), int(m.group(2))) if m else None


def slots_of(win: str, names) -> list[str]:
    """Die Slot-Namen dieses Buchstabens, numerisch sortiert (A2 vor A10)."""
    hits = []
    for n in names:
        sp = split_slot(n)
        if sp and sp[0] == win.upper():
            hits.append((sp[1], n))
    return [n for _, n in sorted(hits)]


def next_slot(win: str, taken) -> str:
    """Naechster freier Name: hoechster vergebener Index dieses Buchstabens + 1, wie
    detect.nextIndex in der Extension - also auch Namensrecycling nach dem Schliessen
    des hoechsten Agenten."""
    idx = [sp[1] for sp in map(split_slot, taken) if sp and sp[0] == win.upper()]
    return f"{win.upper()}{max(idx, default=0) + 1}"


def wanted_or_next(win: str, wanted: str | None, taken) -> str:
    """Der Wunschname (Neustart), wenn er zu diesem Buchstaben passt und frei ist,
    sonst der naechste freie."""
    sp = split_slot(wanted or "")
    if sp and sp[0] == win.upper() and wanted not in set(taken):
        return f"{sp[0]}{sp[1]}"
    return next_slot(win, taken)


def claude_argv(model: str | None = None, args: str | None = None) -> list[str]:
    """Die Argumente fuer `claude`. `args` ist eine FERTIGE Flag-Zeile aus
    domain/restart_plan.flags (z. B. `--resume "<id>" --model "opus[1m]"`) und hat
    Vorrang; sonst nur `--model <model>`. Zerlegt wird wie von einer POSIX-Shell, damit
    die Anfuehrungszeichen der Flag-Zeile wirken und nicht im Argument landen."""
    if args and args.strip():
        return shlex.split(args, posix=True)
    return ["--model", model] if model else []


def launch_spec(slot: str, state_dir: str, argv: list[str]) -> str:
    """Was das Startskript im Pane wissen muss, als Base64-JSON: Slot, State-Ordner,
    claude-Argumente. Base64, weil die Zeile durch zwei Parser muss (wt.exe, dann
    PowerShell) und weder ';' (trennt bei wt Unterbefehle) noch Anfuehrungszeichen
    unbeschadet durch beide kaemen."""
    raw = json.dumps({"slot": slot, "state": state_dir, "args": list(argv)})
    return base64.b64encode(raw.encode("utf-8")).decode("ascii")


def decode_spec(spec: str) -> dict[str, Any]:
    """Umkehrung von launch_spec (fuer Tests und Diagnose)."""
    return json.loads(base64.b64decode(spec).decode("utf-8"))


def wt_argv(wt_exe: str, slot: str, cwd: str, shell: str, script: str,
            spec: str, *, profile: str | None = PROFILE) -> list[str]:
    """Kommandozeile fuer einen neuen Agenten: ein eigenes Fenster je Agent (siehe
    Modulkopf); nebeneinander legt sie danach ops/wt_host."""
    argv = [wt_exe, "-w", window_name(slot), "new-tab"]
    if profile:
        argv += ["-p", profile]
    argv += ["--title", slot, "--suppressApplicationTitle", "-d", cwd,
             shell, "-NoLogo", "-NoExit", "-ExecutionPolicy", "Bypass",
             "-File", script, "-Spec", spec]
    return argv


def slot_from_title(title: str | None) -> str | None:
    """Fenstertitel eines Windows-Terminal-Fensters -> Slot des aktiven Panes, oder
    None (fremdes Pane, Einstellungen, ...). Der Titel IST der Slot-Name (siehe oben)."""
    t = (title or "").strip()
    sp = split_slot(t)
    return f"{sp[0]}{sp[1]}" if sp else None


def parse_pidfile(data: Any) -> tuple[int, int] | None:
    """Inhalt einer .wtpid-Datei -> (pid, startzeit als FILETIME) oder None."""
    if not isinstance(data, dict):
        return None
    try:
        pid, created = int(data["pid"]), int(data["created"])
    except (KeyError, TypeError, ValueError):
        return None
    return (pid, created) if pid > 0 else None


def same_start(a: int, b: int, tolerance: int = 10_000_000) -> bool:
    """Zwei FILETIME-Startzeiten (100-ns-Schritte) gelten als gleich, wenn sie weniger
    als eine Sekunde auseinanderliegen. PowerShell rechnet die Startzeit ueber
    DateTime um; exakt gleich muss sie dabei nicht bleiben."""
    return abs(int(a) - int(b)) < tolerance


def send_steps(text: str, *, execute: bool = True, submit: bool = False) -> list:
    """Eingabe-Schritte fuer `send`, wie die Extension sie schickt: submit -> Text als
    Bracketed Paste, Pause, dann ein EIGENES Enter; sonst Text plus Enter (execute)."""
    if submit:
        return [("text", PASTE_START + text + PASTE_END),
                ("sleep", SUBMIT_ENTER_DELAY_S), ("text", "\r")]
    return [("text", text + ("\r" if execute else ""))]


def key_steps(key: str, repeat: int = 1) -> list:
    """Eingabe-Schritte fuer `key`: bekannte Namen ueber KEYMAP, sonst roh."""
    seq = KEYMAP.get(key, key)
    steps: list = []
    for i in range(max(1, int(repeat or 1))):
        if i:
            steps.append(("sleep", KEY_REPEAT_DELAY_S))
        steps.append(("text", seq))
    return steps
