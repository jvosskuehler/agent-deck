"""Tasten in die Konsole eines fremden Prozesses schreiben, ohne Fokus.

Windows Terminal hat kein `sendText` wie VS Code. Jede Konsole hat aber einen
Eingabepuffer, und `WriteConsoleInputW` legt dort Tastendruecke ab, als haette jemand
getippt: der Prozess im Pane liest sie wie echte Eingabe, egal welches Fenster vorne
ist. Dazu haengt man sich per `AttachConsole(pid)` an dessen Konsole. Erprobt am
2026-11-06 mit Claude Code in einer unsichtbaren Konsole (`/exit` beendete die
Session) und in Windows-Terminal-Panes.

Laeuft als EIGENER, kurzlebiger Prozess (`python -m deck.platform.console_input`),
nie im Panel: ein Prozess, der an einer fremden Konsole haengt, bekommt deren
CTRL_CLOSE_EVENT ab, wenn sie zugeht - und dessen Standardbehandlung beendet ihn.
Das soll ein Wegwerfprozess abbekommen, nicht das Deck.

Steuerzeichen (`\\r`, Esc, Tab) gehen mit passendem Virtual-Key-Code raus, alles
andere als Zeichen. Folgen wie `\\x1b[Z` (Shift+Tab) oder die Bracketed-Paste-Marker
kommen Zeichen fuer Zeichen an; Claude liest daraus dieselbe Sequenz wie aus VS Code.
"""
import base64
import ctypes
import json
import sys
import time
from ctypes import wintypes
from typing import Any

KEY_EVENT = 0x0001
_VK = {"\r": 0x0D, "\x1b": 0x1B, "\t": 0x09, "\x08": 0x08}
_GENERIC_RW = 0xC0000000
_SHARE_RW = 0x3
_OPEN_EXISTING = 3


class _KEY_EVENT_RECORD(ctypes.Structure):
    _fields_ = [("bKeyDown", wintypes.BOOL), ("wRepeatCount", wintypes.WORD),
                ("wVirtualKeyCode", wintypes.WORD), ("wVirtualScanCode", wintypes.WORD),
                ("uChar", wintypes.WCHAR), ("dwControlKeyState", wintypes.DWORD)]


class _EVENT(ctypes.Union):
    _fields_ = [("KeyEvent", _KEY_EVENT_RECORD), ("_pad", ctypes.c_byte * 16)]


class _INPUT_RECORD(ctypes.Structure):
    _fields_ = [("EventType", wintypes.WORD), ("Event", _EVENT)]


def _records(text: str) -> list:
    """Je Zeichen ein Druecken und ein Loslassen. Zeichen ausserhalb der BMP gehen als
    zwei UTF-16-Haelften raus - so liest sie auch ReadConsoleInputW."""
    units = text.encode("utf-16-le")
    out = []
    for i in range(0, len(units), 2):
        ch = units[i:i + 2].decode("utf-16-le", errors="surrogatepass")
        for down in (True, False):
            r = _INPUT_RECORD()
            r.EventType = KEY_EVENT
            r.Event.KeyEvent.bKeyDown = down
            r.Event.KeyEvent.wRepeatCount = 1
            r.Event.KeyEvent.wVirtualKeyCode = _VK.get(ch, 0)
            r.Event.KeyEvent.uChar = ch
            out.append(r)
    return out


def inject(pid: int, steps: list) -> int:
    """Schritte in die Konsole von `pid` schreiben. steps = [("text", s) | ("sleep", sek)].
    Rueckgabe: Zahl der geschriebenen Eingabe-Ereignisse. Wirft OSError, wenn das
    Anhaengen oder Schreiben scheitert."""
    k = ctypes.WinDLL("kernel32", use_last_error=True)
    k.CreateFileW.restype = wintypes.HANDLE
    k.FreeConsole()
    if not k.AttachConsole(int(pid)):
        raise OSError(ctypes.get_last_error(), f"AttachConsole({pid})")
    try:
        h = k.CreateFileW("CONIN$", _GENERIC_RW, _SHARE_RW, None, _OPEN_EXISTING, 0, None)
        if h in (None, wintypes.HANDLE(-1).value):
            raise OSError(ctypes.get_last_error(), "CONIN$")
        try:
            total = 0
            for kind, val in steps:
                if kind == "sleep":
                    time.sleep(float(val))
                    continue
                recs = _records(str(val))
                if not recs:
                    continue
                arr = (_INPUT_RECORD * len(recs))(*recs)
                n = wintypes.DWORD(0)
                if not k.WriteConsoleInputW(h, arr, len(recs), ctypes.byref(n)):
                    raise OSError(ctypes.get_last_error(), "WriteConsoleInputW")
                total += n.value
            return total
        finally:
            k.CloseHandle(h)
    finally:
        k.FreeConsole()


def encode_steps(steps: list) -> str:
    """Schritte fuer die Kommandozeile des Wegwerfprozesses (Base64-JSON)."""
    return base64.b64encode(json.dumps(steps).encode("utf-8")).decode("ascii")


def main(argv: list[str]) -> int:
    try:
        pid = int(argv[1])
        steps: Any = json.loads(base64.b64decode(argv[2]).decode("utf-8"))
        n = inject(pid, [tuple(s) for s in steps])
        print(f"ok {n}")
        return 0
    except Exception as e:      # Ausgabe statt Traceback: der Aufrufer liest nur die Zeile
        print(f"fail {type(e).__name__}: {e}")
        return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
