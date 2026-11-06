"""Windows: Fenster nach vorn holen + native Titelleiste umstylen (reines ctypes).

Wird von der App benutzt, um beim Klick auf eine Kachel das richtige VS-Code-
Fenster zu finden und nach vorn zu holen (find_window + focus_window) sowie die
eigene Titelleiste dunkel zu stylen (style_titlebar). Das Fokussieren des
einzelnen Terminals (Pane) macht die Extension per Broker – nicht mehr dieses
Modul.
"""
import ctypes
from ctypes import wintypes
from typing import Any

from deck.platform.win32 import dwmapi, kernel32, user32

# ── Fenster anhand des Titels finden ────────────────────
_EnumProc = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
user32.EnumWindows.argtypes = [_EnumProc, wintypes.LPARAM]
user32.GetWindowTextLengthW.argtypes = [wintypes.HWND]
user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
user32.IsWindowVisible.argtypes = [wintypes.HWND]
user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]


def _title(hwnd: int) -> str:
    n = user32.GetWindowTextLengthW(hwnd)
    if n == 0:
        return ""
    buf = ctypes.create_unicode_buffer(n + 1)
    user32.GetWindowTextW(hwnd, buf, n + 1)
    return buf.value


def find_window(*needles: str) -> int | None:
    """HWND des ersten sichtbaren Fensters, dessen Titel ALLE needles enthaelt."""
    hits = []

    def cb(hwnd: int, _: Any) -> bool:
        if user32.IsWindowVisible(hwnd):
            t = _title(hwnd)
            if t and all(nd.lower() in t.lower() for nd in needles):
                hits.append(hwnd)
        return True

    user32.EnumWindows(_EnumProc(cb), 0)
    return hits[0] if hits else None


def list_titles(*needles: str) -> list[str]:
    """Titel ALLER sichtbaren Fenster, deren Titel ALLE needles (case-insensitiv)
    enthaelt. Gedacht, um festzustellen, welche VS-Code-Fenster ueberhaupt noch offen
    sind (z.B. list_titles(VSCODE_MARKER)) – ein geschlossenes Fenster taucht nicht mehr
    auf, ein bloss neu ladendes/minimiertes schon (WS_VISIBLE bleibt bei minimiert)."""
    out = []

    def cb(hwnd: int, _: Any) -> bool:
        if user32.IsWindowVisible(hwnd):
            t = _title(hwnd)
            if t and all(nd.lower() in t.lower() for nd in needles):
                out.append(t)
        return True

    user32.EnumWindows(_EnumProc(cb), 0)
    return out


user32.GetForegroundWindow.restype = wintypes.HWND


def foreground_hwnd() -> int:
    """HWND des aktuell aktiven Fensters (fuer 'klick-zum-Verbinden')."""
    return user32.GetForegroundWindow()


def title_of(hwnd: int) -> str:
    """Fenstertitel zu einem HWND (leerer String, wenn keiner)."""
    return _title(hwnd) if hwnd else ""


user32.GetClassNameW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]


def class_of(hwnd: int) -> str:
    """Fensterklasse zu einem HWND (leerer String, wenn keine). Windows Terminal etwa
    heisst immer CASCADIA_HOSTING_WINDOW_CLASS - verlaesslicher als jeder Titel."""
    if not hwnd:
        return ""
    buf = ctypes.create_unicode_buffer(256)
    return buf.value if user32.GetClassNameW(hwnd, buf, 256) else ""


# ── Tk-Fensterhandle aufloesen ──────────────────────────
_GA_ROOT = 2
user32.GetAncestor.argtypes = [wintypes.HWND, wintypes.UINT]
user32.GetAncestor.restype = wintypes.HWND


def toplevel_hwnd(child_id: int) -> int:
    """Von der Tk-winfo_id zum echten Top-Level-Fensterhandle (GA_ROOT)."""
    return user32.GetAncestor(child_id, _GA_ROOT) or child_id


