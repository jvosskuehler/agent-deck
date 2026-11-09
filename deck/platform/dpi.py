"""HiDPI: das Deck in ECHTEN Bildschirmpixeln zeichnen statt hochrechnen lassen.

Ohne diese Anmeldung behandelt Windows das Panel wie ein Programm aus der
96-dpi-Zeit: Tk zeichnet eine Kachel in 148x52 LOGISCHEN Pixeln, und das fertige
Fensterbild wird anschliessend auf die echte Aufloesung gestreckt (bei 150 % also
auf 222x78). Jeder gezeichnete Pixel wird zu anderthalb – darum sah frueher nicht
nur die runde Ecke treppig aus, sondern auch der Text weich: es war ein
Bitmap-Zoom, kein Rendering.

Nach enable() zeichnet Tk in Geraetepixeln. Damit die Oberflaeche dabei nicht auf
zwei Drittel schrumpft, muss alles mitwachsen – und dafuer gibt es hier bewusst
ZWEI Wege, je nach Welt:

  * WIDGETS (Dialoge, Menues, Eingabefelder): Deren Schriftgroessen stehen in
    PUNKTEN, und Tk rechnet Punkt->Pixel ueber `tk scaling`. Diesen Wert stellt Tk
    nach enable() von selbst richtig (bei 150 % auf 2.0) – die Dialoge wachsen
    also ohne eine einzige Codeaenderung mit. Nur beim Monitorwechsel zieht Tk
    nicht nach; das macht sync_tk_scaling().
  * CANVAS (das Deck selbst): Hier gilt das eigene Design-Einheiten-System mit
    explizitem Faktor. Damit die Schrift NICHT zusaetzlich ueber `tk scaling`
    wandert (sie waere sonst doppelt skaliert und liefe aus den Kacheln), werden
    Canvas-Schriften in PIXELN angegeben – fontpx(). Feste Masse ausserhalb des
    Kachel-Renderers (Leistenhoehe, Griffdicke, Polster) gehen durch px().

Merksatz: Punkte = Widgets (Tk skaliert), Pixel = Canvas (wir skalieren).

Drei Monitore, drei Skalierungen (hier: 150 / 100 / 125 %) sind der Normalfall,
darum Per-Monitor-V2 und nicht die System-DPI: der Faktor gilt pro Fenster und
wird beim Verschieben nachgezogen (Tk kennt kein WM_DPICHANGED, das Panel fragt
ihn in seiner Poll-Schleife ab).

Windows-only wie das ganze Deck; jeder Aufruf ist best effort und faellt still
auf Faktor 1.0 zurueck – lieber ein weich gezeichnetes Deck als gar keins.
"""
import ctypes
from typing import Any

# Per-Monitor-V2: das Fenster bekommt seine DPI vom Monitor, auf dem es liegt,
# und Windows skaliert nichts mehr fuer uns. Als Zeiger-Konstante uebergeben.
_PER_MONITOR_AWARE_V2 = ctypes.c_void_p(-4)
_PROCESS_PER_MONITOR_DPI_AWARE = 2

# Punkt -> Pixel bei 100 %: Tks Grundumrechnung (96 dpi / 72 pt). Bei 150 % setzt
# sync_tk_scaling das Anderthalbfache – und fontpx() rechnet damit Design-
# Punktgroessen in feste Canvas-Pixel um.
PT_PX = 96.0 / 72.0

_ui = 1.0            # aktueller Oberflaechenfaktor (1.0 = 100 %, 1.5 = 150 %)


def enable() -> bool:
    """Windows melden, dass wir selbst in Geraetepixeln zeichnen.

    MUSS vor dem ersten Tk-Aufruf stehen – danach steht die Awareness des
    Prozesses fest und laesst sich nicht mehr aendern. Drei Stufen, weil die
    modernste API erst ab Windows 10 1703 existiert; die letzte (SetProcessDPIAware)
    kennt jede Version ab Vista, ist aber nur system-weit statt pro Monitor.
    Rueckgabe: True, wenn irgendeine Stufe gegriffen hat."""
    try:
        if ctypes.windll.user32.SetProcessDpiAwarenessContext(_PER_MONITOR_AWARE_V2):
            return True
    except (AttributeError, OSError):
        pass
    try:
        ctypes.windll.shcore.SetProcessDpiAwareness(_PROCESS_PER_MONITOR_DPI_AWARE)
        return True
    except (AttributeError, OSError):
        pass
    try:
        return bool(ctypes.windll.user32.SetProcessDPIAware())
    except (AttributeError, OSError):
        return False


