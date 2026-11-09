"""Welches Design der Fokus-Rahmen beim naechsten Fokus zeigt (ui/focus_frame.py).

Die Namen sind die Tafeln 03 bis 06 des Canvas "Fokus-Rahmen Varianten" (2026-11-06);
gezeichnet werden sie vom Hilfsprogramm focus-frame/ (Designs.cs, Frame.hlsl).
"""
import random

DESIGNS = ("kometen", "glut", "hochspannung", "lauflicht")


def pick(designs: tuple[str, ...], last: str | None, rnd: random.Random | None = None) -> str:
    """Zufaelliges Design fuer den naechsten Fokus, nie zweimal dasselbe hintereinander
    (sonst wirkt der Wechsel wie keiner)."""
    pool = [d for d in designs if d != last] or list(designs)
    return (rnd or random).choice(pool)
