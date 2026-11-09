"""Alle nach vorn: wer den Fokus bekommt und wann ein Druck zurueckfuehrt (domain/front.py)."""
import helpers  # noqa: F401  - legt die Repo-Wurzel auf den sys.path

from deck.domain import front


def test_die_einstellung_darf_text_oder_liste_sein():
    assert front.combos("ctrl+shift+<") == ["ctrl+shift+<"]
    assert front.combos(["ctrl+shift+<", " f20 ", "", "F20"]) == ["ctrl+shift+<", "f20"]
    assert front.combos("") == [] and front.combos(None) == [] and front.combos([]) == []


def test_den_fokus_bekommt_wer_am_laengsten_wartet():
    states = {"A1": {"status": "done", "ts": 5}, "A2": {"status": "waiting", "ts": 9},
              "A3": {"status": "waiting", "ts": 3}}
    assert front.focus_target(states, ["A1", "A2", "A3"], "A1") == "A3"


def test_anders_als_die_sprungtaste_geht_es_nicht_reihum():
    # Steht man schon beim Dringendsten, bleibt er es: wer alle holt, will dorthin.
    states = {"A1": {"status": "waiting", "ts": 1}, "A2": {"status": "waiting", "ts": 2}}
    assert front.focus_target(states, ["A1", "A2"], "A1") == "A1"


def test_wartet_keiner_bleibt_der_zuletzt_gewaehlte_sonst_der_erste():
    states = {"A1": {"status": "idle"}, "A2": {"status": "running"}}
    assert front.focus_target(states, ["A1", "A2"], "A2") == "A2"
    assert front.focus_target(states, ["A1", "A2"], "C7") == "A1"
    assert front.focus_target(states, [], None) is None


def test_zweiter_druck_beim_agenten_fuehrt_zurueck():
    t = front.BackToggle()
    assert t.press(500, False, lambda h: True) is None      # aus dem Editor: alle holen
    assert t.press(101, True, lambda h: True) == 500        # beim Agenten: zurueck
    # Danach ist der Merker verbraucht: wieder beim Agenten heisst neu holen.
    assert t.press(101, True, lambda h: True) is None


def test_ein_geschlossenes_ausgangsfenster_holt_die_agenten_neu():
    t = front.BackToggle()
    t.press(500, False, lambda h: True)
    assert t.press(101, True, lambda h: False) is None


def test_wer_inzwischen_woanders_ist_setzt_dort_den_neuen_ausgangspunkt():
    t = front.BackToggle()
    t.press(500, False, lambda h: True)
    assert t.press(600, False, lambda h: True) is None
    assert t.press(101, True, lambda h: True) == 600


def test_vom_deck_aus_gibt_es_keinen_rueckweg():
    # Das Deck meldet sein eigenes Fenster als 0: dorthin will niemand zurueck.
    t = front.BackToggle()
    t.press(0, False, lambda h: True)
    assert t.press(101, True, lambda h: True) is None
