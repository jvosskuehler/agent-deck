"""Der Neustart-Plan: was nach dem Beenden von VS Code wieder aufgebaut wird.

Der Knopf „VS Code neu starten" schließt alle Fenster und öffnet sie wieder - und
danach soll jede Kachel ihren Chat zurückhaben, nicht bloß ein leeres Terminal. Möglich
ist das, weil `claude/hooks/report.py` je Slot `session_id` UND `cwd` mitschreibt und
`claude --resume <id>` genau diese Unterhaltung wieder aufnimmt (ohne `--fork-session`
behält sie ihre ID - der nächste Hook meldet also weiter auf dieselbe Kachel).

Hier steht die ganze Rechnung dazu und nichts sonst: aus dem, was der Broker gerade
weiß, und dem, was in den Zustandsdateien steht, wird eine Liste von Fenstern mit
Ordnerpfad und darin eine Liste von Slots mit fertiger Kommandozeile. Kein Fenster wird
geschlossen, kein Prozess gestartet - das tut `ops/vscode_restart` mit diesem Plan in
der Hand.

Der Plan wird in eine Datei geschrieben, BEVOR das erste Fenster zugeht. Ein Neustart
ist der eine Moment, in dem das Deck seine Kacheln absichtlich verliert; stirbt das
Panel mitten drin, wäre ohne die Datei jede Zuordnung von Slot zu Session weg. Mit ihr
setzt derselbe Knopf den Lauf fort (siehe `needs_relaunch`).
"""
import os
import re
import time
from collections.abc import Sequence
from typing import Any

from deck.domain import ordering
from deck.domain.binding import repo_from_title, strip_remote_tag, window_from_title
from deck.domain.paths import REPO_ROOT, load_json, save_json

# Neben dem Code, wie alle Laufzeit-JSONs (siehe paths.REPO_ROOT - NICHT __file__).
PLAN_FILE = os.path.join(REPO_ROOT, "restart_plan.json")

# Eine Session-ID ist eine UUID. Die Prüfung ist nicht Kosmetik: der Wert landet
# ungeschützt in einer Shell-Zeile (`claude --resume "<id>"`), und was hier durchkommt,
# muss darin harmlos sein. Dasselbe gilt für die anderen Flag-Werte unten - alle
# stammen aus unseren eigenen Dateien, aber eine Datei ist kein Beweis.
_UUID = re.compile(r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}"
                   r"-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$")

# Modellnamen wie "opus[1m]" - Buchstaben, Ziffern, Klammern, Punkt, Strich, Unterstrich.
_MODEL = re.compile(r"^[A-Za-z0-9._\-\[\]]+$")

# Gültige `--effort`-Level (verifiziert an Claude Code, siehe claude/settings.py).
# "ultracode" ist KEINS davon, sondern xhigh plus ein separates Flag, das die CLI nicht
# kennt -> es fällt auf seinen Level zurück. Der Merker in slot_effort.json bleibt
# davon unberührt, die Karte zeigt also weiter "ultracode", und ein `/effort ultracode`
# im Chat holt den Rest nach.
_EFFORTS = {"low", "medium", "high", "xhigh", "max"}
_EFFORT_ALIAS = {"ultracode": "xhigh"}

# Deck-Name des Permission-Modus (so schreibt ihn report.py) -> Wert für
# `--permission-mode`. Die Umkehrung von hooks/report._MODE_MAP, aber bewusst NUR für
# die vier Modi, die das Deck selbst setzt (claude/settings.MODE_CHOICES). "bypass" und
# "dontask" fehlen mit Absicht: deren CLI-Schreibweise ist hier nicht belegt, und
# Berechtigungen per Automatik AUFZUWEITEN wäre der falsche Fehler. Wer in einem dieser
# Modi lief, startet im Modus der wiederaufgenommenen Session.
_MODES = {"manual": "default", "accept": "acceptEdits",
          "plan": "plan", "auto": "auto"}


def valid_session(sid: Any) -> bool:
    """Ist das eine brauchbare Session-ID (UUID)?"""
    return isinstance(sid, str) and bool(_UUID.match(sid.strip()))


def flags(session_id: Any = None, model: Any = None, effort: Any = None,
          mode: Any = None) -> str:
    """Die Flag-Zeile für `claude` - fertig zitiert, in fester Reihenfolge.

    Alles, was die Prüfungen oben nicht besteht, fällt still weg: ein Agent, der ohne
    sein Modell startet, ist ärgerlich - eine Zeichenkette, die eine Shell-Zeile
    aufbricht, ist ein Loch. Leere Rückgabe = `claude` blank starten.
    """
    parts = []
    if valid_session(session_id):
        parts.append(f'--resume "{str(session_id).strip()}"')
    if isinstance(model, str) and _MODEL.match(model.strip()):
        # Anführungszeichen halten das '[1m]'-Suffix in PowerShell/bash literal
        # (dieselbe Begründung wie beim Start eines neuen Agenten in extension.js).
        parts.append(f'--model "{model.strip()}"')
    if isinstance(effort, str):
        lvl = effort.strip().lower()
        lvl = _EFFORT_ALIAS.get(lvl, lvl)
        if lvl in _EFFORTS:
            parts.append(f"--effort {lvl}")
    if isinstance(mode, str):
        cli = _MODES.get(mode.strip().lower())
        if cli:
            parts.append(f"--permission-mode {cli}")
    return " ".join(parts)


