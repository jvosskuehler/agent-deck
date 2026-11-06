"""Waechter: startet das Panel neu, wenn es verschwunden ist – und protokolliert,
WIE es verschwunden ist.

Warum es das gibt: das Panel ist am 2026-07-28 zweimal weggewesen, auf zwei
verschiedene Arten (harter Tcl-Panic mit Event-Log-Eintrag; und ein voellig
stiller Tod ohne jede Spur im System). Der Panic-Weg ist behoben (Queue statt
root.after aus dem Thread, siehe AgentDeck._post). Fuer alles andere gilt: ein
Dashboard, das man an einer Bildschirmkante erwartet, muss einfach da sein – der
Nutzer soll nicht merken, dass ein Prozess gestorben ist.

Bewusst ZUSTANDSLOS: kein Dauerprozess, der selbst mitsterben koennte, sondern ein
kurzer Lauf, den die Windows-Aufgabenplanung im Takt aufruft (siehe install_watchdog.ps1).
Ein Lauf macht genau drei Dinge:

  1. Laeuft ein Panel? (Lock-PID lebt UND hat ein Fenster – ohne es nach vorn zu
     holen, sonst wuerde der Waechter dem Nutzer im Takt den Fokus klauen.)
  2. Wenn nicht: aus panel.log ablesen, wie der Vorgaenger geendet hat, und das
     als Befund ins Log schreiben. So sammelt sich die Diagnose von selbst an,
     ohne dass jemand im Moment des Absturzes dabei sein muss.
  3. Panel starten – abgekoppelt (DETACHED_PROCESS | CREATE_BREAKAWAY_FROM_JOB),
     damit es NICHT an der Prozessgruppe dessen haengt, der den Waechter aufruft.
     Ein Job-Object mit KILL_ON_JOB_CLOSE nimmt sonst alle Mitglieder lautlos mit
     (genau das Bild des stillen Todes) – `start ""` in einer .bat schuetzt davor
     NICHT.

Aufruf:  pythonw watchdog.py            ein Durchgang (fuer die Aufgabenplanung)
         python  watchdog.py --status   nur berichten, nichts starten
         pythonw watchdog.py --loop     als Dauerwaechter im eigenen Prozess

--loop ist der Weg OHNE Aufgabenplanung: ein eigener, sehr kleiner Prozess, der
nichts tut als schlafen, eine Datei lesen und im Notfall starten. Er ruehrt weder
Tk noch GDI an – genau die Bausteine, an denen das Panel stirbt – und ist damit
deutlich langlebiger als das, was er bewacht. Ein zweiter Loop kann nicht
entstehen (eigenes Lock, siehe _loop_lock).
"""
import ctypes
import os
import subprocess
import sys
import time
from ctypes import wintypes

from deck.domain import paths
from deck.ops import instance as si
from deck.ops import log

HERE = paths.REPO_ROOT
PANEL = os.path.join(HERE, "agent_deck.py")
# Eigenes Lock des Dauerwaechters (nicht das des Panels!) – verhindert, dass neben
# einem laufenden Loop ein zweiter entsteht (z.B. Autostart + Start von Hand).
LOOP_LOCK = os.path.join(os.path.dirname(si.LOCK_PATH), "watchdog.lock")
LOOP_EVERY_S = 60.0
# Befund, nach dem der Waechter absichtlich NICHTS tut (siehe main).
CLEAN_END = "sauberes Ende"
# So lange nach einem "kein Panel da" nochmal hinsehen: das Panel kann sich gerade
# selbst neu starten (restart() -> os._exit + Kind). Ohne diese Pause wuerde der
# Waechter in genau dieses Fenster hinein ein zweites Panel starten.
RECHECK_S = 6.0

# Win32-Flags fuer den abgekoppelten Start (subprocess kennt sie teils nicht namentlich).
DETACHED_PROCESS = 0x00000008
CREATE_BREAKAWAY_FROM_JOB = 0x01000000
CREATE_NEW_PROCESS_GROUP = 0x00000200


def _exe_name(pid: int) -> str:
    """Dateiname der EXE dieses Prozesses (klein), "" wenn nicht ermittelbar.
    Dient dem Recycling-Schutz: steckt hinter der Lock-PID inzwischen ein
    Fremdprozess, ist das Panel in Wahrheit weg."""
    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    h = k32.OpenProcess(0x1000, False, int(pid))     # QUERY_LIMITED_INFORMATION
    if not h:
        return ""
    try:
        buf = ctypes.create_unicode_buffer(1024)
        size = wintypes.DWORD(len(buf))
        if k32.QueryFullProcessImageNameW(h, 0, buf, ctypes.byref(size)):
            return os.path.basename(buf.value).lower()
        return ""
    finally:
        k32.CloseHandle(h)


