"""Der orange Rahmen um das Windows-Terminal-Fenster des Agenten, in das gerade getippt
wird. Er liegt genau auf der Aussenkante des Fensters, die Tab-Leiste eingeschlossen.

Ein eigenes, durchklickbares Fenster des Decks (platform/overlay) statt eines Shaders im
Terminal: der Shader malte nur in die Textflaeche, und die liegt hinter Innenabstand und
Scrollleiste (Begruendung in render/focus_ring.py).

Gezeigt wird der Rahmen nur, solange ein Agentenfenster das VORDERSTE Fenster ist. Dann
liegt es ohnehin ueber allen normalen Fenstern, und der Rahmen, der als TOPMOST darueber
liegt, verdeckt nichts Fremdes. Ein Blick alle 60 ms reicht, um einen Fokuswechsel zu
bemerken; steht der Rahmen, schaut er alle 20 ms nach, damit er beim Ziehen mitkommt.

Die Agentenfenster bekommen eckige Ecken (overlay.square_corners): ein eckiger Rahmen um
eine runde Win11-Ecke liesse dort den Hintergrund durchscheinen. Das geschieht bei jedem
Fokuswechsel fuer alle Agentenfenster, die es noch nicht haben, damit nebeneinander
gekachelte Fenster gleich aussehen, auch die noch nie fokussierten.
"""
from collections.abc import Callable
from typing import Any

from deck.domain import focus_frame as rules
from deck.domain import wt
from deck.ops import log
from deck.platform import arrange as pa
from deck.platform import dpi
from deck.platform import focus as wf
from deck.platform import overlay
from deck.render import focus_ring


class _Win32:
    """Die echten Eingriffe. Jede Methode schlaegt ihr Ziel erst beim Aufruf nach, so
    greift das Schutzgitter aus tests/helpers.py auch hier."""

    def foreground(self) -> int:
        return wf.foreground_hwnd() or 0

    def describe(self, hwnd: int) -> tuple[str, str]:
        return wf.class_of(hwnd), wf.title_of(hwnd)

    def minimized(self, hwnd: int) -> bool:
        return overlay.minimized(hwnd)

    def visible_rect(self, hwnd: int) -> tuple[int, int, int, int] | None:
        return overlay.visible_rect(hwnd)

    def factor(self, hwnd: int) -> float:
        return dpi.factor_for_window(hwnd)

    def windows(self) -> list[tuple[int, str, bool]]:
        return pa.class_windows(wt.WINDOW_CLASS)

    def square(self, hwnd: int) -> None:
        overlay.square_corners(hwnd)

    def create(self) -> int:
        return overlay.create()

    def show(self, ov: int, rect: tuple[int, int, int, int], bits: bytes) -> bool:
        return overlay.show(ov, rect, bits)

    def move(self, ov: int, x: int, y: int) -> bool:
        return overlay.move(ov, x, y)

    def hide(self, ov: int) -> None:
        overlay.hide(ov)


class FocusFrame:
    FOLLOW_MS = 20      # Rahmen steht: Ziehen und Groessenaenderung nachfahren
    WATCH_MS = 60       # kein Rahmen: nur auf den naechsten Fokuswechsel warten

    def __init__(self, root: Any, is_agent: Callable[[str], bool], win: Any = None) -> None:
        self.root = root
        self.is_agent = is_agent            # Fenstertitel -> gehoert zu einem WT-Block?
        self.win = win or _Win32()
        self._ov = 0                        # unser Rahmenfenster, erst bei Bedarf angelegt
        self._shown: rules.Frame | None = None
        self._failed: rules.Frame | None = None   # an diesem Ziel ist Zeigen gescheitert
        self._last_fg = 0
        self._squared: set[int] = set()
        self._job: Any = None
        self._logged = False

    def start(self) -> None:
        self._tick()

    def stop(self) -> None:
        if self._job is not None:
            try:
                self.root.after_cancel(self._job)
            except Exception:
                pass
            self._job = None
        if self._shown is not None:
            self.win.hide(self._ov)
            self._shown = None

    def _tick(self) -> None:
        try:
            self.step()
        except Exception:
            # Einmal ins Log, nicht im 20-ms-Takt. Der Rahmen geht dann aus, das Deck laeuft.
            if not self._logged:
                log.exc("focus_frame")
                self._logged = True
            if self._shown is not None:
                try:
                    self.win.hide(self._ov)
                except Exception:
                    pass
                self._shown = None
        self._job = self.root.after(self.FOLLOW_MS if self._shown else self.WATCH_MS,
                                    self._tick)

    def _target(self, fg: int) -> rules.Frame | None:
        """Wo der Rahmen stehen soll: um <fg>, wenn das ein Agentenfenster ist."""
        if not fg:
            return None
        cls, title = self.win.describe(fg)
        if cls != wt.WINDOW_CLASS or not self.is_agent(title) or self.win.minimized(fg):
            return None
        rect = self.win.visible_rect(fg)
        if rect is None:
            return None
        return rect, focus_ring.thickness(self.win.factor(fg))

    def _square_agents(self) -> None:
        for hwnd, title, _ in self.win.windows():
            if hwnd not in self._squared and self.is_agent(title):
                self.win.square(hwnd)
                self._squared.add(hwnd)

    def step(self) -> None:
        fg = self.win.foreground()
        if fg != self._last_fg:
            self._last_fg = fg
            self._square_agents()
        want = self._target(fg)
        act = rules.next_action(self._shown, want)
        if act == rules.KEEP:
            return
        if act == rules.HIDE:
            self.win.hide(self._ov)
            self._shown = None
            return
        if want is None or want == self._failed:
            return
        if not self._ov:
            self._ov = self.win.create()
            if not self._ov:
                self._failed = want
                return
        rect, t = want
        if act == rules.MOVE:
            ok = self.win.move(self._ov, rect[0], rect[1])
        else:
            bits = focus_ring.ring_bgra(rect[2] - rect[0], rect[3] - rect[1], t)
            ok = self.win.show(self._ov, rect, bits)
        self._shown = want if ok else None
        self._failed = None if ok else want
