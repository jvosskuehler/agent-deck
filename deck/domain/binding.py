"""Persistenz rund um die Fenster-Bindung: welches VS-Code-Fenster (A/B/C/D) auf
welches Repo zeigt (bindings.json) und welches Effort je Slot zuletzt gesetzt war
(slot_effort.json). Dazu die zwei kleinen Workspace-Helfer, die entscheiden, was
ueberhaupt ein bindbarer Workspace ist bzw. wie sein Name aus dem Fenstertitel
faellt – sie gehoeren fachlich zur Bindung, nicht zum Zeichnen.

Beide Dateien werden selbstheilend geladen und atomar (deck_paths) geschrieben.
"""
import os
import re
from typing import Any

from deck.domain import config as cfg
from deck.domain.paths import REPO_ROOT, load_json, save_json

# Die Laufzeit-JSONs liegen neben dem Code, also in der Repo-Wurzel -
# NICHT neben diesem Modul (siehe paths.REPO_ROOT).
_DIR = REPO_ROOT
BIND_FILE = os.path.join(_DIR, "bindings.json")
EFFORT_FILE = os.path.join(_DIR, "slot_effort.json")
SETTINGS_FILE = os.path.join(_DIR, "deck_settings.json")
TICKET_FILE = os.path.join(_DIR, "tickets.json")
ORDER_FILE = os.path.join(_DIR, "slot_order.json")
WIN_ORDER_FILE = os.path.join(_DIR, "win_order.json")


def is_placeholder_ws(ws: str | None) -> bool:
    """'Kein echtes Projekt' -> nicht binden/persistieren (sonst Phantom-Kachel).
    Platzhalter sind: leer/None und das reservierte Sentinel 'unknown'. 'unknown'
    war frueher der Name, den ein VS-Code-Fenster OHNE geoeffneten Ordner meldete
    (die Extension meldet solche Fenster inzwischen als null). Es wird bewusst
    UEBERALL als Platzhalter behandelt – u.a. damit ein noch nicht neu geladenes
    altes Fenster das Phantom nicht zur Laufzeit erneut bindet. Preis: ein ECHTER
    Ordner namens 'unknown' wird nicht automatisch verfolgt (praktisch nie relevant;
    dann Ordner umbenennen)."""
    return not ws or str(ws).strip().lower() in ("", "unknown")


def repo_from_title(title: str) -> str:
    """Aus 'file - repo - Visual Studio Code' den Repo-/Ordnernamen ziehen."""
    t = title.replace("●", " ").strip()
    parts = [p.strip() for p in t.split(" - ") if p.strip()]
    if cfg.VSCODE_MARKER in parts:
        i = parts.index(cfg.VSCODE_MARKER)
        if i >= 1:
            return parts[i - 1]
    return parts[-1] if parts else ""


def ticket_slug(ticket: str) -> str:
    """Ticket-ID -> ordner-/branch-tauglicher Slug: klein, nur [a-z0-9], alles andere
    zu einem einzelnen '-' zusammengefasst, Raender getrimmt. 'ABC-123: Login fix'
    -> 'abc-123-login-fix'. Leer -> '' (Aufrufer faengt das ab)."""
    out = []
    for ch in str(ticket).strip().lower():
        out.append(ch if ch.isalnum() else "-")
    slug = "".join(out)
    while "--" in slug:
        slug = slug.replace("--", "-")
    return slug.strip("-")


def ticket_branch(ticket: str, prefix: str | None = None) -> str:
    """Branch-Name aus Ticket-ID: PREFIX + slug (z.B. 'ticket/abc-123'). Leerer Slug
    -> ''. prefix default aus config.TICKET_BRANCH_PREFIX."""
    slug = ticket_slug(ticket)
    if not slug:
        return ""
    pref = prefix if prefix is not None else cfg.TICKET_BRANCH_PREFIX
    return pref + slug


def jira_key(ticket: str, project: str | None = None) -> str:
    """Aus der eingegebenen Ticket-ID einen Jira-Issue-Key machen, damit der Agent das
    Ticket eindeutig nachschlagen kann. Steckt schon ein Key der Form ABC-123 drin ->
    unveraendert uebernehmen (Projektteil gross, z.B. 'proj-42' -> 'PROJ-42'). Wurde nur
    eine Nummer eingegeben (evtl. mit fuehrendem '#') -> das Projekt-Praefix davorsetzen
    (project default aus config.JIRA_PROJECT_KEY; leer -> Nummer bleibt roh). Passt keins
    von beidem, kommt die Eingabe unveraendert zurueck (der Agent versucht dann sein
    Bestes / fragt nach). Leer -> ''."""
    t = str(ticket or "").strip()
    if not t:
        return ""
    m = re.match(r"^([A-Za-z][A-Za-z0-9]*)-(\d+)$", t)
    if m:                                    # schon ein Key -> Projektteil gross
        return m.group(1).upper() + "-" + m.group(2)
    num = t.lstrip("#").strip()
    proj = (project if project is not None
            else cfg.JIRA_PROJECT_KEY or "").strip().upper()
    if proj and num.isdigit():               # nur Nummer + Projekt bekannt -> Key bauen
        return proj + "-" + num
    return t


