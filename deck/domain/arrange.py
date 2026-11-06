"""Fenster anordnen: welche gehören dazu, und wohin genau.

Die anzeigefreien Regeln hinter dem Knopf „VS-Code-Fenster einsammeln" (Einstellungen)
und hinter dem Kachelraster der Windows-Terminal-Agenten (tile_grid). Der Win32-Teil -
Fenster aufzählen, verschieben, maximieren - steht in `platform/arrange.py`; hier
stehen nur die Entscheidungen, die man auf Papier nachprüfen kann.

**Erkannt wird am PROZESS, nicht am Titel.** Der Titel wäre der naheliegende Weg (das
Deck sucht seine Fenster sonst per `VSCODE_MARKER`), aber er irrt in beide Richtungen:
ein Browser-Tab „VS-Code-Doku - Visual Studio Code - Firefox" würde MITGENOMMEN, und
ein Fenster mit eigenem `window.title` wäre unsichtbar. Beim Fokussieren einer Kachel
kostet ein Fehlgriff einen falschen Vordergrund; hier verschiebt er ein fremdes Fenster.
Der Prozessname ist die ehrliche Auskunft - `Code.exe` ist `Code.exe`, egal was in der
Leiste steht. Der Titel bleibt nur der Notnagel, wenn das Betriebssystem den Prozess
nicht herausgibt (siehe is_editor_window).
"""
import os

from deck.domain import config as cfg

# Prozessnamen, die als „VS-Code-Fenster" gelten. Klein geschrieben verglichen.
# Absichtlich knapp: Stable, Insiders und VSCodium sind dieselbe Anwendung in drei
# Auslieferungen. Andere Forks (Cursor, Windsurf) trägt ein, wer sie benutzt - wer
# hier zu breit filtert, verschiebt dem Nutzer Fenster, an die er nicht gedacht hat.
EDITOR_EXES = frozenset({
    "code.exe",                 # Visual Studio Code (Stable)
    "code - insiders.exe",      # Visual Studio Code Insiders
    "codium.exe",               # VSCodium
})

WS_MAXIMIZEBOX = 0x00010000     # Fensterstil-Bit: „hat einen Maximieren-Knopf"

# Untergrenze für das Wiederherstellungs-Rechteck. Ein Fenster, dessen gemerkte
# Normalgröße kaputt (0x0) oder winzig ist, würde sonst nach dem Entmaximieren als
# unauffindbarer Punkt auf dem Zielmonitor liegen - und genau dieses Rechteck ist es,
# das der Nutzer später mit einem Klick auf „Wiederherstellen" zu sehen bekommt.
MIN_W, MIN_H = 640, 420


def is_editor_window(exe: str, title: str = "") -> bool:
    """Gehört das Fenster zu VS Code?

    `exe` ist der volle Pfad der ausführenden Datei (leer, wenn Windows ihn nicht
    hergibt - dann fällt die Entscheidung auf den Titel zurück, wie im Modulkopf
    beschrieben). Ist der Prozess bekannt, entscheidet AUSSCHLIESSLICH er: ein
    `firefox.exe` mit „Visual Studio Code" im Titel bleibt liegen.
    """
    base = os.path.basename(exe or "").strip().lower()
    if base:
        return base in EDITOR_EXES
    return bool(title) and cfg.VSCODE_MARKER.lower() in title.lower()


def is_maximizable(style: int) -> bool:
    """Darf dieses Fenster überhaupt maximiert werden (WS_MAXIMIZEBOX)?

    Trennt die Hauptfenster von allem, was VS Code sonst noch offen hat - Dialoge
    („Speichern?"), Auswahl-Popups. Die haben das Bit nicht, und ein Maximieren wäre
    dort keine Aufräumhilfe, sondern ein Bildschirm voll grauer Fläche.
    """
    return bool(int(style) & WS_MAXIMIZEBOX)


def normal_rect(w: int, h: int, area: tuple[int, int, int, int]) -> tuple[int, int, int, int]:
    """Wiederherstellungs-Rechteck (x, y, w, h) für ein Fenster der Größe w x h,
    zentriert auf der Arbeitsfläche <area> = (links, oben, rechts, unten).

    Zentriert und nicht in die Ecke, weil dieses Rechteck der Zustand NACH einem
    späteren Entmaximieren ist: das Fenster soll dann mittig auf dem Zielmonitor
    liegen, nicht halb hinter der Taskleiste. Größer als die Arbeitsfläche wird es
    nie - eine vom grossen Monitor gemerkte Größe würde auf dem kleinen sonst
    überstehen.
    """
    left, top, right, bottom = area
    aw, ah = max(1, right - left), max(1, bottom - top)
    w = min(max(int(w), MIN_W), aw)
    h = min(max(int(h), MIN_H), ah)
    return (left + (aw - w) // 2, top + (ah - h) // 2, w, h)


# Höchstens so viele Agentenfenster stehen nebeneinander, auch auf sehr breiten
# Schirmen; ab dem nächsten beginnt eine zweite Reihe.
MAX_COLS = 4
# Schmaler wird keine Kachel, gerechnet in Pixeln bei 100 %: rund 60 Zeichen in der
# Standardschrift von Windows Terminal (Cascadia Mono 12 pt, etwa 10 px je Zeichen).
# Schmaler bricht Claude Code fast jede Zeile um; dann ist eine zweite Reihe besser.
# Beispiele: 4K bei 150 % -> 4 nebeneinander, Laptop 1920 px bei 125 % -> 2.
MIN_TILE_W = 600


def max_cols_for(width: int, factor: float = 1.0) -> int:
    """Wie viele Kacheln auf eine <width> Pixel breite Fläche nebeneinander passen,
    bei Skalierung <factor> (1.5 = 150 %): so viele, dass jede mindestens MIN_TILE_W
    breit bleibt, höchstens MAX_COLS, mindestens eine."""
    logical = int(int(width) / max(float(factor or 1.0), 0.1))
    return max(1, min(MAX_COLS, logical // MIN_TILE_W))


def tile_grid(n: int, area: tuple[int, int, int, int],
              max_cols: int = MAX_COLS) -> list[tuple[int, int, int, int]]:
    """<n> gleich große Kacheln (x, y, w, h) auf der Arbeitsfläche <area> =
    (links, oben, rechts, unten), zeilenweise von links oben.

    Erst so wenige Reihen wie möglich (bis <max_cols> nebeneinander), dann die Spalten
    gleichmäßig auf die Reihen verteilt: 5 Fenster sind 2 Reihen à 3, nicht 4 + 1.
    Bleibt dabei ein Platz frei, bleibt er leer - gleich groß geht vor lückenlos.
    Die Kanten werden über die ganze Fläche verteilt gerundet, damit keine Pixelreihe
    zwischen den Kacheln übrig bleibt.
    """
    n = int(n)
    if n <= 0:
        return []
    left, top, right, bottom = area
    rows = -(-n // max(1, int(max_cols)))
    cols = -(-n // rows)
    xs = [left + (right - left) * c // cols for c in range(cols + 1)]
    ys = [top + (bottom - top) * r // rows for r in range(rows + 1)]
    out = []
    for i in range(n):
        r, c = divmod(i, cols)
        out.append((xs[c], ys[r], xs[c + 1] - xs[c], ys[r + 1] - ys[r]))
    return out