def system_factor() -> float:
    """Skalierung des Hauptmonitors (144 dpi -> 1.5). Fallback, solange es noch
    kein Fenster gibt."""
    try:
        dpi = ctypes.windll.user32.GetDpiForSystem()
        if dpi:
            return dpi / 96.0
    except (AttributeError, OSError):
        pass
    return 1.0


def factor_at(x: int, y: int) -> float:
    """Skalierung des Monitors unter dem Punkt (x, y). Fuer Fenster, die erst noch
    dorthin ziehen: ihr eigener Faktor ist dann der des alten Monitors."""
    try:
        from ctypes import wintypes

        from deck.platform.win32 import shcore, user32
        mon = user32.MonitorFromPoint(wintypes.POINT(int(x), int(y)), 2)   # NEAREST
        dx, dy = wintypes.UINT(), wintypes.UINT()
        if mon and shcore is not None and shcore.GetDpiForMonitor(
                mon, 0, ctypes.byref(dx), ctypes.byref(dy)) == 0 and dx.value:
            return dx.value / 96.0      # 0 = MDT_EFFECTIVE_DPI, die eingestellte Skalierung
    except (AttributeError, OSError):
        pass
    return system_factor()


def factor_for_window(hwnd: int) -> float:
    """Skalierung des Monitors, auf dem <hwnd> gerade liegt.

    Das ist der Wert, der beim Verschieben zwischen unterschiedlich skalierten
    Monitoren wechselt. Ohne Fenster (oder auf zu alten Systemen) faellt es auf
    die System-Skalierung zurueck."""
    if hwnd:
        try:
            dpi = ctypes.windll.user32.GetDpiForWindow(hwnd)
            if dpi:
                return dpi / 96.0
        except (AttributeError, OSError):
            pass
    return system_factor()


def set_ui(factor: float) -> float:
    """Den Oberflaechenfaktor global setzen (das Panel tut das beim Start und bei
    jedem Monitorwechsel). Sehr kleine/kaputte Werte werden abgefangen, damit ein
    Messfehler nicht die ganze Oberflaeche zusammenfaltet."""
    global _ui
    try:
        f = float(factor)
    except (TypeError, ValueError):
        return _ui
    _ui = f if 0.5 <= f <= 6.0 else 1.0
    return _ui


def ui() -> float:
    """Aktueller Oberflaechenfaktor."""
    return _ui


def px(v: float) -> int:
    """Design-Einheit (Mass bei 100 %) -> Geraetepixel, ganzzahlig gerundet.
    Fuer feste Masse ausserhalb des Kachel-Renderers (Leistenhoehe, Griffdicke …)."""
    return round(v * _ui)


def fpx(v: float) -> float:
    """Wie px(), aber ohne Rundung – fuer Koordinaten, die weiterrechnen
    (Mittelpunkte, Radien), damit sich Rundungsfehler nicht aufaddieren."""
    return v * _ui


def sync_tk_scaling(root: Any) -> None:
    """Tks Punkt->Pixel-Umrechnung auf den aktuellen Oberflaechenfaktor bringen.

    Beim Start macht Tk das nach enable() von allein (es liest die DPI des
    Hauptmonitors). Notwendig ist der Aufruf beim MONITORWECHSEL: Tk kennt kein
    WM_DPICHANGED, und ohne dieses Nachsetzen behielten Dialoge und Menues die
    Schriftgroesse des vorigen Schirms."""
    try:
        root.tk.call("tk", "scaling", PT_PX * _ui)
    except Exception:
        pass


def fontpx(pt: float, scale: float = 1.0, family: str = "Segoe UI",
           weight: str | None = None) -> tuple[str, int] | tuple[str, int, str]:
    """Canvas-Schrift in festen PIXELN – unabhaengig von `tk scaling`.

    <pt> ist die Design-Punktgroesse (die Zahl, die frueher im Code stand),
    <scale> der Faktor des Kachel-Renderers (Monitor x Zoom). Tk deutet eine
    NEGATIVE Groesse als Pixel; genau das brauchen wir, damit die Schrift exakt
    dem Kachelraster folgt und nicht zusaetzlich ueber `tk scaling` mitwandert.

    Bei 150 % und Zoom 1.0 wird aus 10 pt: 10 * 1.333 * 1.5 = 20 Pixel – also
    dieselbe optische Groesse wie vor der Umstellung, nur echt gezeichnet statt
    hochgerechnet."""
    size = max(1, round(pt * PT_PX * scale))
    return (family, -size, weight) if weight else (family, -size)
