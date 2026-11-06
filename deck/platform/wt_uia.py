"""Panes in Windows Terminal finden und nach vorn holen - ueber UI Automation.

Windows Terminal hat keine Schnittstelle, um von aussen ein bestimmtes Pane zu
waehlen. Seine Oberflaeche ist aber per UI Automation lesbar: jedes Pane ist ein
"TermControl" mit dem Pane-Titel als Namen (siehe wt_uia_worker.ps1). Die Abfragen
laufen in einem DAUERHAFTEN PowerShell-Prozess, weil schon das Laden der
UIA-Assemblies rund eine Sekunde kostet.

Reine stdlib. Den Pfad zum Arbeiter-Skript bekommt die Klasse von aussen (ops/
wt_host.py, ueber paths.REPO_ROOT): die platform-Schicht kennt die Pfadregeln nicht.
"""
import json
import queue
import subprocess
import threading
from typing import Any

_CREATE_NO_WINDOW = 0x08000000


class UiaWorker:
    def __init__(self, script: str, *, exe: str = "powershell.exe",
                 timeout: float = 8.0) -> None:
        self.script = script
        self.exe = exe
        self.timeout = timeout
        self._proc: subprocess.Popen | None = None
        self._lines: "queue.Queue[str | None]" = queue.Queue()
        self._lock = threading.Lock()

    # ── Prozess ─────────────────────────────────────────
    def _alive(self) -> bool:
        return self._proc is not None and self._proc.poll() is None

    def _start(self) -> None:
        self._lines = queue.Queue()
        self._proc = subprocess.Popen(
            [self.exe, "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
             "-File", self.script],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
            creationflags=_CREATE_NO_WINDOW, text=True, encoding="utf-8", bufsize=1)
        out, lines = self._proc.stdout, self._lines

        def pump() -> None:
            for ln in out:              # endet, wenn der Prozess stirbt
                lines.put(ln)
            lines.put(None)

        threading.Thread(target=pump, name="wt-uia-pump", daemon=True).start()

    def close(self) -> None:
        p, self._proc = self._proc, None
        if p and p.poll() is None:
            try:
                p.stdin.close()         # Arbeiter endet bei EOF von selbst
                p.wait(timeout=2)
            except Exception:
                p.kill()

    # ── Fragen ──────────────────────────────────────────
    def ask(self, op: str, **kw: Any) -> dict | None:
        """Eine Frage stellen; None bei Zeitueberschreitung oder totem Arbeiter. Ein
        haengender Arbeiter wird beendet - die naechste Frage startet einen frischen."""
        with self._lock:
            try:
                if not self._alive():
                    self._start()
                self._proc.stdin.write(json.dumps({"op": op, **kw}) + "\n")
                self._proc.stdin.flush()
                line = self._lines.get(timeout=self.timeout)
            except (queue.Empty, OSError, ValueError):
                line = None
            if line is None:
                if self._proc and self._proc.poll() is None:
                    self._proc.kill()
                self._proc = None
                return None
            try:
                res = json.loads(line)
            except ValueError:
                return None
            return res if isinstance(res, dict) and "error" not in res else None

    def find(self, slot: str) -> int | None:
        """HWND des Windows-Terminal-Fensters, in dem das Pane `slot` steht (dessen Tab
        ist danach der aktive), sonst None."""
        r = self.ask("find", slot=slot)
        return int(r["hwnd"]) if r and r.get("hwnd") else None

    def focus(self, slot: str) -> int | None:
        """Wie find, setzt aber zusaetzlich den Tastaturfokus auf dieses Pane."""
        r = self.ask("focus", slot=slot)
        return int(r["hwnd"]) if r and r.get("ok") and r.get("hwnd") else None