# ── Native Titelleiste per DWM umstylen (Win11) ─────────
# Aus der grauen Standard-Leiste wird eine dunkle mit Cyan-Rand + runden Ecken.
_DWMWA_USE_IMMERSIVE_DARK_MODE = 20   # BOOL – dunkle Leiste (Win10 20H1+/Win11)
_DWMWA_WINDOW_CORNER_PREFERENCE = 33  # int  – 2 = runde Ecken
_DWMWA_BORDER_COLOR = 34              # COLORREF (0x00BBGGRR) – Fensterrand
_DWMWA_CAPTION_COLOR = 35             # COLORREF – Fuellung der Titelleiste
_DWMWA_TEXT_COLOR = 36                # COLORREF – Titeltext
_DWMWCP_ROUND = 2
_DWMWCP_ROUNDSMALL = 3                # kleiner Radius (~4 px) statt Standard (~8 px)


def _colorref(hexstr: str) -> int:
    """'#RRGGBB' -> Windows-COLORREF (0x00BBGGRR)."""
    h = hexstr.lstrip("#")
    r, g, b = int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16)
    return r | (g << 8) | (b << 16)


def _dwm_set(hwnd: int, attr: int, value: Any) -> None:
    if not dwmapi:
        return
    val = ctypes.c_int(int(value))
    try:
        dwmapi.DwmSetWindowAttribute(wintypes.HWND(int(hwnd)), attr,
                                     ctypes.byref(val), ctypes.sizeof(val))
    except Exception:
        pass   # aeltere Builds kennen das Attribut nicht -> leise ignorieren


def style_titlebar(hwnd: int, *, dark: bool = True, border: str | None = None,
                   caption: str | None = None, text: str | None = None,
                   round_corners: bool = True) -> None:
    """Native Titelleiste (Win11-DWM) umstylen: Dark-Mode, farbiger Rand, runde
    Ecken, dunkle Caption + heller Titeltext. Jede Eigenschaft ist einzeln
    gekapselt und faellt auf aelteren Windows leise auf die Standard-Leiste
    zurueck (border/caption/text brauchen Win11 22000+)."""
    if not hwnd or not dwmapi:
        return
    if dark:
        _dwm_set(hwnd, _DWMWA_USE_IMMERSIVE_DARK_MODE, 1)
        _dwm_set(hwnd, 19, 1)   # aeltere Builds nutzten Attribut 19 fuer Dark-Mode
    if round_corners:
        _dwm_set(hwnd, _DWMWA_WINDOW_CORNER_PREFERENCE, _DWMWCP_ROUND)
    if border is not None:
        _dwm_set(hwnd, _DWMWA_BORDER_COLOR, _colorref(border))
    if caption is not None:
        _dwm_set(hwnd, _DWMWA_CAPTION_COLOR, _colorref(caption))
    if text is not None:
        _dwm_set(hwnd, _DWMWA_TEXT_COLOR, _colorref(text))


def round_corners(hwnd: int, *, small: bool = True) -> None:
    """Ecken eines Fensters per DWM runden – auch bei RAHMENLOSEN Fenstern
    (overrideredirect), die keine native Titelleiste/DWM-Kante mehr haben.

    small=True nimmt den kleinen Radius (~4 px, „leicht rund"), sonst den
    Standard (~8 px). DWM rendert die Rundung weich/antialiased und blendet den
    selbstgezeichneten Rand mit aus – im Gegensatz zu SetWindowRgn, das die Ecken
    hart abtreppt und den Rand in der Kurve abreissen laesst.

    Faellt auf Windows ohne dieses Attribut (< Win11 22000) leise auf eckig
    zurueck."""
    if not hwnd:
        return
    _dwm_set(hwnd, _DWMWA_WINDOW_CORNER_PREFERENCE,
             _DWMWCP_ROUNDSMALL if small else _DWMWCP_ROUND)