def panel_state() -> tuple[str, int, str]:
    """Wie steht es um das Panel? -> (zustand, pid, hinweis)

    zustand ist einer von:
      "laeuft"  – frisches Lebenszeichen, alles gut.
      "weg"     – kein Panel-Prozess mehr da; der Waechter darf starten.
      "haengt"  – der Prozess lebt, sein Herzschlag ist aber alt (Poll-Schleife
                  steht). Hier wird BEWUSST NICHT gestartet: ein zweites Panel
                  neben einem lebenden Prozess ist schlimmer als ein haengendes
                  (nur eines bekommt Port 8765, das andere zeigt alles als
                  getrennt). Das wird nur vermerkt – Beenden ist Nutzersache.

    Das FENSTER taugt hier nicht als Kriterium: eingeklappt am Rand hat das Panel
    unter Umstaenden kein sichtbares Fenster (siehe single_instance.BEAT_PATH)."""
    pid = si._read_lock_pid()
    alive = bool(pid) and si._pid_alive(pid)
    if alive and si.beats_for(pid):
        return "laeuft", pid, ""
    if not alive:
        return "weg", 0, ""
    if "python" not in _exe_name(pid):
        return "weg", 0, f"Lock-PID {pid} wurde an einen Fremdprozess recycelt"
    age = si.beat_age()
    alt = f"{int(age)} s" if age is not None else "gar keins"
    return "haengt", pid, f"Prozess {pid} lebt, Lebenszeichen: {alt}"


def last_end() -> str:
    """Aus panel.log ablesen, wie der LETZTE PANEL-LAUF geendet hat.

    Betrachtet strikt den Abschnitt nach der letzten "--- Panel-Start"-Marke;
    alles davor ist ein frueherer Lauf. Die Reihenfolge der Pruefungen ist die
    Rangfolge der Befunde: eine Fehlerspur schlaegt die Exit-Marke (nach einer
    unbehandelten Exception laeuft atexit noch, die Marke steht dann also DA,
    obwohl der Lauf nicht sauber war).

    Rueckgabe: kurzer Klartext-Befund. Er trennt den Programmfehler vom
    "von aussen abgeschossen" – der wichtigste Unterschied bei der Suche."""
    try:
        with open(log.LOG_PATH, encoding="utf-8", errors="replace") as f:
            tail = f.read()[-8000:]
    except OSError:
        return "kein panel.log vorhanden (erster Lauf?)"
    if not tail.strip():
        return "panel.log ist leer"
    if "--- Panel-Start" not in tail:
        return "kein Panel-Lauf im Log (nur Waechter-Zeilen?)"
    seg = tail.split("--- Panel-Start")[-1]
    if "Fatal Python error" in seg or "Current thread 0x" in seg:
        return ("HARTER ABSTURZ – faulthandler-Dump steht im Log (Tcl-Panic o.ae.); "
                "der Stack darin zeigt den schuldigen Thread")
    if "UNBEHANDELTE EXCEPTION" in seg:
        return "unbehandelte Exception – Traceback steht im Log"
    if "Panel-Ende (normaler Exit)" in seg:
        return CLEAN_END + " (Prozess hat sich selbst beendet: geschlossen)"
    if "Panel-Ende (Neustart" in seg:
        # Selbst-Neustart (agent_deck.restart): das Kind sollte laufen. Laeuft es
        # nicht, ist der Neustart fehlgeschlagen -> starten ist genau richtig.
        return "Selbst-Neustart, aber kein Kind da -> Neustart ist fehlgeschlagen"
    return ("ABGESCHOSSEN – Log bricht mitten im Lauf ab: keine Exit-Marke, kein "
            "Crash-Dump. Das ist ein TerminateProcess von aussen (Job-Object, "
            "Aufraeum-Tool, Abmeldung), kein Programmfehler")