def slot_entry(slot: str, state: dict[str, Any] | None, *, model: str | None = None,
               effort: str | None = None, can_resume: Any = None) -> dict[str, Any]:
    """Ein Slot im Plan: Name, Arbeitsverzeichnis, fertige Flag-Zeile.

    `cwd` ist nicht bequem, sondern nötig: Claude findet eine Session über das
    Projektverzeichnis (~/.claude/projects/<kodiertes cwd>/<id>.jsonl), und bei einem
    Ticket-Agenten ist das der git worktree, nicht die Repo-Wurzel.

    `fresh` heißt „keine Session zum Aufnehmen" - der Slot kommt dann als leerer,
    frischer Agent zurück. Das ist der normale Fall für eine Kachel, die noch nie
    gefragt wurde (ohne Hook-Meldung gibt es keine ID).

    `can_resume(session_id, cwd) -> bool` prüft optional, ob es zu der ID überhaupt
    noch ein Transcript gibt. Eine ID allein ist nämlich kein Beweis: die
    Zustandsdateien überleben Aufräumarbeiten in ~/.claude/projects, und ein
    `--resume` auf eine verschwundene Unterhaltung startet Claude gar nicht - die
    Kachel käme mit einer Fehlermeldung statt mit einem Chat zurück. Das Prädikat wird
    von außen gegeben, weil im Rechenkern kein Dateisystem wohnt.
    """
    st = state or {}
    sid = st.get("session_id")
    cwd = st.get("cwd") or ""
    if valid_session(sid) and callable(can_resume) and not can_resume(sid, cwd):
        sid = None
    eff = effort or st.get("effort")
    return {
        "slot": slot,
        "cwd": cwd,
        "args": flags(sid, model, eff, st.get("mode")),
        "fresh": not valid_session(sid),
    }


def build(clients: Sequence[dict[str, Any]], states: dict[str, Any],
          order: dict[str, list[str]] | None = None, *,
          model: str | None = None,
          slot_effort: dict[str, str] | None = None,
          can_resume: Any = None) -> dict[str, Any]:
    """Aus dem Ist-Zustand einen Plan machen.

    `clients` ist, was der Broker über die verbundenen Fenster weiß (Buchstabe,
    Workspace-Name, Ordnerpfad, gemeldete Slots), `states` sind die Slot-Zustände,
    `order` die vom Nutzer gezogene Kachel-Reihenfolge. Die Reihenfolge zählt: die
    Extension vergibt Slot-Namen fortlaufend, also müssen die Chats in genau der
    Reihenfolge wieder entstehen, in der sie gezählt wurden.

    Fenster OHNE Ordnerpfad landen in `skipped` statt im Plan - ein leeres VS-Code-
    Fenster kann man nicht wieder aufmachen, es gibt nichts zu öffnen. Der Rest wird
    trotzdem neu gestartet; was übrig blieb, steht im Plan und kann gemeldet werden.
    """
    order = order or {}
    effs = slot_effort or {}
    wins, skipped = [], []
    for cl in clients:
        win = (cl.get("window") or "").strip()
        folder = (cl.get("folder") or "").strip()
        if not win:
            continue                    # verbunden, aber ohne Buchstaben -> keine Kachel
        if not folder:
            skipped.append(win)
            continue
        slots = ordering.apply_order(order.get(win, []), cl.get("slots") or [])
        wins.append({
            "window": win,
            "workspace": cl.get("workspace") or "",
            "folder": folder,
            "slots": [slot_entry(s, states.get(s), model=model, effort=effs.get(s),
                                 can_resume=can_resume) for s in slots],
        })
    return {"ts": time.time(), "windows": wins, "skipped": skipped}


# Ein Ordner ist ein Pfad ("C:\repo", "/home/x/repo"), eine Gegenstelle eine Adresse
# ("vscode-remote://ssh-remote+10.8.0.10/home/x/repo"). Erkannt am Schema VOR dem "//";
# ein Windows-Laufwerksbuchstabe faellt nicht darunter, weil auf sein ":" kein "//" folgt.
_URI = re.compile(r"^[a-z][a-z0-9+.-]*://", re.IGNORECASE)


