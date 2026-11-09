"""Tooltips und Dialoge auf dem Monitor halten, auf dem sie erscheinen.

Tk kennt nur EINEN Bildschirm: winfo_screenwidth/height liefern immer die Größe
des PRIMÄR-Monitors. Damit zu klemmen ist schlimmer, als nicht zu klemmen – auf
einem zweiten Monitor zieht es Tooltip und Dialog fälschlich auf den Hauptschirm
zurück (genau daran ist der erste Versuch gescheitert, siehe Tooltip in
canvas_kit). Windows dagegen weiß, welcher Monitor unter einem Punkt liegt und
wie groß dort die ARBEITSFLÄCHE ist (Monitorfläche minus Taskleiste) – das fragt
work_area() ab, und nur dagegen wird geklemmt.

fit() ist die reine Rechnung und darum ohne Fenster testbar. Bevorzugte Position
ist Anker + Versatz. Passt das Fenster dort nicht mehr ganz auf die
Arbeitsfläche, wird es auf die ANDERE Seite des Ankers gespiegelt: ein Tooltip am
rechten Rand klappt also nach links vom Mauszeiger, statt unter ihm zu kleben,
und der Einstellungs-Dialog erscheint links neben einem rechts angedockten Deck.
Passt es auch gespiegelt nicht, wird an den Rand geklemmt – dann ist der
Bildschirm einfach kleiner als das Fenster.

Alle Koordinaten sind Bildschirmpixel im selben Raum wie wm_geometry und
winfo_pointerx/y. Nach hidpi.enable() ist das auch der Raum von Win32 (der
Prozess wird nicht mehr DPI-virtualisiert), die beiden Welten sind also direkt
vergleichbar.
"""
import tkinter as tk
from typing import Any

# Windows-Zugriff bewusst best effort: fehlt er (anderes OS, alter Build), liefert
# work_area() None und die Aufrufer platzieren wie früher ungeklemmt.
try:
    import ctypes
    from ctypes import wintypes

    _user32: ctypes.WinDLL | None = ctypes.WinDLL("user32", use_last_error=True)
except Exception:
    _user32 = None

_MONITOR_DEFAULTTONEAREST = 2      # Punkt außerhalb aller Monitore -> nächstgelegener
_GA_ROOT = 2                       # von der Tk-Fenster-Id zum echten Top-Level-Fenster

if _user32 is not None:
    class _MONITORINFO(ctypes.Structure):
        _fields_ = [("cbSize", wintypes.DWORD),
                    ("rcMonitor", wintypes.RECT),
                    ("rcWork", wintypes.RECT),
                    ("dwFlags", wintypes.DWORD)]

    _user32.MonitorFromPoint.argtypes = [wintypes.POINT, wintypes.DWORD]
    _user32.MonitorFromPoint.restype = wintypes.HANDLE
    _user32.GetMonitorInfoW.argtypes = [wintypes.HANDLE, ctypes.POINTER(_MONITORINFO)]
    _user32.GetMonitorInfoW.restype = wintypes.BOOL
    _user32.GetAncestor.argtypes = [wintypes.HWND, wintypes.UINT]
    _user32.GetAncestor.restype = wintypes.HWND
    _user32.GetWindowRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]
    _user32.GetCursorPos.argtypes = [ctypes.POINTER(wintypes.POINT)]
    _user32.GetCursorPos.restype = wintypes.BOOL


def work_area(x: int, y: int) -> tuple[int, int, int, int] | None:
    """Arbeitsfläche des Monitors unter dem Punkt (x, y) als (links, oben, rechts,
    unten) – Monitorfläche ohne Taskleiste, damit nichts hinter ihr landet.

    None heißt für die Aufrufer bewusst „nicht klemmen": lieber ein Tooltip, der
    am Rand übersteht, als einer, den eine geratene Bildschirmgröße auf den
    falschen Monitor zieht."""
    if _user32 is None:
        return None
    try:
        mon = _user32.MonitorFromPoint(wintypes.POINT(int(x), int(y)),
                                       _MONITOR_DEFAULTTONEAREST)
        if not mon:
            return None
        info = _MONITORINFO()
        info.cbSize = ctypes.sizeof(_MONITORINFO)
        if not _user32.GetMonitorInfoW(mon, ctypes.byref(info)):
            return None
        r = info.rcWork
        if r.right <= r.left or r.bottom <= r.top:
            return None
        return (int(r.left), int(r.top), int(r.right), int(r.bottom))
    except Exception:
        return None


def cursor_pos() -> tuple[int, int] | None:
    """Wo der Mauszeiger steht, in Bildschirmpixeln. None, wenn Windows es nicht sagt
    (etwa auf dem Sperrbildschirm)."""
    if _user32 is None:
        return None
    try:
        pt = wintypes.POINT()
        if not _user32.GetCursorPos(ctypes.byref(pt)):
            return None
        return (int(pt.x), int(pt.y))
    except Exception:
        return None