# ── Groesse nur an der Ecke unten-rechts ziehbar ────────
# Ein natives Fenster (WS_THICKFRAME) laesst sich an ALLEN Kanten/Ecken ziehen.
# Per WNDPROC-Subclassing fangen wir WM_NCHITTEST ab: DefWindowProc rechnet die
# Trefferzone aus, und jede Resize-Zone AUSSER der Ecke unten-rechts geben wir als
# toten Rand (HTBORDER) zurueck. Dadurch zieht nur noch die Ecke unten-rechts die
# Groesse; die Seiten/oberen Ecken sind inaktiv, das Bewegen an der Titelleiste
# (HTCAPTION) bleibt unberuehrt. Programmatisches root.geometry() ist nicht
# betroffen (NCHITTEST steuert nur das Ziehen mit der Maus).
_WM_NCHITTEST = 0x0084
_GWLP_WNDPROC = -4
_HTBORDER = 18
_HTBOTTOMRIGHT = 17
# Alle Rahmen-Trefferzonen, die DefWindowProc als "hier zieht die Groesse" liefert
# (HTLEFT..HTBOTTOMRIGHT, 10..17):
_HT_RESIZE = frozenset(range(10, 18))

_WNDPROC = ctypes.WINFUNCTYPE(ctypes.c_ssize_t, wintypes.HWND, wintypes.UINT,
                              wintypes.WPARAM, wintypes.LPARAM)
user32.CallWindowProcW.argtypes = [ctypes.c_void_p, wintypes.HWND, wintypes.UINT,
                                   wintypes.WPARAM, wintypes.LPARAM]
user32.CallWindowProcW.restype = ctypes.c_ssize_t
# 64-bit: SetWindowLongPtrW; 32-bit-Python kennt nur die Nicht-Ptr-Variante.
if hasattr(user32, "SetWindowLongPtrW"):
    _set_wndproc, _get_wndproc = user32.SetWindowLongPtrW, user32.GetWindowLongPtrW
else:
    _set_wndproc, _get_wndproc = user32.SetWindowLongW, user32.GetWindowLongW
_set_wndproc.argtypes = [wintypes.HWND, ctypes.c_int, _WNDPROC]
_set_wndproc.restype = ctypes.c_void_p
_get_wndproc.argtypes = [wintypes.HWND, ctypes.c_int]
_get_wndproc.restype = ctypes.c_void_p

# Subclass-Zustand je Fenster festhalten: die WNDPROC-Callbacks (und die alte Proc
# fuer die Aufrufkette) duerfen NICHT vom GC eingesammelt werden – sonst ruft
# Windows in freigegebenen Speicher (Absturz).
# hwnd -> die alte Fensterprozedur (WNDPROC), damit sie beim Abraeumen
# zurueckgesetzt werden kann. MUSS am Leben bleiben: gibt der GC den
# Callback frei, stuerzt Windows beim naechsten Fensterereignis ab.
_resize_hooks: dict[int, tuple[Any, Any]] = {}


def restrict_resize_to_corner(hwnd: int) -> None:
    """Fenstergroesse nur noch an der Ecke unten-rechts ziehbar machen.

    Haengt sich per Subclassing in WM_NCHITTEST und wandelt jede Resize-Zone ausser
    HTBOTTOMRIGHT in HTBORDER (toter Rand) – Seiten und obere Ecken ziehen also
    nicht mehr, nur die Ecke unten-rechts vergroessert/verkleinert. WS_THICKFRAME
    bleibt, damit diese Ecke nativ zieht. Idempotent (zweiter Aufruf fuers selbe
    Fenster tut nichts) und faellt bei Fehlern leise zurueck (Fenster bleibt dann
    normal ziehbar)."""
    try:
        hwnd = int(hwnd)
    except (TypeError, ValueError):
        return
    if not hwnd or hwnd in _resize_hooks:
        return

    def _proc(h: int, msg: int, wp: int, lp: int) -> int:
        _cb, old = _resize_hooks[hwnd]        # gehaltene Callback- + Alt-Proc-Refs
        res = user32.CallWindowProcW(old, h, msg, wp, lp)
        if msg == _WM_NCHITTEST and res in _HT_RESIZE and res != _HTBOTTOMRIGHT:
            return _HTBORDER
        return res

    try:
        cb = _WNDPROC(_proc)
        old = _get_wndproc(wintypes.HWND(hwnd), _GWLP_WNDPROC)
        if not old:
            return
        _resize_hooks[hwnd] = (cb, old)       # ERST halten, DANN einhaengen
        _set_wndproc(wintypes.HWND(hwnd), _GWLP_WNDPROC, cb)
    except OSError:
        _resize_hooks.pop(hwnd, None)


