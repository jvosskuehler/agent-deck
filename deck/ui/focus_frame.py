"""Der leuchtende Rahmen um das Windows-Terminal-Fenster des Agenten, in das gerade
getippt wird. Er liegt auf der Aussenkante des Fensters, die Tab-Leiste eingeschlossen,
und sein Hof reicht nach aussen ueber die Kante.

Bei jedem Fokuswechsel kommt ein zufaelliges Design dran, nie zweimal dasselbe
hintereinander (domain/focus_frame). Gezeichnet wird er vom Hilfsprogramm
focus-frame/ auf der Grafikkarte (warum: ops/frame_helper.py); dieses Modul entscheidet
nur, ob und um welches Fenster, und sagt es ihm. Das Hilfsprogramm folgt dem Fenster
danach selbst, auch beim Ziehen.

Ein Shader im Terminal kam nicht in Frage: der malt nur in die Textflaeche, und die liegt
hinter Innenabstand und Scrollleiste (verworfen 2026-11-06).

Die Agentenfenster bekommen eckige Ecken (focus.square_corners): ein eckiger Rahmen um
eine runde Win11-Ecke liesse dort den Hintergrund durchscheinen. Das geschieht bei jedem
Fokuswechsel fuer alle Agentenfenster, die es noch nicht haben, damit nebeneinander
gekachelte Fenster gleich aussehen, auch die noch nie fokussierten.
"""
import random
import threading
from collections.abc import Callable
from typing import Any

from deck.domain import focus_frame as ff
from deck.domain import wt
from deck.ops import log
from deck.ops.frame_helper import FrameHelper
from deck.platform import arrange as pa
from deck.platform import focus as wf


class _Win32:
    """Die echten Eingriffe. Jede Methode schlaegt ihr Ziel erst beim Aufruf nach, so
    greift das Schutzgitter aus tests/helpers.py auch hier."""

    def foreground(self) -> int:
        return wf.foreground_hwnd() or 0

    def describe(self, hwnd: int) -> tuple[str, str]:
        return wf.class_of(hwnd), wf.title_of(hwnd)

    def minimized(self, hwnd: int) -> bool:
        return wf.is_minimized(hwnd)

    def windows(self) -> list[tuple[int, str, bool]]:
        return pa.class_windows(wt.WINDOW_CLASS)

    def square(self, hwnd: int) -> None:
        wf.square_corners(hwnd)


class FocusFrame:
    WATCH_S = 0.06          # so oft schauen, welches Fenster vorn ist
    RESEND_S = 2.0          # den Stand so oft wiederholen: ein neu gestartetes
                            # Hilfsprogramm weiss sonst nicht, was es zeigen soll

    def __init__(self, is_agent: Callable[[str], bool], *, helper: Any = None,
                 win: Any = None, designs: tuple[str, ...] = ff.DESIGNS,
                 rnd: random.Random | None = None) -> None:
        self.is_agent = is_agent            # Fenstertitel -> gehoert zu einem WT-Block?
        self.helper = helper or FrameHelper()
        self.win = win or _Win32()
        self.designs = designs
        self.rnd = rnd or random.Random()
        self.design: str | None = None      # das gerade laufende Design
        self._target = 0                    # das Agentenfenster mit Rahmen
        self._seed = 0
        self._last_fg = 0
        self._since_send = 0.0
        self._squared: set[int] = set()
        self._stop = threading.Event()
        self._logged = False

    def start(self) -> None:
        threading.Thread(target=self._run, name="focus-frame", daemon=True).start()

    def stop(self) -> None:
        self._stop.set()
        self.helper.stop()

    def _run(self) -> None:
        while not self._stop.wait(self.WATCH_S):
            try:
                self.step(self.WATCH_S)
            except Exception:
                if not self._logged:             # einmal, nicht alle 60 ms
                    log.exc("focus_frame")
                    self._logged = True

    def _agent(self, fg: int) -> bool:
        if not fg:
            return False
        cls, title = self.win.describe(fg)
        return cls == wt.WINDOW_CLASS and self.is_agent(title) and not self.win.minimized(fg)

    def _square_agents(self) -> None:
        for hwnd, title, _ in self.win.windows():
            if hwnd not in self._squared and self.is_agent(title):
                self.win.square(hwnd)
                self._squared.add(hwnd)

    def _send(self) -> None:
        if self._target:
            self.helper.send({"cmd": "show", "hwnd": self._target, "design": self.design,
                              "seed": self._seed})
        else:
            self.helper.send({"cmd": "hide"})
        self._since_send = 0.0

    def step(self, elapsed: float = 0.0) -> None:
        """Einmal hinschauen; dem Hilfsprogramm nur Aenderungen sagen (und den Stand
        alle RESEND_S wiederholen)."""
        fg = self.win.foreground()
        if fg != self._last_fg:
            self._last_fg = fg
            self._square_agents()
        target = fg if self._agent(fg) else 0
        if target != self._target:
            self._target = target
            if target:                      # neuer Fokus: neues Design
                self.design = ff.pick(self.designs, self.design, self.rnd)
                self._seed = self.rnd.randrange(1 << 30)
            self._send()
            return
        self._since_send += elapsed
        if self._since_send >= self.RESEND_S:
            self._send()
