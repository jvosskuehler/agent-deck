"""watchdog: die Beurteilung des letzten Panel-Laufs - daran haengt die
Entscheidung, ob neu gestartet wird. Dazu die Heartbeat-Pruefung.
"""

import os
import time

import helpers  # noqa: F401 - Import MIT Absicht: legt die Repo-Wurzel auf den

                # sys.path und nagelt die Deck-Sprache auf Deutsch.


# Frage, die der Waechter falsch machen DARF und nicht falsch machen SOLL: darf er
# das Panel wieder hochholen? Ein bewusst geschlossenes Deck muss geschlossen
# bleiben, ein abgestuerztes muss zurueckkommen.
def _befund_fuer(log_text):
    """last_end() gegen ein praepariertes panel.log laufen lassen."""
    import tempfile

    from deck.ops import log
    from deck.ops import watchdog as wd
    fd, path = tempfile.mkstemp(prefix="panellog_", suffix=".log")
    os.close(fd)
    alt = log.LOG_PATH
    try:
        with open(path, "w", encoding="utf-8") as f:
            f.write(log_text)
        log.LOG_PATH = path
        return wd.last_end()
    finally:
        log.LOG_PATH = alt
        try:
            os.remove(path)
        except OSError:
            pass


def test_watchdog_sieht_sauberes_ende():
    """Panel hat sich selbst beendet -> der Nutzer hat es geschlossen. Der Waechter
    muss das als CLEAN_END erkennen, sonst kommt das Deck alle drei Minuten zurueck."""
    from deck.ops import watchdog as wd
    befund = _befund_fuer(
        "[..] --- Panel-Start (PID 1, Python 3.14.0) ---\n"
        "[..] mainloop beendet (Fenster zerstoert) -> Panel endet regulaer\n"
        "[..] --- Panel-Ende (normaler Exit) ---\n")
    assert befund.startswith(wd.CLEAN_END), befund


def test_watchdog_sieht_abschuss():
    """Log bricht mitten im Lauf ab: keine Exit-Marke, kein Dump -> von aussen
    abgeschossen. Muss neu gestartet werden (also NICHT als CLEAN_END gelten)."""
    from deck.ops import watchdog as wd
    befund = _befund_fuer(
        "[..] --- Panel-Start (PID 1, Python 3.14.0) ---\n"
        "[..] Fehler in Tk-Callback:\n(harmlos, Panel lief weiter)\n")
    assert "ABGESCHOSSEN" in befund
    assert not befund.startswith(wd.CLEAN_END)


def test_watchdog_sieht_harten_absturz():
    """faulthandler-Dump (Tcl-Panic) schlaegt alles andere – auch eine Exit-Marke,
    die zufaellig noch im selben Abschnitt steht."""
    befund = _befund_fuer(
        "[..] --- Panel-Start (PID 1, Python 3.14.0) ---\n"
        "Fatal Python error: Aborted\n\nCurrent thread 0x00001234 (most recent call first):\n")
    assert "HARTER ABSTURZ" in befund


def test_watchdog_beurteilt_nur_den_letzten_lauf():
    """Nur der Abschnitt nach der LETZTEN Panel-Start-Marke zaehlt: ein Absturz von
    vorgestern darf den heutigen sauberen Lauf nicht ueberstimmen."""
    from deck.ops import watchdog as wd
    befund = _befund_fuer(
        "[..] --- Panel-Start (PID 1, Python 3.14.0) ---\n"
        "Fatal Python error: Aborted\n"
        "[..] --- Panel-Start (PID 2, Python 3.14.0) ---\n"
        "[..] --- Panel-Ende (normaler Exit) ---\n")
    assert befund.startswith(wd.CLEAN_END), befund


def test_watchdog_waechter_zeilen_sind_kein_panel_lauf():
    """Der Waechter selbst schreibt KEINE Start-/Ende-Marken (log.install(marks=False)).
    Stuenden welche im Log, wuerde er seinen eigenen Lauf beurteilen und jedes
    geschlossene Deck wieder hochholen."""
    from deck.ops import watchdog as wd
    befund = _befund_fuer(
        "[..] --- Panel-Start (PID 1, Python 3.14.0) ---\n"
        "[..] --- Panel-Ende (normaler Exit) ---\n"
        "[..] Waechter: kein Panel da. Vorgaenger-Ende: irgendwas\n")
    assert befund.startswith(wd.CLEAN_END), befund


def test_heartbeat_frische_und_pid_muessen_passen():
    """beats_for(): nur ein frisches Lebenszeichen DIESER PID gilt. Sonst haelt der
    Guard ein fremdes/altes Signal fuer ein lebendes Panel (oder umgekehrt)."""
    import tempfile

    from deck.ops import instance as si
    d = tempfile.mkdtemp(prefix="beat_")
    alt = si.BEAT_PATH
    try:
        si.BEAT_PATH = os.path.join(d, "panel.heartbeat")
        assert si.beat_age() is None                  # noch gar keins
        assert si.beats_for(4711) is False
        si.beat()                                     # schreibt die EIGENE PID
        assert si.beat_pid() == os.getpid()
        assert si.beats_for(os.getpid()) is True
        assert si.beats_for(os.getpid() + 1) is False  # fremde PID -> nein
        age = si.beat_age()
        assert age is not None and age < si.BEAT_FRESH_S
        # Zu alt -> gilt nicht mehr (mtime zurueckdrehen statt zu warten).
        old = time.time() - si.BEAT_FRESH_S - 5
        os.utime(si.BEAT_PATH, (old, old))
        assert si.beats_for(os.getpid()) is False
    finally:
        si.BEAT_PATH = alt
        import shutil
        shutil.rmtree(d, ignore_errors=True)


def test_neustart_startet_den_einsprungpunkt_und_nicht_das_panel_modul():
    """Im laufenden Panel zeigt sys.argv[0] auf deck/ui/panel.py (runpy, alter_sys).
    Von dort aus startet kein Panel: ModuleNotFoundError 'deck', unter pythonw ohne
    Spur - so ist der Neustart-Knopf bis 2026-11-06 gestorben. Gestartet werden muss
    agent_deck.py, mit der Neustart-Markierung, und zuerst abgekoppelt vom Job."""
    import subprocess
    from deck.ops import watchdog as wd

    calls = []

    class _Fake:
        def __init__(self, argv, **kw):
            calls.append((argv, kw))

    echt = subprocess.Popen
    subprocess.Popen = _Fake
    try:
        assert wd.start_panel(env={"AGENT_DECK_RESTART": "1"}, args=["--x"])
    finally:
        subprocess.Popen = echt
    (argv, kw), = calls
    assert argv[1] == wd.PANEL and argv[1].endswith("agent_deck.py")
    assert argv[2:] == ["--x"]
    assert kw["env"] == {"AGENT_DECK_RESTART": "1"} and kw["cwd"] == wd.HERE
    assert kw["creationflags"] & wd.CREATE_BREAKAWAY_FROM_JOB
