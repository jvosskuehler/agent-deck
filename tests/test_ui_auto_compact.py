"""Auto-Compact im Deck: wer wann /compact bekommt. Mit Attrappen, ohne Tk und Fenster."""
import helpers  # noqa: F401 - legt die Repo-Wurzel auf den sys.path

from deck.ui import auto_compact as ac

JETZT = 1_000_000.0


class _Hosts:
    def __init__(self, connected=True, remote=None, wt=True):
        self._c, self._r, self._wt = connected, remote, wt

    def connected(self, win):
        return self._c

    def remote(self, win):
        return self._r

    def is_wt(self, win):
        return self._wt


def _bau(settings=None, hosts=None, front=False, idle=600.0, ok=True):
    gesendet = []

    def send(slot, text):
        gesendet.append((slot, text))
        return ok

    c = ac.AutoCompactor({} if settings is None else settings, hosts or _Hosts(), send,
                         front=lambda slot: front, input_idle=lambda: idle)
    return c, gesendet


def _live(rest=120, ttl=3600, tokens=200_000, alter=5):
    return {"cache_until": JETZT + rest, "cache_ttl": ttl, "ctx_tokens": tokens,
            "ts": JETZT - alter}


def test_wartender_agent_bekommt_einmal_compact():
    c, gesendet = _bau()
    states, live = {"A1": {"status": "done"}}, {"A1": _live()}
    assert c.step(["A1"], states, live, JETZT) == ["A1"]
    assert gesendet == [("A1", "/compact")]
    # Derselbe Cache-Stand: kein zweites Mal, auch wenn das Compact nicht durchging.
    assert c.step(["A1"], states, live, JETZT + 10) == []
    # Neuer Stand (du hast geschrieben, der Timer lief neu und wieder fast ab): wieder.
    live2 = {"A1": dict(_live(), cache_until=JETZT + 3600 + 120, ts=JETZT + 3600 - 5)}
    assert c.step(["A1"], states, live2, JETZT + 3600) == ["A1"]


def test_schalter_aus_heisst_nichts_senden():
    c, gesendet = _bau(settings={ac.SETTING: False})
    assert c.step(["A1"], {"A1": {"status": "done"}}, {"A1": _live()}, JETZT) == []
    assert gesendet == []


def test_nicht_wenn_du_gerade_in_diesem_fenster_tippst():
    c, gesendet = _bau(front=True, idle=10)
    assert c.step(["A1"], {"A1": {"status": "done"}}, {"A1": _live()}, JETZT) == []
    # Fenster vorn, aber seit Minuten keine Eingabe: du bist weg.
    c, gesendet = _bau(front=True, idle=600)
    assert c.step(["A1"], {"A1": {"status": "done"}}, {"A1": _live()}, JETZT) == ["A1"]


def test_nicht_ohne_laufende_statuszeile_und_nicht_bei_getrenntem_fenster():
    """Alte live.json: Claude ist zu, /compact landete in der Shell."""
    c, _ = _bau()
    assert c.step(["A1"], {"A1": {"status": "done"}}, {"A1": _live(alter=600)}, JETZT) == []
    c, _ = _bau(hosts=_Hosts(connected=False))
    assert c.step(["A1"], {"A1": {"status": "done"}}, {"A1": _live()}, JETZT) == []
    c, _ = _bau(hosts=_Hosts(remote="ssh-remote"))
    assert c.step(["A1"], {"A1": {"status": "done"}}, {"A1": _live()}, JETZT) == []


def test_fehlgeschlagenes_senden_wird_im_naechsten_takt_wiederholt():
    c, gesendet = _bau(ok=False)
    states, live = {"A1": {"status": "done"}}, {"A1": _live()}
    assert c.step(["A1"], states, live, JETZT) == []
    assert c.step(["A1"], states, live, JETZT + 1) == []
    assert len(gesendet) == 2


def test_ein_fehler_haelt_die_poll_schleife_nicht_an():
    def kaputt(slot):
        raise RuntimeError("Win32 weg")

    c = ac.AutoCompactor({}, _Hosts(), lambda s, t: True, front=kaputt, input_idle=lambda: 0)
    assert c.step(["A1", "B1"], {"A1": {"status": "done"}, "B1": {"status": "done"}},
                  {"A1": _live(), "B1": _live()}, JETZT) == []

