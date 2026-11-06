"""Fremde Fenster auf einen Monitor holen und dort maximieren oder kacheln (Win32).

Der Handwerksteil hinter „VS-Code-Fenster einsammeln" und hinter dem Kachelraster der
Windows-Terminal-Agenten (`place_window`, unten); WELCHE Fenster gemeint sind und
wohin, entscheidet `domain/arrange.py` (diese Schicht darf domain nicht kennen), die
Bilanz ziehen `ops/window_gather.py` bzw. `ops/wt_host.py`.

**Der Umzug braucht ZWEI Schritte, und das ist gemessen.** Naheliegend wäre ein
einzelnes SetWindowPlacement mit `showCmd = SW_SHOWMAXIMIZED` und einem
rcNormalPosition auf dem Zielmonitor. Der Aufruf meldet dann auch brav Erfolg - und das
Fenster bleibt trotzdem auf seinem alten Schirm maximiert liegen (nachgemessen am
2026-08-19: rect vor und nach dem Aufruf identisch). Windows hält für ein maximiertes
Fenster den Maximierungs-Monitor fest; rcNormalPosition beschreibt nur, wohin es beim
Wiederherstellen zurückfiele. Also erst `SW_SHOWNORMAL` auf die Zielfläche - damit
zieht das Fenster wirklich um -, dann `SW_SHOWMAXIMIZED`, das auf dem Monitor
maximiert, auf dem es jetzt liegt.

Warum SetWindowPlacement und nicht ShowWindow+SetWindowPos: es nimmt Position und
Zustand in EINEM Aufruf, arbeitet auch am minimierten Fenster (aus dem Symbol heraus
direkt maximiert auf dem Ziel - ebenfalls gemessen) und zieht das Fenster nicht
ungefragt in den Vordergrund.

Alle Koordinaten sind ECHTE Bildschirmpixel - das gilt nur, weil das Panel per
`dpi.enable()` als Per-Monitor-V2 angemeldet ist. Ein Aufrufer ohne diese Anmeldung
bekommt von Windows virtualisierte Zahlen (bei 150 % meldet derselbe Monitor
2560x1392 statt 3840x2088) und legt das Fenster damit irgendwohin. Wer diese
Funktionen aus einem eigenen Skript benutzt, ruft vorher `dpi.enable()`.
"""
import ctypes
from ctypes import wintypes
from typing import Any, NamedTuple

from deck.platform.win32 import dwmapi, kernel32, user32

_GWL_STYLE = -16
_WM_CLOSE = 0x0010
_SW_SHOWNORMAL = 1
_SW_SHOWMAXIMIZED = 3
_PROCESS_QUERY_LIMITED_INFORMATION = 0x1000     # reicht für den Prozesspfad, ab Vista

_EnumProc = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)


class _WINDOWPLACEMENT(ctypes.Structure):
    _fields_ = [("length", wintypes.UINT),
                ("flags", wintypes.UINT),
                ("showCmd", wintypes.UINT),
                ("ptMinPosition", wintypes.POINT),
                ("ptMaxPosition", wintypes.POINT),
                ("rcNormalPosition", wintypes.RECT)]


# Signaturen, die win32.py noch nicht kennt - nach derselben Regel wie dort: alles, was
# Handles führt, wird typisiert, sonst schneidet ctypes sie auf 32 Bit ab.
user32.EnumWindows.argtypes = [_EnumProc, wintypes.LPARAM]
user32.GetWindowTextLengthW.argtypes = [wintypes.HWND]
user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
user32.IsWindowVisible.argtypes = [wintypes.HWND]
user32.GetWindowLongW.argtypes = [wintypes.HWND, ctypes.c_int]
user32.GetWindowLongW.restype = wintypes.LONG
user32.GetWindowPlacement.argtypes = [wintypes.HWND, ctypes.POINTER(_WINDOWPLACEMENT)]
user32.GetWindowPlacement.restype = wintypes.BOOL
user32.SetWindowPlacement.argtypes = [wintypes.HWND, ctypes.POINTER(_WINDOWPLACEMENT)]
user32.SetWindowPlacement.restype = wintypes.BOOL
kernel32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
kernel32.OpenProcess.restype = wintypes.HANDLE
kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
if hasattr(kernel32, "QueryFullProcessImageNameW"):
    kernel32.QueryFullProcessImageNameW.argtypes = [
        wintypes.HANDLE, wintypes.DWORD, wintypes.LPWSTR, ctypes.POINTER(wintypes.DWORD)]
    kernel32.QueryFullProcessImageNameW.restype = wintypes.BOOL


class Win(NamedTuple):
    """Ein sichtbares Top-Level-Fenster, so weit es die Auswahl braucht."""
    hwnd: int
    title: str
    exe: str        # voller Pfad der ausführenden Datei ("" = nicht ermittelbar)
    style: int      # Fensterstil-Bits (GWL_STYLE)


