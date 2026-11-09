"""Single-Instance-Guard fuers Agent-Deck-Panel.

Verhindert den klassischen »zweites Panel = alles rot«-Fehler: ein zweiter Start
brachte bisher ein zweites Fenster hoch, dessen Broker NIE eine VS-Code-Verbindung
bekommt (der erste Binder auf Port 8765 behaelt alle) -> das zweite Panel zeigte
jedes Fenster als getrennt, waehrend die Fenster in Wahrheit am ersten Panel hingen.

Jetzt holt ein zweiter Start das bereits laufende Panel nach vorn und beendet sich
selbst, statt ein totes Zweit-Panel zu oeffnen.

Mechanik: eine Lock-Datei im State-Ordner haelt die PID des lebenden Panels.
  * Frischer Start findet ein LEBENDES Panel (PID laeuft UND hat ein Fenster) ->
    dessen Fenster nach vorn holen, False zurueckgeben -> Aufrufer beendet sich.
  * Eintrag tot (harter Absturz/kill) ODER PID inzwischen an einen Fremdprozess
    recycelt (kein Panel-Fenster) -> Lock gilt als veraltet und wird uebernommen.
  * Der eigene Neustart (agent_deck.restart()) meldet sich per RESTART_ENV explizit
    an: waehrend des Neustarts leben alt+neu kurz gleichzeitig – das ist KEIN
    Doppelstart, das Kind uebernimmt das Lock ohne Pruefung. Der Wert ist die PID des
    ALTEN Panels, und er gilt nur, wenn genau diese PID im Lock steht.

Warum die PID und nicht einfach "1" (gefunden 2026-11-06): eine Umgebungsvariable
erbt jeder Prozess, den das Kind startet. Das Panel startet wt.exe, und laeuft Windows
Terminal noch nicht, wird es dabei zum Kind des Panels und erbt AGENT_DECK_RESTART=1 -
und mit ihm jede Shell und jeder Claude-Agent darin, bis Windows Terminal endet. Ein
`start.bat` oder `python agent_deck.py` aus so einem Terminal sah dann aus wie ein
Neustart, uebersprang die Pruefung und brachte ein ZWEITES Panel hoch. Darum zwei
Sicherungen: das Kind nimmt die Variable sofort aus seiner eigenen Umgebung (seine
Kinder erben sie nicht mehr), und ein geerbter Rest passt zu keinem Lock.

Zuruecktreten heisst NICHT "nichts tun": der Zweitstart hinterlaesst zusaetzlich
einen Reveal-Wunsch (request_reveal), den das lebende Panel in seiner Poll-
Schleife abholt. Ohne den war ein zweiter Start am angedockten Deck faktisch
wirkungslos – dort ist nur ein 12 px schmaler Griff sichtbar, focus_pid() holte
also genau diesen Griff nach vorn und es sah aus wie »oeffnet sich nicht mehr«.

Windows-only (wie das ganze Deck): ctypes/Win32. Best effort – ein Lock-Fehler
darf den Start NIE verhindern (lieber ein moegliches Zweit-Panel als gar keins).
"""
import ctypes
import os
import time
from ctypes import wintypes

from deck.domain import paths as dp
from deck.platform import focus as wf