# ── Fenster in den Vordergrund holen ────────────────────
_SW_RESTORE = 9
user32.IsIconic.argtypes = [wintypes.HWND]
user32.IsIconic.restype = wintypes.BOOL


def focus_window(hwnd: int) -> None:
    """Holt hwnd zuverlaessig nach vorn (AttachThreadInput-Trick gegen die
    Windows-Sperre, die das Klauen des Vordergrunds normal verhindert).

    Restauriert NUR, wenn das Fenster minimiert ist - ein maximiertes/Vollbild-
    Fenster behaelt so seine Groesse (SW_RESTORE wuerde es sonst verkleinern)."""
    if user32.IsIconic(hwnd):
        user32.ShowWindow(hwnd, _SW_RESTORE)
    fg = user32.GetForegroundWindow()
    cur = kernel32.GetCurrentThreadId()
    fg_thr = user32.GetWindowThreadProcessId(fg, None)
    tgt_thr = user32.GetWindowThreadProcessId(hwnd, None)
    user32.AttachThreadInput(cur, fg_thr, True)
    user32.AttachThreadInput(cur, tgt_thr, True)
    try:
        user32.BringWindowToTop(hwnd)
        user32.SetForegroundWindow(hwnd)
    finally:
        user32.AttachThreadInput(cur, fg_thr, False)
        user32.AttachThreadInput(cur, tgt_thr, False)


def windows_of_pid(pid: int) -> list[int]:
    """Alle sichtbaren Top-Level-Fenster dieses Prozesses (Liste von hwnd).

    Reine Abfrage OHNE Nebenwirkung – wichtig fuer den Waechter (watchdog.py):
    der muss "lebt das Panel noch?" beantworten koennen, ohne dabei ein Fenster
    nach vorn zu holen (focus_pid tut genau das und wuerde dem Nutzer alle paar
    Minuten den Fokus klauen)."""
    hits = []

    def cb(hwnd: int, _: Any) -> bool:
        if user32.IsWindowVisible(hwnd):
            wpid = wintypes.DWORD()
            user32.GetWindowThreadProcessId(hwnd, ctypes.byref(wpid))
            if wpid.value == pid:
                hits.append(hwnd)
        return True

    user32.EnumWindows(_EnumProc(cb), 0)
    return hits


def has_window(pid: int) -> bool:
    """True, wenn der Prozess mindestens ein sichtbares Fenster hat (nebenwirkungsfrei)."""
    return bool(windows_of_pid(pid))


def focus_pid(pid: int) -> bool:
    """Erstes sichtbares Top-Level-Fenster des Prozesses <pid> nach vorn holen.
    True, wenn eins gefunden und fokussiert wurde, sonst False.

    Gedacht fuer den Single-Instance-Guard (single_instance.py): eine Zweit-
    Instanz holt so das bereits laufende Panel nach vorn, statt ein zweites
    (totes) Panel zu oeffnen. Anhand der PID statt des Titels, damit kein fremdes
    Fenster mit aehnlichem Titel getroffen wird."""
    hits = windows_of_pid(pid)
    if not hits:
        return False
    focus_window(hits[0])
    return True
