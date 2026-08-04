"""Claude-Nutzung (Session + Wochenlimits) fuer den Header – Datenschicht.

Fragt
    https://api.anthropic.com/api/oauth/usage
mit dem OAuth-Token des angemeldeten Kontos ab. Keine Browser-Extension noetig.

ZWEI QUELLEN fuer das Token, beide werden gelesen und der Reihe nach probiert
(read_oauth_token sammelt sie, fetch_usage nimmt das erste, das 200 liefert):

  1. Claude Code CLI – `~/.claude/.credentials.json`, KLARTEXT-JSON unter
     "claudeAiOauth". Der Normalfall: wer das Deck benutzt, hat die CLI zwingend
     installiert, Claude Desktop dagegen oft nicht.
  2. Claude Desktop – dessen config.json, VERSCHLUESSELT. Der Token-Blob ist ein
     Chromium-"v10"-Paket: der AES-256-Schluessel steckt (per Windows-DPAPI
     geschuetzt) in "Local State", das Token selbst ist AES-256-GCM. Damit das
     Deck ABHAENGIGKEITSFREI bleibt (nur stdlib + ctypes, wie der Rest der App),
     entschluesselt _aesgcm_decrypt ueber Windows CNG (bcrypt.dll); klappt das
     aus irgendeinem Grund nicht, faellt es auf das 'cryptography'-Paket zurueck.

Warum ueberhaupt beide: die Tokens haben unterschiedliche Laufzeiten und Scopes.
Ist eins abgelaufen oder wird es mit 401 abgewiesen, traegt das andere weiter —
ohne dass der Nutzer etwas merkt.

Gelesen werden die Dateien NICHT bei jedem Poll: read_oauth_token haelt die Tokens
in _token_cache und liest erst neu, wenn die API eins abweist (401/403 -> force).
Ein erneuertes Token kostet also genau einen fehlgeschlagenen Abruf, kein Polling
auf der Platte.

Alles ist defensiv: fehlt jede Quelle / ein Token / das Netz, liefern die
Funktionen definierte Fehler, die der UsagePoller abfaengt und als Fehlertext in
den Snapshot legt. Das Deck laeuft ungestoert weiter; das Badge zeigt dann "—".

Bewusst OHNE tkinter-Import -> die puren Parser (parse_usage / fmt_reset /
severity_color / tooltip_text) sind headless testbar (tests/test_claude_usage.py).
"""
import json
import os
import random
import threading
import time
import urllib.error
import urllib.request
from typing import Any

from deck import i18n
from deck.claude.usage_token import NoTokenError, read_oauth_token
from deck.claude.usage_view import expire_limits, parse_usage, session_of

USAGE_URL = "https://api.anthropic.com/api/oauth/usage"


# ── Optionaler gemeinsamer Ein-Poller ────────────────────
# NICHT Teil dieses Repos und fuer nichts noetig: wer NEBEN dem Deck noch einen
# zweiten Usage-Anzeiger laufen laesst, kann beide ueber ein Cache-/Mutex-Modul
# denselben Abruf teilen lassen, statt den rate-limitierten Endpoint doppelt zu
# pollen (das erschoepfte sonst das Account-Limit -> HTTP 429). Gesucht wird es
# unter CLAUDE_USAGE_SHARED_DIR bzw. im Nachbarordner 'claude-usage-shared'.
# Fehlt es — der Normalfall —, ruft UsagePoller unten einfach selbst ab.
_shared_mod: Any = "unset"


def _shared() -> Any:
    global _shared_mod
    if _shared_mod != "unset":
        return _shared_mod
    import importlib
    import sys

    from deck.domain import paths
    here = paths.REPO_ROOT
    for p in (os.environ.get("CLAUDE_USAGE_SHARED_DIR"),
              os.path.join(here, "..", "claude-usage-shared")):
        if p and os.path.isfile(os.path.join(p, "usage_poller.py")):
            p = os.path.abspath(p)
            if p not in sys.path:
                sys.path.insert(0, p)
            try:
                _shared_mod = importlib.import_module("usage_poller")
            except Exception:
                _shared_mod = None
            return _shared_mod
    _shared_mod = None
    return _shared_mod