# Lock liegt neben dem state/-Ordner (…/claude-agent-deck/panel.lock).
LOCK_PATH = os.path.join(os.path.dirname(dp.STATE_DIR), "panel.lock")
# Marker daneben: "ein Zweitstart moechte, dass du dich zeigst" (siehe request_reveal).
REVEAL_PATH = os.path.join(os.path.dirname(dp.STATE_DIR), "panel.reveal")
# Aelteren Wunsch nicht mehr bedienen: sonst klappt das Deck wegen eines
# liegengebliebenen Markers (harter Absturz) irgendwann grundlos auf.
REVEAL_MAX_AGE_S = 30
# Lebenszeichen des laufenden Panels (mtime dieser Datei = "zuletzt lief die
# Poll-Schleife"). Der Inhalt ist die PID, rein informativ.
#
# Warum ueberhaupt: ein FENSTER ist als Lebensbeweis untauglich. Am Rand
# angedockt und eingeklappt hat das Panel unter Umstaenden kein einziges
# Fenster, das EnumWindows als sichtbar meldet (sichtbar ist dann nur der
# layered Griff) – wer daran "lebt es noch?" entscheidet, haelt ein gesundes
# Panel fuer tot und startet ein ZWEITES daneben. Zwei Panels sind der
# schlimmste Zustand ueberhaupt: nur eines bekommt Port 8765, das andere zeigt
# jedes Fenster als getrennt. Der Herzschlag entscheidet das eindeutig – und
# erkennt zusaetzlich ein HAENGENDES Panel (Prozess lebt, Schleife steht).
BEAT_PATH = os.path.join(os.path.dirname(dp.STATE_DIR), "panel.heartbeat")
BEAT_EVERY_S = 5.0     # so oft schlaegt das Panel (refresh ruft gedrosselt beat())
BEAT_FRESH_S = 30.0    # so alt darf der letzte Schlag sein, damit "lebt" gilt
# restart() setzt diese Umgebungsvariable im Kindprozess -> Guard ueberspringt die
# Doppelstart-Pruefung und uebernimmt das Lock direkt.
RESTART_ENV = "AGENT_DECK_RESTART"

_kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
_PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
_STILL_ACTIVE = 259


def _pid_alive(pid: int) -> bool:
    """True, wenn ein Prozess mit dieser PID laeuft. Best effort ueber OpenProcess;
    ein bereits beendeter (aber noch nicht abgeraeumter) Prozess meldet einen
    Exit-Code != STILL_ACTIVE und gilt als tot."""
    if not pid or pid <= 0:
        return False
    h = _kernel32.OpenProcess(_PROCESS_QUERY_LIMITED_INFORMATION, False, int(pid))
    if not h:
        return False   # PID existiert nicht (mehr)
    try:
        code = wintypes.DWORD()
        if _kernel32.GetExitCodeProcess(h, ctypes.byref(code)):
            return code.value == _STILL_ACTIVE
        return True    # Handle da, Status unklar -> vorsichtshalber als lebend werten
    finally:
        _kernel32.CloseHandle(h)


def _read_lock_pid() -> int:
    """PID aus der Lock-Datei (0, wenn keine/kaputt)."""
    try:
        with open(LOCK_PATH, encoding="utf-8") as f:
            return int(f.read().strip() or 0)
    except (OSError, ValueError):
        return 0


def _write_lock() -> None:
    """Eigene PID atomar ins Lock schreiben (Zielordner bei Bedarf anlegen)."""
    try:
        os.makedirs(os.path.dirname(LOCK_PATH), exist_ok=True)
        tmp = LOCK_PATH + ".tmp"
        with open(tmp, "w", encoding="utf-8") as f:
            f.write(str(os.getpid()))
        os.replace(tmp, LOCK_PATH)
    except OSError:
        pass   # Lock ist best effort -> ein Schreibfehler darf den Start nicht kippen


def beat() -> None:
    """Lebenszeichen setzen – das Panel ruft das aus seiner Poll-Schleife
    (gedrosselt auf BEAT_EVERY_S). Best effort, ein Schreibfehler darf nichts kippen."""
    try:
        os.makedirs(os.path.dirname(BEAT_PATH), exist_ok=True)
        with open(BEAT_PATH, "w", encoding="utf-8") as f:
            f.write(str(os.getpid()))
    except OSError:
        pass


def beat_age() -> float | None:
    """Alter des letzten Lebenszeichens in Sekunden; None, wenn es keins gibt."""
    try:
        return max(0.0, time.time() - os.path.getmtime(BEAT_PATH))
    except OSError:
        return None


def beat_pid() -> int:
    """PID aus dem Lebenszeichen (0, wenn keine/kaputt)."""
    try:
        with open(BEAT_PATH, encoding="utf-8") as f:
            return int(f.read().strip() or 0)
    except (OSError, ValueError):
        return 0


def beats_for(pid: int) -> bool:
    """True, wenn ein FRISCHES Lebenszeichen vorliegt, das zu dieser PID passt.

    Ein Lebenszeichen ohne PID-Angabe (0) wird akzeptiert: es kann von einer
    aelteren Panel-Version stammen, die noch keine geschrieben hat."""
    age = beat_age()
    if age is None or age > BEAT_FRESH_S:
        return False
    return beat_pid() in (0, pid)


