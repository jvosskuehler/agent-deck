"""Der Fokus-Rahmen: ein Ring genau auf der Aussenkante des Windows-Terminal-Fensters,
in dem gerade getippt wird. Innen ist er durchsichtig.

Gezeichnet wird ein fertiges BGRA-Bild fuer UpdateLayeredWindow (platform/layered):
deckend auf dem Ring, Alpha 0 innen. Wo Alpha 0 ist, klickt man durch, das Terminal
darunter bleibt also voll bedienbar. Vormultipliziert ist hier trivial, es gibt nur die
Deckungen 255 und 0.

Warum kein Shader im Terminal mehr: ein Pixel-Shader (experimental.pixelShaderPath)
malt nur in der Textflaeche. Windows Terminal legt Innenabstand und Scrollleiste aussen
herum, der Rahmen sass dadurch 8 Punkte (rechts 24) vom Fensterrand weg, und die
Tab-Leiste lag ganz ausserhalb. Verworfen 2026-11-06.
"""

COLOR = "#D97757"   # Claude-Orange, dieselbe Farbe wie der Tab im Profil "Agent Deck"
THICK_PT = 3        # Rahmenstaerke in Punkten


def thickness(factor: float) -> int:
    """Rahmenstaerke in Pixeln fuer einen Monitor mit Skalierung <factor>: so ist der
    Rahmen auf jedem Monitor gleich dick."""
    return max(1, round(THICK_PT * factor))


def _bgra(color: str) -> bytes:
    h = color.lstrip("#")
    r, g, b = int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16)
    return bytes((b, g, r, 255))


def ring_bgra(w: int, h: int, t: int, color: str = COLOR) -> bytes:
    """w*h*4 Bytes BGRA, von oben nach unten: aussen ein <t> Pixel breiter Ring in
    <color>, innen voll durchsichtig. Ist das Fenster schmaler als zwei Ringe, wird es
    ganz gefuellt.

    Zusammengesetzt aus ganzen Zeilen statt Pixel fuer Pixel: ein 4K-hohes Fenster hat
    gut eine Million Pixel, das waere in Python spuerbar."""
    if w <= 0 or h <= 0:
        return b""
    px = _bgra(color)
    full = px * w
    t = max(0, t)
    if 2 * t >= w or 2 * t >= h:
        return full * h
    row = px * t + bytes(4 * (w - 2 * t)) + px * t
    return full * t + row * (h - 2 * t) + full * t
