"""Ein durchklickbares Fenster ueber einem fremden Fenster, fuer den Fokus-Rahmen
(ui/focus_frame.py).

Bewusst ein rohes Win32-Fenster und kein Tk-Toplevel: Tk holt ein Fenster beim
Einblenden nach vorn, und ein Rahmen, der dem Terminal den Fokus stiehlt, waere
schlimmer als keiner. Die Fensterprozedur ist DefWindowProcW selbst, es gibt also keinen
Python-Rueckruf, der am Leben gehalten werden muesste. Das Bild kommt per
UpdateLayeredWindow (platform/layered), ein WM_PAINT braucht es nicht.

Die Stil-Bits und warum:
  WS_EX_LAYERED      Bild mit Alpha je Pixel, innen durchsichtig
  WS_EX_TRANSPARENT  Maus geht immer durch, auch auf dem Ring selbst
  WS_EX_NOACTIVATE   wird nie aktiv, nimmt nie den Fokus
  WS_EX_TOOLWINDOW   nicht in Taskleiste und Alt+Tab
  WS_EX_TOPMOST      liegt ueber dem Terminal; gezeigt wird es nur, solange das
                     Terminal ohnehin das vorderste Fenster ist

Erstellt wird das Fenster auf dem Tk-Thread: dessen Nachrichtenschleife bedient es mit.
"""
import ctypes
from ctypes import wintypes

from deck.platform.layered import layered_enable, layered_push
from deck.platform.win32 import dwmapi, kernel32, user32

_CLASS = "AgentDeckFocusFrame"

_WS_POPUP = 0x80000000
_WS_EX_TOPMOST = 0x00000008
_WS_EX_TRANSPARENT = 0x00000020
_WS_EX_TOOLWINDOW = 0x00000080
_WS_EX_LAYERED = 0x00080000
_WS_EX_NOACTIVATE = 0x08000000
_EX_STYLE = (_WS_EX_LAYERED | _WS_EX_TRANSPARENT | _WS_EX_NOACTIVATE
             | _WS_EX_TOOLWINDOW | _WS_EX_TOPMOST)

_HWND_TOPMOST = wintypes.HWND(-1)
_SWP_NOSIZE = 0x0001
_SWP_NOMOVE = 0x0002
_SWP_NOZORDER = 0x0004
_SWP_NOACTIVATE = 0x0010
_SWP_SHOWWINDOW = 0x0040
_SWP_HIDEWINDOW = 0x0080
_ERROR_CLASS_ALREADY_EXISTS = 1410

_DWMWA_EXTENDED_FRAME_BOUNDS = 9
_DWMWA_WINDOW_CORNER_PREFERENCE = 33
_DWMWCP_DONOTROUND = 1

_WNDPROC = ctypes.WINFUNCTYPE(ctypes.c_ssize_t, wintypes.HWND, wintypes.UINT,
                              wintypes.WPARAM, wintypes.LPARAM)


class _WNDCLASSEXW(ctypes.Structure):
    _fields_ = [("cbSize", wintypes.UINT), ("style", wintypes.UINT),
                ("lpfnWndProc", _WNDPROC), ("cbClsExtra", ctypes.c_int),
                ("cbWndExtra", ctypes.c_int), ("hInstance", wintypes.HINSTANCE),
                ("hIcon", wintypes.HICON), ("hCursor", wintypes.HANDLE),
                ("hbrBackground", wintypes.HBRUSH), ("lpszMenuName", wintypes.LPCWSTR),
                ("lpszClassName", wintypes.LPCWSTR), ("hIconSm", wintypes.HICON)]


try:
    kernel32.GetModuleHandleW.argtypes = [wintypes.LPCWSTR]
    kernel32.GetModuleHandleW.restype = wintypes.HMODULE
    user32.RegisterClassExW.argtypes = [ctypes.POINTER(_WNDCLASSEXW)]
    user32.RegisterClassExW.restype = wintypes.ATOM
    user32.CreateWindowExW.argtypes = [
        wintypes.DWORD, wintypes.LPCWSTR, wintypes.LPCWSTR, wintypes.DWORD,
        ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int,
        wintypes.HWND, wintypes.HMENU, wintypes.HINSTANCE, wintypes.LPVOID]
    user32.CreateWindowExW.restype = wintypes.HWND
    user32.SetWindowPos.argtypes = [wintypes.HWND, wintypes.HWND, ctypes.c_int,
                                    ctypes.c_int, ctypes.c_int, ctypes.c_int,
                                    wintypes.UINT]
    user32.SetWindowPos.restype = wintypes.BOOL
    user32.IsIconic.argtypes = [wintypes.HWND]
    user32.IsIconic.restype = wintypes.BOOL
    if dwmapi is not None:
        dwmapi.DwmGetWindowAttribute.argtypes = [wintypes.HWND, wintypes.DWORD,
                                                 ctypes.c_void_p, wintypes.DWORD]
        dwmapi.DwmGetWindowAttribute.restype = ctypes.c_long