# ── Usage-API abfragen ───────────────────────────────────
def _fetch_one(token: str) -> dict[str, Any]:
    req = urllib.request.Request(USAGE_URL, headers={
        "Authorization": "Bearer " + token,
        "anthropic-beta": "oauth-2025-04-20",
        "Accept": "application/json",
        "User-Agent": "agent-deck-usage/1",
    })
    with urllib.request.urlopen(req, timeout=20) as r:
        return json.loads(r.read().decode("utf-8"))


def fetch_usage(tokens: str | list[str]) -> dict[str, Any]:
    """Probiert die Tokens der Reihe nach; nimmt das erste, das 200 liefert.
    401/403 -> naechstes Token; alles andere (429, 5xx, Timeout) fliegt hoch."""
    if isinstance(tokens, str):
        tokens = [tokens]
    last_err = None
    for tok in tokens:
        try:
            return _fetch_one(tok)
        except urllib.error.HTTPError as e:
            last_err = e
            if e.code in (401, 403):
                continue
            raise
    raise last_err if last_err else RuntimeError("Kein Token verfuegbar")


# ── Hintergrund-Poller ───────────────────────────────────
def _empty_snapshot() -> dict[str, Any]:
    return {"state": "pending", "session_percent": None, "session_severity": "",
            "session_resets_at": None, "limits": [], "error": None, "ts": None}


