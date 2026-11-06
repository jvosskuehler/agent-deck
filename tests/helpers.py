"""Gemeinsame Grundlage aller Testdateien.

Wird von jeder Testdatei importiert - auch wenn sie nichts davon benutzt, denn der
Import erledigt drei Dinge, die sonst jede Datei wiederholen muesste: die Repo-Wurzel
auf den sys.path legen, die Deck-Sprache festnageln und das Schutzgitter aufstellen.
"""
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if ROOT not in sys.path:
    sys.path.insert(0, ROOT)

from deck import i18n
from deck.platform import arrange as _arrange
from deck.platform import overlay as _overlay_mod

# ── Schutzgitter ────────────────────────────────────────────────────────────
# Die Tests laufen meist in einem VS-Code-Terminal. Ein Test, der eine Attrappe
# vergisst, erreicht sonst die echten Eingriffe des Decks: WM_CLOSE an jedes
# VS-Code-Fenster und `taskkill /F /T /IM Code.exe` (ops/vscode_restart). Genau das ist
# am 2026-11-06 passiert - `python tests/run.py` hat zweimal das VS Code beendet, in
# dem er lief, ohne Crash-Dump und ohne eine Zeile im VS-Code-Log. Darum scheitern
# solche Aufrufe hier LAUT, statt still zu wirken. Geprueft wird das aufgerufene
# Programm, nicht Text in den Argumenten (die killable-Tests reichen "Code.exe" als
# Beispieldaten an node durch).
_GESPERRT = ("taskkill", "taskkill.exe", "code", "code.exe", "code.cmd")


def _gesperrt(args):
    seq = list(args) if isinstance(args, (list, tuple)) else str(args).split()
    exe = os.path.basename(str(seq[0])).lower() if seq else ""
    if exe in _GESPERRT:
        return True
    if exe in ("powershell", "powershell.exe", "pwsh", "pwsh.exe"):
        return "stop-process" in " ".join(map(str, seq)).lower()
    return False


_echt_run, _echt_popen = subprocess.run, subprocess.Popen


def _run(args, *a, **k):
    if _gesperrt(args):
        raise AssertionError(f"Schutzgitter: ein Test wollte {args!r} wirklich ausfuehren")
    return _echt_run(args, *a, **k)


class _Popen(_echt_popen):
    def __init__(self, args, *a, **k):
        if _gesperrt(args):
            raise AssertionError(f"Schutzgitter: ein Test wollte {args!r} wirklich starten")
        super().__init__(args, *a, **k)


def _post_close(hwnd, *a, **k):
    raise AssertionError(f"Schutzgitter: ein Test wollte das echte Fenster {hwnd} schliessen")


def _place_window(hwnd, *a, **k):
    raise AssertionError(f"Schutzgitter: ein Test wollte das echte Fenster {hwnd} verschieben")


def _overlay(*a, **k):
    raise AssertionError("Schutzgitter: ein Test wollte ein echtes Rahmenfenster anlegen "
                         "oder fremde Fensterecken aendern")


subprocess.run, subprocess.Popen = _run, _Popen
_arrange.post_close = _post_close
_arrange.place_window = _place_window      # ops/wt_tiles kachelt sonst echte Fenster
_overlay_mod.create = _overlay             # ui/focus_frame legt sonst ein echtes Fenster an
_overlay_mod.square_corners = _overlay

# Die Usage- und Anzeige-Tests pruefen die deutsche Baseline. Ohne diese Zeile haengen
# sie am realen ~/.claude/settings.json, das schon auf 'english' stehen kann.
i18n._lang = i18n.GERMAN

# Die Modus-Reihenfolge des Rechtsklick-Menues. Von mehreren Testdateien gebraucht,
# darum hier statt in einer davon.
_CYCLE = ["manual", "accept", "plan", "auto"]

# Alle Statuswerte, fuer die es eine Glow-Farbe geben MUSS.
_GLOW = {"idle": 1, "done": 1, "thinking": 1, "running": 1, "waiting": 1, "none": 1}

# Masse der Griff-Kapsel fuer die Bildtests: Fenster 26 px = Roehre 16 + Hof-Luft 10.
# Stehen hier, weil sowohl die Kapsel- als auch die Schwapp-Tests dagegen rechnen.
HR_W, HR_TUBE, HR_LEN = 26, 16, 160