class BindStore:
    """Laedt/speichert bindings.json + slot_effort.json. `bindings` und `effort`
    sind schlichte Dicts, die der Aufrufer direkt mutiert; nach Aenderungen
    save_bindings()/save_effort() rufen."""

    def __init__(self, bind_file: str = BIND_FILE, effort_file: str = EFFORT_FILE,
                 ticket_file: str = TICKET_FILE, settings_file: str = SETTINGS_FILE,
                 order_file: str = ORDER_FILE,
                 win_order_file: str = WIN_ORDER_FILE) -> None:
        self.bind_file = bind_file
        self.effort_file = effort_file
        self.ticket_file = ticket_file
        self.settings_file = settings_file
        self.order_file = order_file
        self.win_order_file = win_order_file
        self.bindings = self._load_bindings()   # {"A": repo, "B": repo, …}
        self.effort = self._load_effort()        # {slot: "xhigh"/"ultracode"}
        self.tickets = self._load_tickets()      # {slot: "ABC-123"}
        self.settings = self._load_settings()    # {"slim": bool, …} Panel-Einstellungen
        self.order = self._load_order()          # {"A": [slot, …]} vom Nutzer gezogene Reihenfolge
        self.win_order = self._load_win_order()  # ["repo-b", …] gezogene Block-Reihenfolge

    def _load_bindings(self) -> dict[str, str]:
        raw = load_json(self.bind_file)
        if not isinstance(raw, dict):
            # Erst-Start / kaputt / kein Objekt -> aus config.WINDOW_MATCH vorbelegen.
            raw = dict(cfg.WINDOW_MATCH or {})
        # Selbstheilung: Platzhalter-/Leer-/Alt-'unknown'-Bindungen beim Laden
        # rauswerfen und die bereinigte Datei zurueckschreiben -> nach einem
        # Neustart ist ein altes Phantom weg.
        clean = {w: repo for w, repo in raw.items() if not is_placeholder_ws(repo)}
        if clean != raw:
            try:
                save_json(self.bind_file, clean)
            except Exception:
                pass
        return clean

    def save_bindings(self) -> None:
        try:
            save_json(self.bind_file, self.bindings)
        except Exception:
            pass

    def _load_effort(self) -> dict[str, str]:
        """Gemerktes Effort je Slot. Nur so ist ultracode von xhigh unterscheidbar -
        die statusLine meldet fuer beide nur 'xhigh'. Persistent, damit die
        Unterscheidung Neustart/Modellwechsel ueberlebt."""
        raw = load_json(self.effort_file)
        return raw if isinstance(raw, dict) else {}

    def save_effort(self) -> None:
        try:
            save_json(self.effort_file, self.effort)
        except Exception:
            pass

    def _load_tickets(self) -> dict[str, str]:
        """Zugewiesenes Ticket je Slot (Anzeige auf der Karte). Persistent, damit die
        Ticket-ID einen Panel-Neustart ueberlebt. {slot: ticket-id-string}."""
        raw = load_json(self.ticket_file)
        if not isinstance(raw, dict):
            return {}
        # Selbstheilung: nur String-Werte behalten (leere/kaputte Eintraege raus).
        return {k: str(v) for k, v in raw.items() if isinstance(v, str) and v.strip()}

    def save_tickets(self) -> None:
        try:
            save_json(self.ticket_file, self.tickets)
        except Exception:
            pass

    def _load_settings(self) -> dict[str, Any]:
        """Panel-weite Einstellungen (z.B. Slim-Modus an/aus). Ein schlichtes Dict,
        das der Aufrufer direkt mutiert; danach save_settings() rufen. Persistent,
        damit ein Moduswechsel einen Panel-Neustart ueberlebt."""
        raw = load_json(self.settings_file)
        return raw if isinstance(raw, dict) else {}

    def save_settings(self) -> None:
        try:
            save_json(self.settings_file, self.settings)
        except Exception:
            pass

    def _load_order(self) -> dict[str, list[str]]:
        """Vom Nutzer per Drag&Drop gewaehlte Kachel-Reihenfolge je Fenster. VS Code
        gibt die visuelle Terminal-/Pane-Reihenfolge nicht preis (kein Positions-API)
        -> das Deck ist die Quelle der Wahrheit und merkt sie hier persistent, damit
        eine getauschte Anordnung einen Panel-Neustart ueberlebt. {win: [slotname, …]}.
        Selbstheilend: nur Fenster-Buchstaben mit einer Liste von Strings behalten."""
        raw = load_json(self.order_file)
        if not isinstance(raw, dict):
            return {}
        clean = {}
        for w, seq in raw.items():
            if isinstance(seq, list):
                clean[str(w)] = [str(s) for s in seq if isinstance(s, str) and s]
        return clean

    def save_order(self) -> None:
        try:
            save_json(self.order_file, self.order)
        except Exception:
            pass

    def _load_win_order(self) -> list[str]:
        """Vom Nutzer per Drag&Drop gewaehlte Reihenfolge der REPO-BLOECKE - also der
        Bloecke untereinander, nicht der Kacheln darin (die stehen in slot_order.json).

        Eine Liste von REPO-Namen, z.B. ["backend", "agent-deck"] - NICHT von
        Fenster-Buchstaben: der Buchstabe wird beim Schliessen eines Fensters frei und neu
        vergeben, eine Reihenfolge daraus waere nach dem naechsten Fenster-Wechsel eine
        andere (Begruendung in ui/tiles.py, _block_key). Sie darf mehr Repos nennen, als
        gerade offen sind - ein geschlossenes behaelt so seinen Platz - und muss nicht
        vollstaendig sein: unbekannte haengen beim Anzeigen hinten an.

        Selbstheilend: nur nicht-leere Strings, Duplikate raus."""
        raw = load_json(self.win_order_file)
        if not isinstance(raw, list):
            return []
        clean: list[str] = []
        for w in raw:
            if isinstance(w, str) and w and w not in clean:
                clean.append(w)
        return clean

    def save_win_order(self) -> None:
        try:
            save_json(self.win_order_file, self.win_order)
        except Exception:
            pass
