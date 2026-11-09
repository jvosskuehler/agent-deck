"""single_instance: der Doppelstart-Guard (Lockfile, Heartbeat, Handoff).
"""

import os
import time

import helpers  # noqa: F401 - Import MIT Absicht: legt die Repo-Wurzel auf den

                # sys.path und nagelt die Deck-Sprache auf Deutsch.


def _patch_si(alive, focus_ret, restart_env=False):
    """single_instance fuer den Test isolieren: Lock UND Reveal-Marker in ein Temp-
    Verzeichnis legen, _pid_alive/focus_pid faken, RESTART_ENV setzen/loeschen. Gibt
    (si, restore, focus_calls) zurueck; restore() setzt am Ende alles zurueck."""
    import tempfile

    from deck.ops import instance as si
    focus_calls = []
    saved = {"LOCK_PATH": si.LOCK_PATH, "REVEAL_PATH": si.REVEAL_PATH,
             "_pid_alive": si._pid_alive,
             "focus_pid": si.wf.focus_pid, "env": os.environ.get(si.RESTART_ENV)}
    tmp = tempfile.mkdtemp()
    si.LOCK_PATH = os.path.join(tmp, "panel.lock")
    si.REVEAL_PATH = os.path.join(tmp, "panel.reveal")
    si._pid_alive = lambda pid: alive
    si.wf.focus_pid = lambda pid: (focus_calls.append(pid), focus_ret)[1]
    if restart_env:
        os.environ[si.RESTART_ENV] = "1" if restart_env is True else str(restart_env)
    else:
        os.environ.pop(si.RESTART_ENV, None)

    def restore():
        si.LOCK_PATH = saved["LOCK_PATH"]
        si.REVEAL_PATH = saved["REVEAL_PATH"]
        si._pid_alive = saved["_pid_alive"]
        si.wf.focus_pid = saved["focus_pid"]
        if saved["env"] is None:
            os.environ.pop(si.RESTART_ENV, None)
        else:
            os.environ[si.RESTART_ENV] = saved["env"]

    return si, restore, focus_calls


def test_si_lock_roundtrip():
    si, restore, _ = _patch_si(alive=False, focus_ret=False)
    try:
        assert si._read_lock_pid() == 0                 # keine Datei -> 0
        si._write_lock()
        assert si._read_lock_pid() == os.getpid()       # eigene PID gelesen
        with open(si.LOCK_PATH, "w") as f:
            f.write("kein-int")
        assert si._read_lock_pid() == 0                 # Muell -> 0, nie Exception
    finally:
        restore()


def test_si_pid_alive_real():
    """Echter ctypes-Pfad: der eigene Prozess lebt, unsinnige PIDs nicht."""
    from deck.ops import instance as si
    assert si._pid_alive(os.getpid()) is True
    assert si._pid_alive(0) is False
    assert si._pid_alive(-1) is False


def test_si_takes_over_dead_lock():
    """Totes Lock (PID lebt nicht) -> uebernehmen (True, eigene PID), NICHT fokussieren."""
    si, restore, focus_calls = _patch_si(alive=False, focus_ret=True)
    try:
        with open(si.LOCK_PATH, "w") as f:
            f.write("424242")
        assert si.acquire_or_focus() is True
        assert si._read_lock_pid() == os.getpid()
        assert focus_calls == []
    finally:
        restore()


def test_si_defers_to_live_panel():
    """Lebendes Lock mit Fenster -> fokussieren + False (Zweit-Instanz beendet sich),
    Lock bleibt unveraendert. Zusaetzlich MUSS ein Reveal-Wunsch hinterlassen werden:
    angedockt ist das Panel eingeklappt, Fokus allein bliebe unsichtbar."""
    si, restore, focus_calls = _patch_si(alive=True, focus_ret=True)
    try:
        with open(si.LOCK_PATH, "w") as f:
            f.write("424242")
        assert si.acquire_or_focus() is False
        assert focus_calls == [424242]
        assert si._read_lock_pid() == 424242
        assert si.take_reveal_request() is True     # Wunsch liegt vor
    finally:
        restore()