def request_reveal() -> None:
    """Dem lebenden Panel hinterlassen: »zeig dich«.

    Gedacht fuer den zurueckgetretenen Zweitstart. Reiner Datei-Marker (wie die
    uebrigen Deck-Zustaende) – kein zusaetzlicher Port, keine Protokoll-Aenderung.
    Das laufende Panel holt ihn per take_reveal_request() in seiner Poll-Schleife
    ab und klappt auf. Best effort: ein Schreibfehler darf nichts kippen."""
    try:
        os.makedirs(os.path.dirname(REVEAL_PATH), exist_ok=True)
        with open(REVEAL_PATH, "w", encoding="utf-8") as f:
            f.write(str(os.getpid()))
    except OSError:
        pass


def clear_reveal_request() -> None:
    """Marker wegraeumen (auch wenn keiner da ist)."""
    try:
        os.remove(REVEAL_PATH)
    except OSError:
        pass


def take_reveal_request() -> bool:
    """True, wenn ein FRISCHER Reveal-Wunsch vorliegt – vom laufenden Panel gepollt.

    Der Marker wird in jedem Fall entfernt (auch ein veralteter), der Wunsch gilt
    also genau einmal und ein Wunsch aelter als REVEAL_MAX_AGE_S wird verworfen."""
    try:
        age = time.time() - os.path.getmtime(REVEAL_PATH)
    except OSError:
        return False
    clear_reveal_request()
    return age <= REVEAL_MAX_AGE_S


def acquire_or_focus() -> bool:
    """Entscheidet, ob diese Instanz laufen darf.

    Rueckgabe:
      True  -> laufen (Lock uebernommen).
      False -> es laeuft bereits ein Panel; dessen Fenster wurde nach vorn geholt
               UND es wurde gebeten, sich zu zeigen (request_reveal – angedockt
               ist Fokus allein unsichtbar). Der Aufrufer soll sich beenden
               (kein zweites, totes Panel oeffnen).

    Der Neustart-Fall (RESTART_ENV = PID des alten Panels, die im Lock steht) uebernimmt
    das Lock ohne Pruefung: waehrend des Neustarts leben alt+neu kurz gleichzeitig, das
    ist KEIN Doppelstart. Jeder andere Wert ist ein geerbter Rest und zaehlt nicht."""
    handoff = os.environ.pop(RESTART_ENV, None)   # nie an eigene Kinder weitergeben
    pid = _read_lock_pid()
    if handoff and pid and handoff.strip() == str(pid):
        _write_lock()
        return True
    if pid and pid != os.getpid() and _pid_alive(pid):
        # Lebende PID – aber lebt dahinter wirklich ein PANEL? Frueher entschied das
        # allein focus_pid(), also die Frage "hat der Prozess ein sichtbares Fenster".
        # Das ist zu streng: eingeklappt am Rand meldet EnumWindows unter Umstaenden
        # keins, und dann startete hier ein ZWEITES Panel neben dem gesunden (toter
        # Broker, alles zeigt "getrennt"). Ein frisches Lebenszeichen ist der
        # verlaessliche Beweis; die Fenstersuche bleibt als Rueckfall fuer eine alte
        # Panel-Version, die noch keinen Herzschlag schreibt.
        if beats_for(pid):
            wf.focus_pid(pid)   # best effort – eingeklappt gibt es nichts zu fokussieren
            request_reveal()    # … darum zusaetzlich: "zeig dich" (siehe unten)
            return False
        if wf.focus_pid(pid):
            # Fokus allein reicht nicht: am Rand angedockt ist nur der schmale
            # Griff sichtbar, fokussiert wird also ein 12-px-Balken. Das laufende
            # Panel soll aufklappen -> Wunsch hinterlassen.
            request_reveal()
            return False
    # Wir starten selbst -> ein liegengebliebener Wunsch (Absturz der Vorinstanz)
    # gehoert nicht uns und darf das frisch eingeklappte Deck nicht aufklappen.
    clear_reveal_request()
    _write_lock()
    return True