def _title(hwnd: int) -> str:
    n = user32.GetWindowTextLengthW(hwnd)
    if n == 0:
        return ""
    buf = ctypes.create_unicode_buffer(n + 1)
    user32.GetWindowTextW(hwnd, buf, n + 1)
    return buf.value


def exe_of(hwnd: int) -> str:
    """Voller Pfad der ausführenden Datei hinter <hwnd>, oder "".

    Leer heisst „nicht ermittelbar", nicht „kein Prozess": Fenster von Diensten oder
    höher rechtenden Programmen geben ihren Pfad nicht heraus. Der Aufrufer fällt
    dann auf den Titel zurück (domain.arrange.is_editor_window).
    """
    if not hasattr(kernel32, "QueryFullProcessImageNameW"):
        return ""
    pid = wintypes.DWORD()
    user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
    if not pid.value:
        return ""
    handle = kernel32.OpenProcess(_PROCESS_QUERY_LIMITED_INFORMATION, False, pid.value)
    if not handle:
        return ""
    try:
        buf = ctypes.create_unicode_buffer(1024)
        size = wintypes.DWORD(len(buf))
        if kernel32.QueryFullProcessImageNameW(handle, 0, buf, ctypes.byref(size)):
            return buf.value
        return ""
    finally:
        kernel32.CloseHandle(handle)


def visible_windows() -> list[Win]:
    """Alle sichtbaren Top-Level-Fenster mit Titel, samt Prozesspfad und Stil.

    Minimierte Fenster sind dabei (WS_VISIBLE bleibt bei minimiert gesetzt) - genau
    die will man beim Einsammeln ja mitnehmen.
    """
    out: list[Win] = []

    def cb(hwnd: int, _: Any) -> bool:
        if user32.IsWindowVisible(hwnd):
            title = _title(hwnd)
            if title:
                # Stil unsigned halten: GetWindowLongW liefert LONG, und ein
                # gesetztes oberstes Bit käme sonst als negative Zahl an (der
                # Bit-Test stimmte, die Zahl im Log wäre Unsinn).
                style = int(user32.GetWindowLongW(hwnd, _GWL_STYLE)) & 0xFFFFFFFF
                out.append(Win(int(hwnd), title, exe_of(hwnd), style))
        return True

    user32.EnumWindows(_EnumProc(cb), 0)
    return out


def normal_size(hwnd: int) -> tuple[int, int] | None:
    """Gemerkte Größe (Breite, Höhe) des NICHT maximierten Fensters, oder None.

    Das ist die Größe, mit der es nach einem „Wiederherstellen" erscheint - auch
    während es maximiert oder minimiert ist. GetWindowRect liefert stattdessen die
    Größe des aktuellen Zustands und wäre hier die falsche Zahl.
    """
    wp = _WINDOWPLACEMENT()
    wp.length = ctypes.sizeof(_WINDOWPLACEMENT)
    if not user32.GetWindowPlacement(wintypes.HWND(hwnd), ctypes.byref(wp)):
        return None
    r = wp.rcNormalPosition
    return (r.right - r.left, r.bottom - r.top)


def move_maximized(hwnd: int, rect: tuple[int, int, int, int]) -> bool:
    """<hwnd> auf <rect> = (x, y, w, h) legen und dort maximieren. True = geschafft.

    <rect> ist das Wiederherstellungs-Rechteck und bestimmt über seinen Monitor, WO
    maximiert wird. Die zwei Schritte sind Pflicht, siehe Modulkopf - der zweite ohne
    den ersten ist ein Aufruf, der Erfolg meldet und nichts tut.
    """
    x, y, w, h = (int(v) for v in rect)
    wp = _WINDOWPLACEMENT()
    wp.length = ctypes.sizeof(_WINDOWPLACEMENT)
    handle = wintypes.HWND(hwnd)
    if not user32.GetWindowPlacement(handle, ctypes.byref(wp)):
        return False
    wp.rcNormalPosition = wintypes.RECT(x, y, x + w, y + h)
    wp.showCmd = _SW_SHOWNORMAL              # 1. entmaximieren -> zieht wirklich um
    if not user32.SetWindowPlacement(handle, ctypes.byref(wp)):
        return False
    wp.showCmd = _SW_SHOWMAXIMIZED           # 2. auf dem Zielmonitor maximieren
    return bool(user32.SetWindowPlacement(handle, ctypes.byref(wp)))


def post_close(hwnd: int) -> bool:
    """Ein Fenster zum Schliessen auffordern (WM_CLOSE) - dasselbe, was Alt+F4 tut.

    Gebraucht beim Neustart (`ops/vscode_restart`) fuer VS-Code-Fenster, die das Deck
    NICHT kennt: an sie kommt kein Kommando ueber die Extension, sie halten aber den
    Prozess offen, auf dessen Ende gewartet wird.

    Bewusst PostMessage und nicht SendMessage: SendMessage wartet, bis das fremde
    Fenster geantwortet hat, und haengt genau dann, wenn es haengt - also im
    interessanten Fall. Bewusst auch kein TerminateProcess: WM_CLOSE laesst VS Code
    seine ungespeicherten Dateien selbst abfragen. Ein True heisst darum nur „gefragt",
    nicht „geschlossen"; wer es wissen muss, sieht danach in die Prozessliste
    (platform/procs).
    """
    try:
        return bool(user32.PostMessageW(wintypes.HWND(hwnd), _WM_CLOSE, 0, 0))
    except Exception:
        return False


