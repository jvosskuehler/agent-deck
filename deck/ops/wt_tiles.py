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


def _here() -> tuple[tuple[int, int, int, int], float] | None:
    """Arbeitsflaeche und Skalierung des Monitors unter dem Mauszeiger."""
    pos = monitor.cursor_pos()
    area = monitor.work_area(*pos) if pos else None
    if not pos or not area:
        return None
    return area, dpi.factor_at(*pos)


class WtTiler:
    def __init__(self, *, windows: Callable | None = None, place: Callable | None = None,
                 area_of: Callable | None = None,
                 factor_of: Callable | None = None, here: Callable | None = None,
                 restore: Callable | None = None) -> None:
        self.windows = windows or (lambda: pa.class_windows(wt.WINDOW_CLASS))
        # Erst beim Aufruf nachgeschlagen statt als Default gebunden: so greift das
        # Schutzgitter der Tests (tests/helpers.py) auch hier.
        self.place = place or (lambda hwnd, rect: pa.place_window(hwnd, rect))
        self.restore = restore or (lambda hwnd: pa.restore_window(hwnd))
        self.area_of = area_of or _area_of
        self.factor_of = factor_of or dpi.factor_for_window
        self.here = here or _here
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

    def gather(self, slots: list[str]) -> list[tuple[str, int]]:
        """Taste „alle nach vorn“: die Fenster dieser Agenten, ueber alle Bloecke hinweg,
        als EIN Raster in Slot-Reihenfolge auf den Monitor unter der Maus legen.

        Anders als `tile` auch die minimierten: sie werden wiederhergestellt, denn wer
        alle holt, meint alle. Ein Raster statt eines je Block, weil sich zwei Bloecke
        auf demselben Monitor sonst ueberdecken. Weiss Windows nicht, wo die Maus
        steht, bleiben die Fenster liegen, wo sie sind (nach vorn kommen sie trotzdem).

        Rueckgabe: (Slot, Fenster) in Slot-Reihenfolge; nach vorn holt der Aufrufer."""
        found: dict[str, tuple[int, bool]] = {}
        for hwnd, title, minimized in self.windows():
            slot = wt.slot_from_title(title)
            if slot in slots and slot not in found:
                found[slot] = (hwnd, minimized)
        order = [(s, *found[s]) for s in slots if s in found]
        for _, hwnd, minimized in order:
            if minimized:
                self.restore(hwnd)
        spot = self.here() if order else None
        if spot:
            area, factor = spot
            cols = rules.max_cols_for(area[2] - area[0], factor)
            grid = rules.tile_grid(len(order), area, cols)
            for (_, hwnd, _), rect in zip(order, grid, strict=True):
                self.place(hwnd, rect)
        return [(s, hwnd) for s, hwnd, _ in order]
