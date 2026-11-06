"""Das Bild des Fokus-Rahmens (render/focus_ring.py)."""
import helpers  # noqa: F401  - legt die Repo-Wurzel auf den sys.path

from deck.render import focus_ring as fr

ORANGE = bytes((0x57, 0x77, 0xD9, 255))   # #D97757 als BGRA
CLEAR = bytes(4)


def _px(bits, w, x, y):
    i = (y * w + x) * 4
    return bits[i:i + 4]


def test_ring_ist_aussen_deckend_orange_und_innen_durchsichtig():
    w, h, t = 10, 8, 2
    bits = fr.ring_bgra(w, h, t)
    assert len(bits) == w * h * 4
    for x, y in ((0, 0), (1, 1), (9, 7), (8, 4), (5, 0), (5, 7), (0, 4)):
        assert _px(bits, w, x, y) == ORANGE, (x, y)
    for x, y in ((2, 2), (7, 5), (5, 4)):
        assert _px(bits, w, x, y) == CLEAR, (x, y)


def test_zu_kleines_fenster_wird_ganz_gefuellt():
    bits = fr.ring_bgra(3, 20, 2)
    assert bits == ORANGE * 60


def test_leeres_fenster_gibt_leeres_bild():
    assert fr.ring_bgra(0, 10, 3) == b""


def test_staerke_waechst_mit_der_skalierung_des_monitors():
    assert fr.thickness(1.0) == 3
    assert fr.thickness(1.5) in (4, 5)
    assert fr.thickness(2.0) == 6
    assert fr.thickness(0.1) == 1                 # nie unsichtbar