def start_panel(*, env: dict | None = None, args: tuple | list = ()) -> bool:
    """Panel abgekoppelt starten. True bei Erfolg. Gebraucht vom Waechter UND vom
    Neustart-Knopf (ui/panel.restart, der `env` mit RESTART_ENV mitgibt).

    Gestartet wird IMMER der Einsprungpunkt PANEL (agent_deck.py) - nie sys.argv[0].
    Im laufenden Panel zeigt sys.argv[0] auf deck/ui/panel.py, weil agent_deck.py das
    Modul per runpy.run_module(alter_sys=True) startet; von deck/ui aus findet Python
    das Paket `deck` nicht. Genau daran ist der Neustart-Knopf seit der Umstellung auf
    den Einsprungpunkt gestorben: ModuleNotFoundError noch vor log.install, unter
    pythonw ohne Konsole, also ohne jede Spur (gefunden 2026-11-06)."""
    exe = os.path.join(os.path.dirname(sys.executable), "pythonw.exe")
    if not os.path.isfile(exe):
        exe = sys.executable
    flags = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB
    for creationflags in (flags, DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP):
        try:
            subprocess.Popen([exe, PANEL, *args], cwd=HERE, creationflags=creationflags,
                             close_fds=True, env=env)
            return True
        except OSError:
            # Breakaway ist nicht immer erlaubt (Job ohne BREAKAWAY_OK -> Fehler 5):
            # dann ohne dieses Flag erneut versuchen, lieber angebunden als gar nicht.
            continue
    return False


def _loop_lock() -> bool:
    """True, wenn dieser Prozess der einzige Dauerwaechter ist (Lock uebernommen).

    Gleiche Mechanik wie beim Panel-Lock: eine PID in einer Datei, und ein Eintrag
    gilt nur, solange die PID lebt UND ein Python ist (PID-Recycling)."""
    pid = 0
    try:
        with open(LOOP_LOCK, encoding="utf-8") as f:
            pid = int(f.read().strip() or 0)
    except (OSError, ValueError):
        pid = 0
    if pid and pid != os.getpid() and si._pid_alive(pid) and "python" in _exe_name(pid):
        return False
    try:
        os.makedirs(os.path.dirname(LOOP_LOCK), exist_ok=True)
        with open(LOOP_LOCK, "w", encoding="utf-8") as f:
            f.write(str(os.getpid()))
    except OSError:
        pass          # Lock ist best effort – lieber wachen als gar nicht wachen
    return True


def run_loop(every: float = LOOP_EVERY_S) -> int:
    """Dauerwaechter: im Takt nachsehen und im Notfall starten. Laeuft, bis er
    beendet wird. Jeder Durchgang ist in try/except gekapselt – ein Fehler im
    Waechter darf niemals das Wachen beenden."""
    if not _loop_lock():
        print("Es laeuft schon ein Dauerwaechter -> dieser beendet sich.")
        return 0
    log.note(f"Waechter: Dauerbetrieb gestartet (PID {os.getpid()}, "
                  f"Takt {int(every)} s).")
    while True:
        try:
            once()
        except Exception:
            log.exc("Waechter-Durchgang")
        time.sleep(every)


def once() -> int:
    """Ein Durchgang: pruefen und ggf. starten. Rueckgabe wie main()."""
    zustand, _pid, hinweis = panel_state()
    if zustand == "laeuft":
        return 0
    if zustand == "haengt":
        log.note(f"Waechter: Panel haengt: {hinweis}. Es wird KEIN zweites gestartet.")
        return 1
    befund = last_end()
    if befund.startswith(CLEAN_END):
        return 0          # bewusst geschlossen -> in Ruhe lassen
    time.sleep(RECHECK_S)             # laeuft gerade ein Selbst-Neustart?
    if panel_state()[0] != "weg":
        return 0
    log.note(f"Waechter: kein Panel da. Vorgaenger-Ende: {befund}")
    ok = start_panel()
    log.note("Waechter: Panel neu gestartet." if ok
                  else "Waechter: Start FEHLGESCHLAGEN.")
    return 0 if ok else 1


def main() -> int:
    args = sys.argv[1:]
    if "--loop" in args:
        log.install(marks=False)
        every = LOOP_EVERY_S
        for a in args:                # optionaler Takt: --loop 120
            try:
                every = max(15.0, float(a))
            except ValueError:
                continue
        return run_loop(every)
    # marks=False: der Waechter ist NICHT das Panel und darf keine Start-/Ende-Marke
    # setzen – die liest er selbst aus (siehe log.install und last_end).
    log.install(marks=False)
    if "--status" not in args:
        return once()                # eine Wahrheit fuer den Durchgang: siehe once()
    # Nur berichten: derselbe Zustand, aber auf die Konsole statt ins Log.
    zustand, pid, hinweis = panel_state()
    if zustand == "laeuft":
        print(f"Panel laeuft (PID {pid}).")
        return 0
    if zustand == "haengt":
        print(f"Panel haengt: {hinweis}. Ein Waechter-Lauf wuerde KEIN zweites starten.")
        return 1
    print(f"Panel laeuft NICHT. Vorgaenger: {last_end()}"
          + (f" [{hinweis}]" if hinweis else ""))
    return 1


if __name__ == "__main__":
    sys.exit(main())
