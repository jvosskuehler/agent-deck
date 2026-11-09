"""Das Hilfsprogramm, das den Fokus-Rahmen zeichnet (focus-frame/, C#).

Warum ein eigenes Programm: der Rahmen ist eine Animation um ein fremdes Fenster, und in
Python auf dem Prozessor gerechnet sah sie grob aus (20 bis 30 Bilder je Sekunde,
sichtbare Stufen im Leuchthof) und kostete trotzdem bis zu einem Drittel eines Kerns.
Das Hilfsprogramm rechnet jeden Pixel auf der Grafikkarte (Direct3D 11, DirectComposition)
im Takt des Monitors. Verworfen am 2026-11-06: der Python-Zeichner
(render/focus_band.py, render/focus_fx.py im Verlauf des Repos).

Das Deck entscheidet, WELCHES Fenster und WELCHES Design (ui/focus_frame.py), das
Hilfsprogramm zeichnet nur. Gesprochen wird ueber stdin, ein JSON-Kommando je Zeile
(Format im Kopf von focus-frame/Program.cs). Schliesst sich stdin, weil das Deck endet
oder abstuerzt, beendet sich das Hilfsprogramm selbst.

Gebaut wird es von install.ps1 nach focus-frame/out/. Fehlt es dort, gibt es eben keinen
Rahmen: das wird einmal ins Log geschrieben, das Deck laeuft weiter.
"""
import json
import os
import subprocess
import time
from collections.abc import Callable
from typing import Any

from deck.domain import paths as dp
from deck.ops import log

EXE = os.path.join(dp.REPO_ROOT, "focus-frame", "out", "FocusFrame.exe")
_NO_WINDOW = 0x08000000              # CREATE_NO_WINDOW


def _spawn(exe: str) -> Any:
    return subprocess.Popen([exe], stdin=subprocess.PIPE, stdout=subprocess.DEVNULL,
                            stderr=subprocess.DEVNULL, creationflags=_NO_WINDOW,
                            close_fds=True)


class FrameHelper:
    RESTART_S = 10.0         # stirbt es, fruehestens nach so vielen Sekunden neu starten

    def __init__(self, exe: str = EXE, *, spawn: Callable | None = None,
                 exists: Callable[[str], bool] = os.path.isfile,
                 clock: Callable[[], float] = time.monotonic) -> None:
        self.exe = exe
        # Erst beim Aufruf nachgeschlagen statt als Default gebunden: so greift das
        # Schutzgitter der Tests (tests/helpers.py) auch hier.
        self.spawn = spawn or (lambda path: _spawn(path))
        self.exists, self.clock = exists, clock
        self._proc: Any = None
        self._last_start = -1e9
        self._missing_logged = False

    def _alive(self) -> bool:
        return self._proc is not None and self._proc.poll() is None

    def _ensure(self) -> bool:
        """Laeuft das Programm? Sonst starten, aber nicht oefter als alle RESTART_S."""
        if self._alive():
            return True
        if not self.exists(self.exe):
            if not self._missing_logged:
                log.note(f"Fokus-Rahmen: {self.exe} fehlt (install.ps1 baut es)")
                self._missing_logged = True
            return False
        now = self.clock()
        if now - self._last_start < self.RESTART_S:
            return False
        self._last_start = now
        try:
            self._proc = self.spawn(self.exe)
            return True
        except OSError:
            log.exc("frame_helper.spawn")
            self._proc = None
            return False

    def send(self, obj: dict[str, Any]) -> bool:
        """Ein Kommando schicken. False, wenn das Programm fehlt oder nicht laeuft."""
        if not self._ensure():
            return False
        try:
            self._proc.stdin.write((json.dumps(obj) + "\n").encode("utf-8"))
            self._proc.stdin.flush()
            return True
        except (OSError, ValueError):
            self._proc = None            # Rohr gerissen: beim naechsten Mal neu starten
            return False

    def stop(self) -> None:
        if not self._alive():
            return
        try:
            self._proc.stdin.close()       # stdin zu = das Programm beendet sich
            self._proc.wait(timeout=2)
        except Exception:
            try:
                self._proc.kill()
            except Exception:
                pass
        self._proc = None
