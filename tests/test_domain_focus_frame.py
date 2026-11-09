"""Welches Design der Fokus-Rahmen zeigt (domain/focus_frame.py)."""
import random

import helpers  # noqa: F401  - legt die Repo-Wurzel auf den sys.path

from deck.domain import focus_frame as ff


def test_neues_design_ist_nie_dasselbe_wie_eben():
    rnd = random.Random(4)
    last = None
    for _ in range(50):
        d = ff.pick(ff.DESIGNS, last, rnd)
        assert d in ff.DESIGNS and d != last
        last = d


def test_mit_nur_einem_design_bleibt_es_dabei():
    assert ff.pick(("glut",), "glut") == "glut"