def test_si_reveal_request_once():
    """Wunsch gilt genau einmal: nach dem Abholen ist der Marker weg."""
    si, restore, _ = _patch_si(alive=False, focus_ret=False)
    try:
        assert si.take_reveal_request() is False    # nichts da
        si.request_reveal()
        assert si.take_reveal_request() is True
        assert si.take_reveal_request() is False    # verbraucht
        assert not os.path.exists(si.REVEAL_PATH)
    finally:
        restore()


def test_si_reveal_request_stale_ignored():
    """Liegengebliebener Wunsch (harter Absturz) wird verworfen UND weggeraeumt –
    sonst klappt das Deck irgendwann grundlos auf."""
    si, restore, _ = _patch_si(alive=False, focus_ret=False)
    try:
        si.request_reveal()
        old = time.time() - si.REVEAL_MAX_AGE_S - 5
        os.utime(si.REVEAL_PATH, (old, old))
        assert si.take_reveal_request() is False
        assert not os.path.exists(si.REVEAL_PATH)
    finally:
        restore()


def test_si_fresh_start_clears_stale_reveal():
    """Uebernimmt diese Instanz das Lock, gehoert ein liegengebliebener Wunsch nicht
    ihr: er wird geraeumt, damit das frisch eingeklappte Deck nicht aufklappt."""
    si, restore, _ = _patch_si(alive=False, focus_ret=True)
    try:
        si.request_reveal()
        assert si.acquire_or_focus() is True
        assert si.take_reveal_request() is False
    finally:
        restore()


def test_si_live_pid_no_window_reclaims():
    """PID lebt, aber kein Panel-Fenster (recycelte PID) -> Lock uebernehmen (True)."""
    si, restore, focus_calls = _patch_si(alive=True, focus_ret=False)
    try:
        with open(si.LOCK_PATH, "w") as f:
            f.write("424242")
        assert si.acquire_or_focus() is True
        assert focus_calls == [424242]                  # Versuch gemacht …
        assert si._read_lock_pid() == os.getpid()       # … aber Lock uebernommen
    finally:
        restore()


def test_si_restart_env_claims_without_check():
    """RESTART_ENV = PID des alten Panels im Lock -> Lock direkt uebernehmen,
    Doppelstart-Pruefung ueberspringen (auch bei lebendem Alt-Panel KEIN focus_pid)."""
    si, restore, focus_calls = _patch_si(alive=True, focus_ret=True, restart_env=424242)
    try:
        with open(si.LOCK_PATH, "w") as f:
            f.write("424242")
        assert si.acquire_or_focus() is True
        assert si._read_lock_pid() == os.getpid()
        assert focus_calls == []
    finally:
        restore()


def test_si_geerbter_neustart_hinweis_ist_kein_freibrief():
    """Am 2026-11-06 erbte Windows Terminal AGENT_DECK_RESTART=1 von einem frisch
    neugestarteten Panel, und jeder Start aus einem Agenten-Terminal uebersprang danach
    die Pruefung: zwei Panels. Ein Wert, der nicht zur PID im Lock passt, zaehlt nicht."""
    for stale in (True, 999):                       # "1" alter Machart, fremde PID
        si, restore, focus_calls = _patch_si(alive=True, focus_ret=True, restart_env=stale)
        try:
            with open(si.LOCK_PATH, "w") as f:
                f.write("424242")
            assert si.acquire_or_focus() is False   # lebendes Panel -> zuruecktreten
            assert focus_calls == [424242]
        finally:
            restore()


def test_si_neustart_hinweis_wird_nicht_an_kinder_vererbt():
    si, restore, _ = _patch_si(alive=False, focus_ret=False, restart_env=424242)
    try:
        with open(si.LOCK_PATH, "w") as f:
            f.write("424242")
        si.acquire_or_focus()
        assert si.RESTART_ENV not in os.environ
    finally:
        restore()
