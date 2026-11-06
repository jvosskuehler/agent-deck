"""Wann der Fokus-Rahmen neu gezeichnet, verschoben oder versteckt wird."""
import helpers  # noqa: F401  - legt die Repo-Wurzel auf den sys.path

from deck.domain import focus_frame as ff

A = ((0, 0, 100, 200), 3)


def test_ohne_agent_vorn_wird_ein_stehender_rahmen_versteckt():
    assert ff.next_action(A, None) == ff.HIDE
    assert ff.next_action(None, None) == ff.KEEP


def test_neuer_rahmen_wird_gezeichnet():
    assert ff.next_action(None, A) == ff.SHOW


def test_reines_verschieben_zeichnet_nicht_neu():
    assert ff.next_action(A, ((50, 10, 150, 210), 3)) == ff.MOVE
    assert ff.next_action(A, A) == ff.KEEP


def test_andere_groesse_oder_staerke_zeichnet_neu():
    assert ff.next_action(A, ((0, 0, 101, 200), 3)) == ff.SHOW
    assert ff.next_action(A, ((0, 0, 100, 200), 4)) == ff.SHOW