def open_args(folder: str) -> list[str]:
    """Argumente fuer Code.exe, um dieses Fenster wieder aufzumachen.

    Ein Remote-Fenster meldet statt eines Pfades seine Adresse - und die oeffnet VS Code
    nur ueber `--folder-uri`. Als nackter Parameter uebergeben wuerde sie nicht etwa
    scheitern, sondern als DATEINAME gelesen: VS Code machte einen leeren Editor mit dem
    URI als Namen auf. Ein Fehler, der wie ein Erfolg aussieht - der Prozess startet ja."""
    f = str(folder or "").strip()
    return ["--new-window", "--folder-uri", f] if _URI.match(f) else ["--new-window", f]


def foreign_repos(titles: Sequence[str], known: Sequence[str]) -> list[str]:
    """Offene VS-Code-Fenster, die das Deck NICHT kennt - benannt nach ihrem Repo.

    Das ist die Frage, an der ein Neustart sonst still scheitert: geschlossen werden
    können nur die verbundenen Fenster (nur die hören auf ein Kommando), beendet sein
    muss aber der ganze Prozess. Ein einziges fremdes Fenster - ein zweites Fenster auf
    dasselbe Repo, ein Projekt ohne Extension, ein „Willkommen"-Tab - hält VS Code am
    Leben, und der Lauf hätte alle Chats zugemacht, ohne einen wieder aufmachen zu
    können.

    `titles` sind die Fenstertitel (platform), `known` die gebundenen Repo-Namen.
    Verglichen wird ohne Groß-/Kleinschreibung; Duplikate fallen weg, die Reihenfolge
    bleibt. Ein Fenster ohne Ordner (Titel nur „Visual Studio Code") heißt hier nach
    dem Marker selbst - es ist fremd und zählt mit, denn es hält den Prozess genauso.
    """
    # Verglichen wird ohne Remote-Anhaengsel (siehe binding.repo_from_title),
    # gemeldet wird MIT - sonst haelt der Neustart ein eigenes Remote-Fenster fuer
    # fremd und bricht ab.
    have = {strip_remote_tag(k).strip().lower() for k in known if k}
    out: list[str] = []
    for title in titles:
        low = repo_from_title(title).strip().lower()
        if not low or low in have or low in {strip_remote_tag(r).lower() for r in out}:
            continue
        out.append(window_from_title(title))
    return out


def counts(plan: dict[str, Any]) -> tuple[int, int, int]:
    """(Fenster, Slots mit Session, Slots ohne) - für die Rückmeldung im Dialog."""
    wins = plan.get("windows") or []
    slots = [s for w in wins for s in (w.get("slots") or [])]
    fresh = sum(1 for s in slots if s.get("fresh"))
    return len(wins), len(slots) - fresh, fresh


def is_usable(plan: Any) -> bool:
    """Taugt das als Plan? (Ein Objekt mit mindestens einem Fenster samt Ordner.)"""
    if not isinstance(plan, dict):
        return False
    wins = plan.get("windows")
    return isinstance(wins, list) and any(
        isinstance(w, dict) and w.get("folder") and w.get("window") for w in wins)


def needs_relaunch(plan: Any, connected: Sequence[str]) -> bool:
    """Ist das ein liegengebliebener Plan, dessen Fenster ALLE zu sind?

    Dann ist ein früherer Lauf zwischen „geschlossen" und „wieder da" abgebrochen (Panel
    beendet, Timeout, Rechner ausgegangen) - und der Knopf soll fortsetzen statt einen
    frischen Plan aus null verbundenen Fenstern zu bauen, der nichts mehr enthält.
    Sobald auch nur eines der geplanten Fenster wieder verbunden ist, gilt der Lauf als
    erledigt (oder als vom Nutzer selbst wieder geöffnet) - dann lieber neu planen.
    """
    if not is_usable(plan):
        return False
    live = {str(w).upper() for w in connected}
    return not any((w.get("window") or "").upper() in live
                   for w in plan["windows"] if isinstance(w, dict))


def save(plan: dict[str, Any], path: str = PLAN_FILE) -> None:
    """Atomar ablegen (paths.save_json). Fehler sind hier nicht tödlich: der Lauf
    funktioniert auch ohne Datei, nur das Fortsetzen nach einem Absturz nicht."""
    try:
        save_json(path, plan)
    except Exception:
        pass


def load(path: str = PLAN_FILE) -> dict[str, Any] | None:
    """Liegengebliebenen Plan lesen. Fehlt/kaputt/unbrauchbar -> None."""
    raw = load_json(path)
    return raw if is_usable(raw) else None


def clear(path: str = PLAN_FILE) -> None:
    """Plan-Datei entfernen (nach einem geglückten Lauf). Fehlt sie -> still ok."""
    try:
        os.remove(path)
    except OSError:
        pass
