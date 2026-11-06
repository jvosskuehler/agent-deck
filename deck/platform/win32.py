"""Die Win32-Grundlage: DLL-Handles und typisierte Signaturen.

Alle platform-Module bauen darauf auf. Die Deklarationen stehen bewusst an EINER
Stelle und nicht verstreut - so ist nachpruefbar, dass keine benutzte Funktion
untypisiert bleibt.
"""
import ctypes
from ctypes import wintypes
from typing import Any

user32 = ctypes.WinDLL("user32", use_last_error=True)
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
gdi32 = ctypes.WinDLL("gdi32", use_last_error=True)

# ── ctypes-Signaturen: Pflicht, nicht Kosmetik ───────────
# Ohne Deklaration nimmt ctypes fuer JEDEN Rueckgabewert `c_int` an und uebergibt
# Python-ints als C-`int` – beides 32 Bit. Handles (HWND/HDC/HGDIOBJ/HMONITOR) sind
# auf 64-Bit-Windows aber 64 Bit breit. Ein Handle wird so beim Holen abgeschnitten
# bzw. beim Weitergeben vorzeichenerweitert, und dann trifft die Operation ein
# ANDERES Objekt: ReleaseDC/DeleteDC geben im schlimmsten Fall einen fremden
# Geraetekontext frei. Das faellt nicht dort auf, wo es passiert – es korrumpiert
# den GDI-/Heap-Zustand des Prozesses, und der Absturz kommt spaeter an der
# aktivsten Allokationsstelle (hier: der Bildpfad des Glow-Timers, siehe
# glow_animator._paint_image_tile im Crash-Dump vom 2026-07-28).
#
# Darum werden ALLE benutzten Funktionen typisiert – auch die, bei denen es
# „bisher lief". Es lief, weil Handle-Werte meist klein sind; das ist Glueck, keine
# Zusage. Die Deklarationen stehen bewusst hier zusammen und nicht verstreut.
def _decl(lib: Any, name: str, restype: Any, *argtypes: Any) -> None:
    """argtypes/restype setzen, aber an einer fehlenden Funktion nicht scheitern:
    der Name wird per getattr aufgeloest (eine aeltere Windows-Version kennt sie
    dann eben nicht, und der jeweilige Aufrufer faellt selbst zurueck). Ein
    `_decl(user32.Fehlt, …)` waere dagegen ein AttributeError beim IMPORT – das
    Panel wuerde ueberhaupt nicht mehr starten."""
    fn = getattr(lib, name, None)
    if fn is None:
        return
    if restype is not None:
        fn.restype = restype
    if argtypes:
        fn.argtypes = list(argtypes)


_HDC = wintypes.HDC
_HWND = wintypes.HWND
_HMONITOR = wintypes.HANDLE

# Geraetekontexte (laufen im Frame-Takt des Griffs -> siehe layered_push)
_decl(user32, "GetDC", _HDC, _HWND)
_decl(user32, "ReleaseDC", ctypes.c_int, _HWND, _HDC)
_decl(gdi32, "DeleteDC", wintypes.BOOL, _HDC)
_decl(gdi32, "DeleteObject", wintypes.BOOL, wintypes.HGDIOBJ)
_decl(gdi32, "GetDeviceCaps", ctypes.c_int, _HDC, ctypes.c_int)
# Monitor-/Anzeigeabfragen
_decl(user32, "MonitorFromWindow", _HMONITOR, _HWND, wintypes.DWORD)
_decl(user32, "GetMonitorInfoW", wintypes.BOOL, _HMONITOR, ctypes.c_void_p)
_decl(user32, "EnumDisplaySettingsW", wintypes.BOOL, wintypes.LPCWSTR,
      wintypes.DWORD, ctypes.c_void_p)
# Fenster-Grundfunktionen
_decl(user32, "IsWindow", wintypes.BOOL, _HWND)
_decl(user32, "ShowWindow", wintypes.BOOL, _HWND, ctypes.c_int)
_decl(user32, "BringWindowToTop", wintypes.BOOL, _HWND)
_decl(user32, "SetForegroundWindow", wintypes.BOOL, _HWND)
# PostMessageW statt SendMessageW: der Aufruf kehrt sofort zurueck. SendMessage wartet,
# bis das FREMDE Fenster geantwortet hat - und haengt damit genau dann, wenn es haengt.
_decl(user32, "PostMessageW", wintypes.BOOL, _HWND, wintypes.UINT,
      ctypes.c_void_p, ctypes.c_void_p)
_decl(user32, "AttachThreadInput", wintypes.BOOL, wintypes.DWORD, wintypes.DWORD,
      wintypes.BOOL)
_decl(user32, "GetWindowThreadProcessId", wintypes.DWORD, _HWND,
      ctypes.POINTER(wintypes.DWORD))
_decl(user32, "GetLayeredWindowAttributes", wintypes.BOOL, _HWND,
      ctypes.POINTER(wintypes.COLORREF), ctypes.POINTER(ctypes.c_ubyte),
      ctypes.POINTER(wintypes.DWORD))
_decl(kernel32, "GetCurrentThreadId", wintypes.DWORD)
# Optional: auf sehr altem Windows fehlt dwmapi. Der Typ sagt das jetzt aus,
# statt dass jeder Aufrufer selbst auf None pruefen muss, ohne es zu wissen.
dwmapi: ctypes.WinDLL | None
try:
    dwmapi = ctypes.WinDLL("dwmapi")
    dwmapi.DwmSetWindowAttribute.argtypes = [wintypes.HWND, wintypes.DWORD,
                                             ctypes.c_void_p, wintypes.DWORD]
    dwmapi.DwmSetWindowAttribute.restype = ctypes.c_long   # HRESULT
except OSError:
    dwmapi = None   # sehr altes Windows -> Titelleiste bleibt Standard