except (AttributeError, OSError):
    pass

_wndclass: _WNDCLASSEXW | None = None   # haelt Klassenname und Prozedur am Leben


def _register() -> bool:
    global _wndclass
    if _wndclass is not None:
        return True
    wc = _WNDCLASSEXW()
    wc.cbSize = ctypes.sizeof(_WNDCLASSEXW)
    wc.lpfnWndProc = _WNDPROC(ctypes.cast(user32.DefWindowProcW, ctypes.c_void_p).value)
    wc.hInstance = kernel32.GetModuleHandleW(None)
    wc.lpszClassName = _CLASS
    if not user32.RegisterClassExW(ctypes.byref(wc)):
        if ctypes.get_last_error() != _ERROR_CLASS_ALREADY_EXISTS:
            return False
    _wndclass = wc
    return True


def create() -> int:
    """Das (noch unsichtbare) Rahmenfenster anlegen. 0, wenn Windows nicht mitspielt."""
    if not _register():
        return 0
    hwnd = user32.CreateWindowExW(_EX_STYLE, _CLASS, "Agent Deck Fokus", _WS_POPUP,
                                  0, 0, 1, 1, None, None,
                                  kernel32.GetModuleHandleW(None), None)
    return int(hwnd or 0)


def show(hwnd: int, rect: tuple[int, int, int, int], bits: bytes) -> bool:
    """Rahmenfenster auf <rect> = (links, oben, rechts, unten) legen, <bits> hineinschieben
    und zeigen, ohne es zu aktivieren. Die Alpha-Schicht wird dabei neu gesetzt: nach
    einem Verstecken lehnt UpdateLayeredWindow sonst ab (siehe platform/layered)."""
    x, y, w, h = rect[0], rect[1], rect[2] - rect[0], rect[3] - rect[1]
    if not hwnd or w <= 0 or h <= 0:
        return False
    user32.SetWindowPos(hwnd, _HWND_TOPMOST, x, y, w, h, _SWP_NOACTIVATE)
    layered_enable(hwnd, force=True)
    if not layered_push(hwnd, bits, w, h):
        return False
    return bool(user32.SetWindowPos(hwnd, _HWND_TOPMOST, 0, 0, 0, 0,
                                    _SWP_NOMOVE | _SWP_NOSIZE | _SWP_NOACTIVATE
                                    | _SWP_SHOWWINDOW))


def move(hwnd: int, x: int, y: int) -> bool:
    """Nur verschieben, Bild und Groesse bleiben. Haelt das Fenster dabei obenauf."""
    return bool(hwnd) and bool(user32.SetWindowPos(
        hwnd, _HWND_TOPMOST, x, y, 0, 0, _SWP_NOSIZE | _SWP_NOACTIVATE))


def hide(hwnd: int) -> None:
    if hwnd:
        user32.SetWindowPos(hwnd, None, 0, 0, 0, 0,
                            _SWP_NOMOVE | _SWP_NOSIZE | _SWP_NOZORDER | _SWP_NOACTIVATE
                            | _SWP_HIDEWINDOW)


def minimized(hwnd: int) -> bool:
    return bool(user32.IsIconic(hwnd))


def visible_rect(hwnd: int) -> tuple[int, int, int, int] | None:
    """Der SICHTBARE Rand von <hwnd> (links, oben, rechts, unten), ohne den unsichtbaren
    Greifrand, den Windows 10/11 um jedes Fenster legt. Ohne DWM-Auskunft None: dann
    wuerde der Rahmen um ein paar Pixel neben dem Fenster schweben."""
    if not hwnd or dwmapi is None:
        return None
    r = wintypes.RECT()
    if dwmapi.DwmGetWindowAttribute(wintypes.HWND(hwnd), _DWMWA_EXTENDED_FRAME_BOUNDS,
                                    ctypes.byref(r), ctypes.sizeof(r)) != 0:
        return None
    return (r.left, r.top, r.right, r.bottom)


def square_corners(hwnd: int) -> None:
    """Die runden Win11-Ecken eines (auch fremden) Fensters abschalten. Sonst schaut an
    jeder Ecke zwischen eckigem Rahmen und runder Fensterecke der Hintergrund durch."""
    if not hwnd or dwmapi is None:
        return
    val = ctypes.c_int(_DWMWCP_DONOTROUND)
    try:
        dwmapi.DwmSetWindowAttribute(wintypes.HWND(hwnd), _DWMWA_WINDOW_CORNER_PREFERENCE,
                                     ctypes.byref(val), ctypes.sizeof(val))
    except Exception:
        pass