def primary_work_area() -> tuple[int, int, int, int] | None:
    """Arbeitsfläche des HAUPTMONITORS (Windows: „Hauptanzeige").

    Gefragt wird über den Punkt (0, 0), und das ist keine Näherung: im virtuellen
    Bildschirmraum liegt die linke obere Ecke des Hauptmonitors per Definition auf
    (0, 0) — die anderen Monitore ordnen sich mit positiven oder negativen
    Koordinaten darum an. Nachgemessen an drei Monitoren: (0, 0) trifft den mit dem
    Flag MONITORINFOF_PRIMARY.
    """
    return work_area(0, 0)


# Zuletzt gemessene Dekorationsmaße (Rand + Titelleiste). Sie hängen am Windows-Thema,
# nicht am einzelnen Fenster – ein einmal gemessener Wert taugt darum als Vorabschätzung
# für den nächsten, noch unsichtbaren Dialog.
_pad_seen = (0, 0)


def _frame_pad(win: Any) -> tuple[int, int]:
    """Um wie viel ein Fenster GRÖSSER ist als sein Inhalt: Rand + Titelleiste.

    Nötig, weil wm_geometry die AUSSENKANTE positioniert, winfo_reqwidth/height aber
    nur den Inhalt messen – ein Dialog stand deshalb am unteren Rand um die Höhe
    seiner Titelleiste zu tief (bei Windows 11 gut 39 px). Randlose Fenster
    (Tooltips, overrideredirect) haben keine Dekoration -> (0, 0).

    Messbar ist der Rahmen erst am sichtbaren Fenster; für ein noch nie gezeigtes
    kommt die Schätzung aus dem letzten gemessenen (siehe _pad_seen)."""
    global _pad_seen
    try:
        if win.overrideredirect():
            return (0, 0)
        if _user32 is None or not win.winfo_ismapped():
            return _pad_seen
        r = wintypes.RECT()
        hwnd = _user32.GetAncestor(win.winfo_id(), _GA_ROOT) or win.winfo_id()
        if not _user32.GetWindowRect(hwnd, ctypes.byref(r)):
            return _pad_seen
        pad = (max(0, (r.right - r.left) - win.winfo_width()),
               max(0, (r.bottom - r.top) - win.winfo_height()))
        if pad != (0, 0):
            _pad_seen = pad
        return pad
    except Exception:
        return (0, 0)


def _axis(anchor: int, size: int, off: int, lo: int, hi: int) -> int:
    """Eine Achse platzieren: bevorzugt anchor+off; reicht das über hi hinaus, auf
    die andere Seite des Ankers spiegeln (anchor-off-size); passt es auch dort
    nicht ganz, an den näheren Rand klemmen."""
    p = anchor + off
    if p + size <= hi:
        return max(lo, p)                  # Normalfall (nur gegen lo gesichert)
    mirror = anchor - off - size
    if mirror >= lo:
        return mirror
    return max(lo, hi - size)


def fit(ax: int, ay: int, w: int, h: int,
        area: tuple[int, int, int, int] | None, *,
        dx: int = 0, dy: int = 0) -> tuple[int, int]:
    """(x, y) für ein Fenster der Größe w×h am Anker (ax, ay), versetzt um (dx, dy)
    und in <area> = (l, t, r, b) gehalten. area=None -> Anker + Versatz, ungeklemmt."""
    if not area:
        return int(ax + dx), int(ay + dy)
    left, top, right, bottom = area
    return (int(_axis(ax, w, dx, left, right)), int(_axis(ay, h, dy, top, bottom)))


def place(win: Any, ax: int, ay: int, *,
          dx: int = 0, dy: int = 0) -> tuple[int, int] | None:
    """<win> (Toplevel) an den Anker (ax, ay) legen, versetzt um (dx, dy), und dabei
    auf dem Monitor unter dem Anker halten. Gibt die gesetzte Position zurück
    (None, wenn das Fenster gerade nicht mehr existiert).

    update_idletasks steckt hier drin, weil die Platzierung die fertige Größe
    braucht: ein Dialog, dessen Widgets noch nicht durchgerechnet sind, meldet
    1x1 und würde nie geklemmt. Aufrufer sollten das Fenster deshalb erst NACH
    dem Aufbau platzieren (und bis dahin withdrawn halten, sonst sieht man den
    Sprung). Gerechnet wird mit der AUSSENgröße (Inhalt + Dekoration), weil
    wm_geometry die Außenkante setzt – siehe _frame_pad."""
    try:
        win.update_idletasks()
        pad_w, pad_h = _frame_pad(win)
        x, y = fit(ax, ay, win.winfo_reqwidth() + pad_w, win.winfo_reqheight() + pad_h,
                   work_area(ax, ay), dx=dx, dy=dy)
        win.wm_geometry(f"+{x}+{y}")
        return x, y
    except tk.TclError:
        return None
