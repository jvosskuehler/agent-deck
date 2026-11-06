"""Was der Fokus-Rahmen (ui/focus_frame.py) beim naechsten Blick tun muss.

Ein Rahmen ist (Rechteck, Staerke), das Rechteck als (links, oben, rechts, unten) in
Bildschirmpixeln. Neu gezeichnet wird nur, wenn sich Groesse oder Staerke aendern: das
Bild ist bei einem hohen Fenster einige Megabyte gross. Ein reines Verschieben, etwa
beim Ziehen des Fensters, setzt nur die Position.
"""

Frame = tuple[tuple[int, int, int, int], int]

KEEP, HIDE, MOVE, SHOW = "keep", "hide", "move", "show"


def _size(rect: tuple[int, int, int, int]) -> tuple[int, int]:
    return rect[2] - rect[0], rect[3] - rect[1]


def next_action(shown: Frame | None, want: Frame | None) -> str:
    """<shown> steht gerade (oder None), <want> soll stehen (oder None = kein Agent vorn)."""
    if want is None:
        return HIDE if shown is not None else KEEP
    if shown is None or shown[1] != want[1] or _size(shown[0]) != _size(want[0]):
        return SHOW
    return MOVE if shown[0] != want[0] else KEEP