class UsagePoller:
    """Fragt die Claude-Nutzung in einem Daemon-Thread ab und haelt den letzten
    Snapshot thread-sicher. Die UI liest ihn per snapshot() aus ihrem eigenen
    after()-Timer (kein Tk-Zugriff aus dem Thread).

    Bis zum ersten Erfolg wird schnell gepollt (die frisch startende Claude-App
    schreibt ihre Dateien staendig neu). Danach im ruhigen poll_seconds-Takt mit
    etwas Jitter – Nutzung aendert sich langsam, und ein groesserer Takt schont das
    API-Rate-Limit (v.a. wenn parallel der Usage-Monitor pollt). Auf 429/Netzfehler
    bleibt der letzte Wert stehen (nur der Fehlertext wird vermerkt)."""

    def __init__(self, poll_seconds: float = 120) -> None:
        self.poll_seconds = max(30, int(poll_seconds))
        self._snap = _empty_snapshot()
        self._lock = threading.Lock()
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None

    def start(self) -> None:
        if self._thread is not None:
            return
        self._thread = threading.Thread(target=self._loop, name="usage-poll", daemon=True)
        self._thread.start()

    def stop(self) -> None:
        self._stop.set()

    def snapshot(self) -> dict[str, Any]:
        with self._lock:
            return dict(self._snap)

    def _set(self, **kw: Any) -> None:
        with self._lock:
            self._snap.update(kw)

    def poll_once(self) -> None:
        """Ein Abruf ueber den gemeinsamen Poller (Fallback: lokaler Direktabruf);
        aktualisiert den Snapshot. Fehler landen im 'error'-Feld, harte Fehler
        (Token/Ordner weg) blenden zusaetzlich die Zahl aus."""
        sh = _shared()
        if sh is not None:
            try:
                snap = sh.get_usage()
            except Exception as e:
                self._fail(f"{type(e).__name__}", hard=False)
                return
            data, err = snap.get("data"), snap.get("error")
            if data is None:
                el = (err or "").lower()
                if "ungueltig" in el:
                    self._fail(i18n.L("Token ungueltig – 'claude auth login'",
                                      "Token invalid – run 'claude auth login'"), hard=True)
                elif "rate" in el:
                    self._fail(i18n.L("Rate-Limit – kurz warten",
                                      "Rate limit – wait a moment"), hard=False)
                else:
                    self._fail(err or i18n.L("warte auf Daten…", "waiting for data…"),
                               hard=False)
                return
            # Zahl aus dem gemeinsamen Cache. Sie kann ALT sein: bei 429 laesst der
            # gemeinsame Poller den letzten Wert stehen und vermerkt nur den Fehler.
            # Beides gehoert darum in den Snapshot — 'data_ts' als echte Datenzeit
            # (nicht 'jetzt', das ist nur der Cache-Lesezeitpunkt) und 'err', auch
            # wenn Daten da sind. Wer den Fehler hier verschluckt, laesst das Deck
            # einen eingefrorenen Wert als frisch ausgeben.
            self._apply(data, ts=snap.get("data_ts"), error=err or None)
            return

        # ── Fallback: lokaler Direktabruf (shared-Modul nicht gefunden) ──
        try:
            try:
                data = fetch_usage(read_oauth_token())
            except urllib.error.HTTPError as e:
                if e.code in (401, 403):                 # gecachtes Token rotiert?
                    data = fetch_usage(read_oauth_token(force=True))
                else:
                    raise
            self._apply(data, ts=time.time(), error=None)   # gerade selbst geholt
        except urllib.error.HTTPError as e:
            if e.code in (401, 403):
                self._fail(i18n.L("Token ungueltig – 'claude auth login'",
                                  "Token invalid – run 'claude auth login'"), hard=True)
            elif e.code == 429:
                self._fail(i18n.L("Rate-Limit – kurz warten",
                                  "Rate limit – wait a moment"), hard=False)
            else:
                self._fail(f"HTTP {e.code}", hard=False)
        except (NoTokenError, FileNotFoundError):
            # Weder CLI noch Desktop haben ein Token. Der Hinweis zeigt auf die CLI:
            # die hat jeder Deck-Nutzer, und ein Login dort ist der kuerzere Weg.
            self._fail(i18n.L("Nicht angemeldet – 'claude auth login'",
                              "Not signed in – run 'claude auth login'"), hard=True)
        except Exception as e:
            self._fail(f"{type(e).__name__}", hard=False)

    def _apply(self, data: dict[str, Any], *, ts: float | None,
               error: str | None) -> None:
        """Geparste Antwort in den Snapshot legen — abgelaufene Limits entwertet.

        Die Entwertung sitzt hier und nicht in parse_usage, weil parse_usage die
        Antwort NUR uebersetzt (gleiche Eingabe, gleiche Ausgabe, keine Uhr). Ob ein
        Wert noch gilt, ist eine Frage an die Uhr — und die stellt sich genau beim
        Uebernehmen in den Snapshot."""
        limits = expire_limits(parse_usage(data)["limits"])
        sess = session_of(limits)
        self._set(state="ok" if not error else "error",
                  session_percent=(sess["percent"] if sess else None),
                  session_severity=(sess["severity"] if sess else ""),
                  session_resets_at=(sess["resets_at"] if sess else None),
                  limits=limits, error=error, ts=ts)

    def _fail(self, msg: str, hard: bool) -> None:
        with self._lock:
            self._snap["state"] = "error"
            self._snap["error"] = msg
            if hard:
                self._snap["session_percent"] = None
                self._snap["session_severity"] = ""
                self._snap["limits"] = []

    def _loop(self) -> None:
        while not self._stop.is_set():
            try:
                self.poll_once()
            except Exception:
                pass                                     # die Schleife darf nie sterben
            with self._lock:
                have = self._snap["session_percent"] is not None
                errored = self._snap["error"] is not None
            # Mit gemeinsamem Poller ist poll_once nur ein guenstiger Cache-Read
            # (der API-Takt + Backoff steckt zentral im shared-Modul). Dann darf
            # das Badge oefter spiegeln, ohne das Rate-Limit zu belasten.
            base = 30 if _shared() is not None else self.poll_seconds
            if have:
                delay = base + random.uniform(-8, 8)     # Jitter gegen Lockstep
            elif errored:
                delay = min(30, self.poll_seconds)       # 429/Token/kein Claude -> zurueckfallen,
                                                         # NICHT alle 5 s weiterhaemmern
            else:
                delay = 5                                # frischer Start: bis zur ersten Zahl fix
            self._stop.wait(max(3.0, delay))
