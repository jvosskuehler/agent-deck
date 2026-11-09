"""Restlaufzeit des Prompt-Caches: dieselbe Regel fuer Statuszeile und Kachel."""
import helpers  # noqa: F401 - legt die Repo-Wurzel auf den sys.path

from deck.domain import prompt_cache as pc


def test_laufzeit_wird_aus_api_schreibweise_gelesen():
    assert pc.ttl_seconds("1h") == 3600
    assert pc.ttl_seconds("5m") == 300
    assert pc.ttl_seconds(900) == 900
    assert pc.ttl_seconds("2h") is None
    assert pc.ttl_seconds(None) is None
    assert pc.ttl_seconds(True) is None          # bool ist in Python auch eine Zahl


def test_minuten_werden_abgerundet_nie_aufgerundet():
    assert pc.view(42 * 60 + 59, 3600) == ("42 min", pc.WARM)
    assert pc.view(59, 3600) == ("<1 min", pc.LOW)
    assert pc.view(0, 3600) == ("kalt", pc.COLD)
    assert pc.view(-5, 3600, "cold") == ("cold", pc.COLD)


def test_knapp_ab_fuenf_minuten_beim_kurzen_cache_erst_in_der_letzten():
    assert pc.view(5 * 60, 3600)[1] == pc.WARM
    assert pc.view(5 * 60 - 1, 3600)[1] == pc.LOW
    assert pc.view(3 * 60, 300)[1] == pc.WARM     # 5-Minuten-Cache: sonst fast immer knapp
    assert pc.view(59, 300)[1] == pc.LOW


# ── Automatisch kompaktieren ──────────────────────────────────────────────
JETZT = 1_000_000.0
GROSS = 200_000


def test_kompaktiert_wird_in_den_letzten_fuenf_minuten_des_stunden_caches():
    assert pc.compact_due("done", JETZT + 300, 3600, GROSS, JETZT)
    assert pc.compact_due("idle", JETZT + 60, 3600, GROSS, JETZT)
    assert not pc.compact_due("done", JETZT + 301, 3600, GROSS, JETZT)    # noch zu frueh
    assert not pc.compact_due("done", JETZT + 30, 3600, GROSS, JETZT)     # laese schon kalt
    assert not pc.compact_due("done", JETZT - 5, 3600, GROSS, JETZT)      # abgelaufen


def test_nicht_beim_fuenf_minuten_cache_und_nicht_ohne_ablaufzeit():
    assert not pc.compact_due("done", JETZT + 120, 300, GROSS, JETZT)
    assert not pc.compact_due("done", None, 3600, GROSS, JETZT)            # nach Compact: kein Timer
    assert not pc.compact_due("done", True, 3600, GROSS, JETZT)


def test_nur_wenn_der_agent_auf_dich_wartet():
    """Roher Hook-Status: 'denkt' kann ein langer Tool-Aufruf sein, 'waiting' eine
    offene Rueckfrage. In beiden Faellen landete /compact an der falschen Stelle."""
    for status in ("thinking", "running", "waiting", None, "unknown"):
        assert not pc.compact_due(status, JETZT + 120, 3600, GROSS, JETZT), status


def test_kleiner_kontext_lohnt_kein_compact():
    assert not pc.compact_due("done", JETZT + 120, 3600, pc.COMPACT_MIN_TOKENS - 1, JETZT)
    assert pc.compact_due("done", JETZT + 120, 3600, pc.COMPACT_MIN_TOKENS, JETZT)
    assert not pc.compact_due("done", JETZT + 120, 3600, None, JETZT)


def test_wer_gerade_an_diesem_fenster_tippt_wird_nicht_gestoert():
    assert pc.user_busy(True, 5)
    assert not pc.user_busy(True, pc.TYPING_GRACE_S)        # vorn, aber lange nichts getippt
    assert not pc.user_busy(False, 0)                       # du tippst woanders


def test_waehrend_der_agent_arbeitet_laeuft_kein_timer():
    assert pc.working({"status": "thinking", "ts": JETZT - 30}, JETZT)
    assert pc.working({"status": "running", "ts": JETZT - 600}, JETZT)   # langer Tool-Aufruf
    assert not pc.working({"status": "done", "ts": JETZT}, JETZT)
    assert not pc.working({"status": "waiting", "ts": JETZT}, JETZT)
    # Mit Esc abgebrochen: kein Stop-Hook, "denkt" bliebe stehen. Nach der Frist zaehlt es nicht mehr.
    assert not pc.working({"status": "thinking", "ts": JETZT - pc.WORKING_STALE_S - 1}, JETZT)
    assert not pc.working(None, JETZT) and not pc.working({}, JETZT)
