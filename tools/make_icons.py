"""Zeichnet die Symbole des Decks (Entwurf „03 Graphit“, gewählt am 09.11.2026).

- resources/agent-taskbar.ico: Taskleisten-Symbol der Windows-Terminal-Agenten
  (dunkle Kachel, Prompt „›“, rosé Block-Cursor). Die App schreibt es beim Start in
  den App-Ordner und hängt es an jedes Agentenfenster (deck_win::taskbar).
- app/src-tauri/app-icon.png: Vorlage für das Symbol der App (dieselbe Kachel mit
  vier Feldern in den Statusfarben). Daraus macht `pnpm tauri icon src-tauri/app-icon.png`
  (in app/) alle Dateien unter app/src-tauri/icons.

Gezeichnet wird im Raster 64 (wie der Entwurf auf der Canvas), 16fach vergrößert und
dann verkleinert, damit die Kanten weich werden. Aufruf: python tools/make_icons.py
(braucht Pillow).
"""

from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
S = 16  # Vergrößerung gegenüber dem 64er-Raster
GRAPHITE, EDGE = "#242220", "#4A4643"
PAPER, ROSE = "#E6E0D6", "#E39B96"
STATUS = ["#E8B86A", "#8AB8D8", "#A8CC8C", "#E39B96"]  # Gold, Blau, Salbei, Rosé
ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256]


def s(v: float) -> int:
    return round(v * S)


def tile() -> tuple[Image.Image, ImageDraw.ImageDraw]:
    img = Image.new("RGBA", (64 * S, 64 * S), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle((s(4), s(4), s(60), s(60)), radius=s(14), fill=GRAPHITE, outline=EDGE, width=s(2))
    return img, d


def stroke(d: ImageDraw.ImageDraw, pts: list[tuple[float, float]], width: float, color: str) -> None:
    """Linienzug mit runden Enden und Ecken (Pillow kann nur runde Ecken)."""
    p = [(s(x), s(y)) for x, y in pts]
    d.line(p, fill=color, width=s(width), joint="curve")
    r = s(width) / 2
    for x, y in (p[0], p[-1]):
        d.ellipse((x - r, y - r, x + r, y + r), fill=color)


def agent() -> Image.Image:
    img, d = tile()
    stroke(d, [(17, 21), (28, 32), (17, 43)], 5.5, PAPER)
    d.rounded_rectangle((s(33), s(21), s(45), s(43)), radius=s(2), fill=ROSE)
    return img


def deck() -> Image.Image:
    img, d = tile()
    for (x, y), color in zip([(13, 13), (34, 13), (13, 34), (34, 34)], STATUS):
        d.rounded_rectangle((s(x), s(y), s(x + 17), s(y + 17)), radius=s(4), fill=color)
    return img


def main() -> None:
    ico = ROOT / "resources" / "agent-taskbar.ico"
    agent().resize((256, 256), Image.LANCZOS).save(ico, sizes=[(n, n) for n in ICO_SIZES])
    png = ROOT / "app" / "src-tauri" / "app-icon.png"
    deck().save(png)
    print(f"{ico}\n{png}")


if __name__ == "__main__":
    main()
