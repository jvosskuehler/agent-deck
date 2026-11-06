"""Sprung-Taste: wer als Naechstes dran ist (domain/attention.py)."""
import helpers  # noqa: F401  - legt die Repo-Wurzel auf den sys.path

from deck.domain import attention as at

STATES = {
    "A1": {"status": "done", "ts": 100},
    "A2": {"status": "waiting", "ts": 300},
    "B1": {"status": "waiting", "ts": 200},
    "B2": {"status": "thinking", "ts": 50},
    "B3": {"status": "idle", "ts": 10},
}
SHOWN = ["A1", "A2", "B1", "B2", "B3"]


def test_rueckfragen_kommen_vor_ungelesenen_antworten_aelteste_zuerst():
    assert at.queue(STATES, SHOWN) == ["B1", "A2", "A1"]


def test_der_erste_druck_fuehrt_zum_am_laengsten_wartenden():
    assert at.next_slot(STATES, SHOWN) == "B1"
    assert at.next_slot(STATES, SHOWN, current="B2") == "B1"   # vorn ist kein Wartender


def test_weitere_druecke_gehen_reihum():
    assert at.next_slot(STATES, SHOWN, current="B1") == "A2"
    assert at.next_slot(STATES, SHOWN, current="A2") == "A1"
    assert at.next_slot(STATES, SHOWN, current="A1") == "B1"


def test_nur_angezeigte_kacheln_zaehlen_und_ohne_wartende_passiert_nichts():
    assert at.next_slot(STATES, ["B2", "B3"]) is None
    assert at.next_slot(STATES, ["A1"]) == "A1"
    assert at.next_slot({"X1": {"status": "waiting", "ts": "kaputt"}}, ["X1"]) == "X1"
