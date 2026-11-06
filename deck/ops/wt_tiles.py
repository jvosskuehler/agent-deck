"""Die Windows-Terminal-Fenster eines Repo-Blocks gleich gross nebeneinander legen.

Jeder WT-Agent hat ein eigenes Fenster (Begruendung im Kopf von domain/wt.py); dieses
Modul legt sie als Kacheln aus, das Raster rechnet domain/arrange.tile_grid. Ausgeloest
wird es von ops/wt_host, sobald sich die Menge der lebenden Agenten eines Blocks
aendert: ein Agent kommt dazu oder geht. Von Hand verschobene Fenster bleiben bis dahin
liegen, wo der Nutzer sie hingelegt hat.

Gefunden wird ein Agentenfenster an seinem TITEL - er ist der Slot-Name, festgenagelt
mit `--suppressApplicationTitle`. Minimierte Fenster bleiben aussen vor: wer eines
weglegt, will es nicht beim naechsten Agenten zurueck in die Kachel gezogen bekommen.

Wie viele nebeneinander, entscheidet die Breite des Monitors in Punkten
(domain/arrange.max_cols_for): auf dem 4K-Schirm vier, auf dem Laptop zwei je Reihe.

Auf welchen Monitor? Auf den des Fensters mit der kleinsten Nummer. Es ist das aelteste,
also das, das der Nutzer am ehesten schon dorthin gezogen hat, wo der Block leben soll;
ein neues Fenster oeffnet Windows Terminal dagegen irgendwo.
"""
from collections.abc import Callable, Iterable

from deck.domain import arrange as rules
from deck.domain import wt
from deck.platform import arrange as pa
from deck.platform import dpi
from deck.platform import monitor


def _area_of(hwnd: int) -> tuple[int, int, int, int] | None:
    """Arbeitsflaeche des Monitors, auf dem die Mitte von <hwnd> liegt."""
    r = pa.window_rect(hwnd)
    if r is None:
        return None
    return monitor.work_area((r[0] + r[2]) // 2, (r[1] + r[3]) // 2)


class WtTiler:
    def __init__(self, *, windows: Callable | None = None, place: Callable | None = None,
                 area_of: Callable | None = None,
                 factor_of: Callable | None = None) -> None:
        self.windows = windows or (lambda: pa.class_windows(wt.WINDOW_CLASS))
        # Erst beim Aufruf nachgeschlagen statt als Default gebunden: so greift das
        # Schutzgitter der Tests (tests/helpers.py) auch hier.
        self.place = place or (lambda hwnd, rect: pa.place_window(hwnd, rect))
        self.area_of = area_of or _area_of
        self.factor_of = factor_of or dpi.factor_for_window
        self._seen: dict[str, tuple[str, ...]] = {}

    def hwnd_of(self, slot: str | None) -> int | None:
        """Fenster des Agenten <slot>, auch minimiert (fokussieren holt es zurueck)."""
        for hwnd, title, _ in self.windows():
            if slot and wt.slot_from_title(title) == slot:
                return hwnd
        return None

    def changed(self, win: str, slots: Iterable[str]) -> bool:
        """Leben in Block <win> andere Agenten als beim letzten Blick? Ein Block, der
        leer geworden ist, braucht keine Kacheln mehr - dann False."""
        cur = tuple(slots)
        if self._seen.get(win) == cur:
            return False
        self._seen[win] = cur
        return bool(cur)

    def tile(self, slots: list[str]) -> int:
        """Die Fenster dieser Agenten in Slot-Reihenfolge kacheln. Gibt zurueck, wie
        viele Fenster gelegt wurden."""
        found: dict[str, int] = {}
        for hwnd, title, minimized in self.windows():
            slot = wt.slot_from_title(title)
            if slot in slots and not minimized and slot not in found:
                found[slot] = hwnd
        order = [found[s] for s in slots if s in found]
        if not order:
            return 0
        area = self.area_of(order[0])
        if not area:
            return 0
        cols = rules.max_cols_for(area[2] - area[0], self.factor_of(order[0]))
        for hwnd, rect in zip(order, rules.tile_grid(len(order), area, cols)):
            self.place(hwnd, rect)
        return len(order)