# ── Agentenfenster kacheln (ops/wt_host) ────────────────────────────────────
_SW_SHOWNOACTIVATE = 4
_SWP_NOZORDER = 0x0004
_SWP_NOACTIVATE = 0x0010
_DWMWA_EXTENDED_FRAME_BOUNDS = 9

user32.GetClassNameW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
user32.IsIconic.argtypes = [wintypes.HWND]
user32.IsIconic.restype = wintypes.BOOL
user32.IsZoomed.argtypes = [wintypes.HWND]
user32.IsZoomed.restype = wintypes.BOOL
user32.GetWindowRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]
user32.GetWindowRect.restype = wintypes.BOOL
user32.SetWindowPos.argtypes = [wintypes.HWND, wintypes.HWND, ctypes.c_int, ctypes.c_int,
                                ctypes.c_int, ctypes.c_int, wintypes.UINT]
user32.SetWindowPos.restype = wintypes.BOOL
if dwmapi is not None:
    dwmapi.DwmGetWindowAttribute.argtypes = [wintypes.HWND, wintypes.DWORD,
                                             ctypes.c_void_p, wintypes.DWORD]
    dwmapi.DwmGetWindowAttribute.restype = ctypes.c_long   # HRESULT


def class_windows(cls: str) -> list[tuple[int, str, bool]]:
    """Sichtbare Top-Level-Fenster der Fensterklasse <cls>: (hwnd, Titel, minimiert)."""
    out: list[tuple[int, str, bool]] = []

    def cb(hwnd: int, _: Any) -> bool:
        if user32.IsWindowVisible(hwnd):
            buf = ctypes.create_unicode_buffer(256)
            if user32.GetClassNameW(hwnd, buf, 256) and buf.value == cls:
                out.append((int(hwnd), _title(hwnd), bool(user32.IsIconic(hwnd))))
        return True

    user32.EnumWindows(_EnumProc(cb), 0)
    return out


def window_rect(hwnd: int) -> tuple[int, int, int, int] | None:
    """Aussenrechteck (links, oben, rechts, unten) samt unsichtbarem Rand, oder None."""
    r = wintypes.RECT()
    if not user32.GetWindowRect(wintypes.HWND(hwnd), ctypes.byref(r)):
        return None
    return (r.left, r.top, r.right, r.bottom)


def _frame_insets(hwnd: int) -> tuple[int, int, int, int]:
    """Wie weit das Aussenrechteck ueber den SICHTBAREN Rahmen hinausragt (links,
    oben, rechts, unten). Windows 10/11 legt um jedes Fenster einen unsichtbaren
    Greifrand von einigen Pixeln; wer ihn nicht abzieht, bekommt zwischen zwei
    Kacheln eine Luecke. Ohne DWM-Auskunft: keiner."""
    outer = window_rect(hwnd)
    if outer is None or dwmapi is None:
        return (0, 0, 0, 0)
    vis = wintypes.RECT()
    hr = dwmapi.DwmGetWindowAttribute(wintypes.HWND(hwnd), _DWMWA_EXTENDED_FRAME_BOUNDS,
                                      ctypes.byref(vis), ctypes.sizeof(vis))
    if hr != 0:
        return (0, 0, 0, 0)
    return (vis.left - outer[0], vis.top - outer[1],
            outer[2] - vis.right, outer[3] - vis.bottom)


def place_window(hwnd: int, rect: tuple[int, int, int, int]) -> bool:
    """<hwnd> so legen, dass sein SICHTBARER Rahmen genau <rect> = (x, y, w, h) fuellt,
    ohne es nach vorn zu holen. True = Windows hat den Auftrag angenommen.

    Ein maximiertes Fenster wird erst wiederhergestellt (sonst klebt es, siehe
    move_maximized). Gelegt wird ZWEIMAL: zieht das Fenster dabei auf einen Monitor mit
    anderer Skalierung, passt es beim ersten Mal seine Groesse an die neue DPI an, und
    erst der zweite Aufruf trifft die Kachel.
    """
    x, y, w, h = (int(v) for v in rect)
    handle = wintypes.HWND(hwnd)
    if user32.IsZoomed(handle):
        user32.ShowWindow(handle, _SW_SHOWNOACTIVATE)
    ok = False
    for _ in range(2):
        left, top, right, bottom = _frame_insets(hwnd)
        ok = bool(user32.SetWindowPos(handle, None, x - left, y - top,
                                      w + left + right, h + top + bottom,
                                      _SWP_NOZORDER | _SWP_NOACTIVATE))
    return ok
