"""claude_usage: Parser der oauth/usage-Antwort und die Token der Claude-Code-CLI.
"""

import os
import time
from datetime import UTC, datetime

import helpers  # noqa: F401 - Import MIT Absicht: legt die Repo-Wurzel auf den

# sys.path und nagelt die Deck-Sprache auf Deutsch.
from deck.claude import usage
from deck.claude import usage_token as utok
from deck.claude import usage_view as uview

# Ausschnitt einer echten API-Antwort (Session kritisch bei 91 %, Woche 15 %, dazu
# ein modell-spezifisches Wochenlimit bei 0 %).
_USAGE_SAMPLE = {
    "five_hour": {"utilization": 91.0, "resets_at": "2026-07-21T23:00:00+00:00"},
    "seven_day": {"utilization": 15.0, "resets_at": "2026-07-28T12:00:00+00:00"},
    "limits": [
        {"kind": "session", "group": "session", "percent": 91, "severity": "critical",
         "resets_at": "2026-07-21T23:00:00+00:00", "scope": None, "is_active": True},
        {"kind": "weekly_all", "group": "weekly", "percent": 15, "severity": "normal",
         "resets_at": "2026-07-28T12:00:00+00:00", "scope": None, "is_active": False},
        {"kind": "weekly_scoped", "group": "weekly", "percent": 0, "severity": "normal",
         "resets_at": None, "is_active": False,
         "scope": {"model": {"id": None, "display_name": "Fable"}, "surface": None}},
    ],
}


def test_usage_fmt_reset():
    base = datetime(2026, 1, 1, 12, 0, tzinfo=UTC)
    assert uview.fmt_reset("2026-01-01T12:45:00+00:00", base) == "45 Min."
    assert uview.fmt_reset("2026-01-01T14:00:00+00:00", base) == "2 Std."
    assert uview.fmt_reset("2026-01-01T14:30:00+00:00", base) == "2 Std. 30 Min."
    assert uview.fmt_reset("2026-01-03T12:00:00+00:00", base) == "2 Tg."
    assert uview.fmt_reset("2026-01-03T15:00:00+00:00", base) == "2 Tg. 3 Std."
    assert uview.fmt_reset("2026-01-01T11:00:00+00:00", base) == "jetzt"    # Vergangenheit
    assert uview.fmt_reset("2026-01-01T12:45:00", base) == "45 Min."        # naiv -> als UTC
    assert uview.fmt_reset(None, base) == ""
    assert uview.fmt_reset("kaputt", base) == ""


def test_usage_severity_color():
    assert uview.severity_color("critical", 91) == "#ff6b6b"
    assert uview.severity_color("warning", 60) == "#ffc48a"
    assert uview.severity_color("normal", 15) == "#6ee7a8"
    assert uview.severity_color("", None) == "#8b8b99"                      # kein Wert -> grau
    assert uview.severity_color("", 30) == "#6ee7a8"                        # Fallback per Schwelle
    assert uview.severity_color("", 70) == "#ffc48a"
    assert uview.severity_color("", 95) == "#ff6b6b"


def test_usage_parse_limits():
    p = uview.parse_usage(_USAGE_SAMPLE)
    assert p["session"]["percent"] == 91
    assert p["session"]["severity"] == "critical"
    assert p["session"]["group"] == "session"
    assert [lim["label"] for lim in p["limits"]] == ["Session", "Woche", "Fable (Woche)"]


def test_usage_parse_fallback():
    # Aeltere Antwort ohne 'limits' -> aus five_hour/seven_day rekonstruiert.
    p = uview.parse_usage({"five_hour": {"utilization": 91.0, "resets_at": "x"},
                        "seven_day": {"utilization": 15.0, "resets_at": "y"}})
    assert p["session"]["percent"] == 91
    assert [lim["label"] for lim in p["limits"]] == ["Session", "Woche"]


def test_usage_parse_empty():
    p = uview.parse_usage({})
    assert p["session"] is None and p["limits"] == []


def test_usage_tooltip_text():
    base = datetime(2026, 7, 21, 21, 55, tzinfo=UTC)
    snap = {"state": "ok", "limits": uview.parse_usage(_USAGE_SAMPLE)["limits"], "error": None}
    txt = uview.tooltip_text(snap, base)
    assert "Claude – Nutzung" in txt
    assert "Session: 91 %" in txt
    assert "Woche: 15 %" in txt
    assert "Reset in 1 Std. 5 Min." in txt          # Session-Reset relativ zu base
    assert "Fable" not in txt                        # 0 %/kein Reset/inaktiv -> ausgefiltert


def test_usage_tooltip_error():
    txt = uview.tooltip_text({"state": "error", "limits": [], "error": "nicht angemeldet"})
    assert "nicht angemeldet" in txt


# ── Alte Werte: die Falle vom 2026-08-05 ─────────────────
def test_usage_limit_hinter_dem_reset_verliert_seine_zahl():
    """Der Kern des Fehlers: hinter dem Reset zaehlt die API ein neues Fenster ab 0,
    ein gecachter Wert von davor beschreibt nichts mehr. Er darf nicht stehenbleiben —
    er stand zu hoch (48 % von gestern statt 13 %) und sah dabei voellig normal aus."""
    base = datetime(2026, 7, 21, 23, 30, tzinfo=UTC)      # NACH dem Session-Reset (23:00)
    limits = uview.expire_limits(uview.parse_usage(_USAGE_SAMPLE)["limits"], base)
    sess = uview.session_of(limits)
    assert sess["percent"] is None, "der Wert hinter dem Reset muss weg"
    assert sess["severity"] == "" and sess["stale"] is True
    # Die Woche laeuft noch (Reset am 28.) -> unangetastet.
    woche = next(lim for lim in limits if lim["kind"] == "weekly_all")
    assert woche["percent"] == 15 and not woche.get("stale")


def test_usage_limit_vor_dem_reset_bleibt_unangetastet():
    base = datetime(2026, 7, 21, 21, 55, tzinfo=UTC)      # VOR dem Reset
    limits = uview.expire_limits(uview.parse_usage(_USAGE_SAMPLE)["limits"], base)
    assert uview.session_of(limits)["percent"] == 91
    assert all(not lim.get("stale") for lim in limits)


def test_usage_limit_ohne_reset_wird_nicht_entwertet():
    """Unwissen ist kein Grund zu entwerten: fehlt resets_at, bleibt der Wert stehen.
    (Das modell-spezifische Wochenlimit kommt ohne Reset-Zeit.)"""
    limits = uview.expire_limits([{"kind": "weekly_scoped", "group": "weekly",
                                   "percent": 7, "severity": "normal",
                                   "resets_at": None, "active": True}])
    assert limits[0]["percent"] == 7 and not limits[0].get("stale")
    # Kaputte Zeitangabe ebenso — nicht raten.
    kaputt = uview.expire_limits([{"kind": "session", "group": "session", "percent": 7,
                                   "severity": "normal", "resets_at": "murks",
                                   "active": True}])
    assert kaputt[0]["percent"] == 7


def test_usage_alter_erst_ab_der_schwelle():
    """Alter ist im Normalbetrieb kein Befund (der gemeinsame Poller cacht 90 s und
    haelt im 429-Backoff bis 10 Min.) — erst darueber nennt der Tooltip den Stand."""
    now = 1_000_000.0
    assert uview.fmt_age(now - 60, now) == ""                    # frisch -> nichts
    assert uview.fmt_age(now - 300, now) == ""                    # Backoff -> noch normal
    assert uview.fmt_age(now - 3600, now) == "vor 1 Std."
    assert uview.fmt_age(now - 5400, now) == "vor 1 Std. 30 Min."
    assert uview.fmt_age(None, now) == "" and uview.fmt_age("alt", now) == ""


def test_usage_tooltip_nennt_stand_und_grund():
    """Ohne diese Zeile sieht ein eingefrorener Wert wie ein frischer aus."""
    base = datetime(2026, 7, 21, 21, 55, tzinfo=UTC)
    limits = uview.parse_usage(_USAGE_SAMPLE)["limits"]
    alt = {"state": "error", "limits": limits, "error": "Rate-Limit",
           "ts": base.timestamp() - 3600}
    txt = uview.tooltip_text(alt, base)
    assert "(Stand vor 1 Std. – Rate-Limit)" in txt, txt
    # Fehler ohne bekannte Datenzeit -> die alte Formulierung bleibt.
    ohne_ts = uview.tooltip_text({"limits": limits, "error": "Rate-Limit"}, base)
    assert "(letzter Wert – Rate-Limit)" in ohne_ts, ohne_ts
    # Frische Daten ohne Fehler -> gar keine Fusszeile.
    frisch = uview.tooltip_text({"limits": limits, "error": None,
                                 "ts": base.timestamp()}, base)
    assert "(" not in frisch, frisch


def test_usage_poller_uebernimmt_datenzeit_und_fehler_des_gemeinsamen_pollers():
    """Die Naht, an der es riss: der gemeinsame Poller laesst bei 429 den LETZTEN Wert
    stehen und vermerkt nur den Fehler. Wer beim Uebernehmen 'data_ts' durch 'jetzt'
    ersetzt und 'error' auf None setzt, macht daraus eine frische, falsche Zahl."""
    jetzt = time.time()
    vorbei = datetime.fromtimestamp(jetzt - 1800, UTC).isoformat()   # Reset vor 30 Min.
    laeuft = datetime.fromtimestamp(jetzt + 86400, UTC).isoformat()
    data = {"limits": [
        {"kind": "session", "group": "session", "percent": 48, "severity": "normal",
         "resets_at": vorbei, "is_active": True},
        {"kind": "weekly_all", "group": "weekly", "percent": 9, "severity": "normal",
         "resets_at": laeuft, "is_active": False}]}
    alt_ts = jetzt - 3600

    class _Attrappe:
        @staticmethod
        def get_usage():
            return {"data": data, "data_ts": alt_ts, "error": "Rate-Limit"}

    ruhe = usage._shared_mod
    try:
        usage._shared_mod = _Attrappe
        p = usage.UsagePoller()
        p.poll_once()
        snap = p.snapshot()
    finally:
        usage._shared_mod = ruhe

    assert snap["ts"] == alt_ts, "die Datenzeit muss die des ABRUFS sein, nicht jetzt"
    assert snap["error"] == "Rate-Limit", "der Fehler darf nicht verschluckt werden"
    assert snap["session_percent"] is None, "48 % aus dem alten Fenster sind keine Zahl"
    assert snap["session_severity"] == ""        # -> graues '—' statt gruener Ampel
    woche = next(lim for lim in snap["limits"] if lim["kind"] == "weekly_all")
    assert woche["percent"] == 9, "die Woche laeuft weiter und bleibt gueltig"


# ── claude_usage: Token der Claude-Code-CLI ──────────────
# Aufbau von ~/.claude/.credentials.json, wie 2026-07-29 vorgefunden. Die Werte sind
# erfunden; geprueft wird nur, dass wir die richtigen FELDER lesen.
def _creds(token="tok-neu", expires_at=None, key="claudeAiOauth"):
    inner = {"accessToken": token, "refreshToken": "rt", "scopes": ["user:inference"],
             "subscriptionType": "max", "rateLimitTier": "default"}
    if expires_at is not None:
        inner["expiresAt"] = expires_at
    return {key: inner}


def test_cli_token_wird_gelesen():
    soon = (time.time() + 3600) * 1000                   # Ablauf in Millisekunden
    assert utok.tokens_from_credentials(_creds("tok-a", soon)) == ["tok-a"]


def test_cli_token_abgelaufen_wird_nicht_gesendet():
    past = (time.time() - 60) * 1000
    assert utok.tokens_from_credentials(_creds("tok-alt", past)) == []


def test_cli_token_ohne_ablaufzeit_gilt_als_gueltig():
    """Ein totes Token kostet nur einen 401 – fetch_usage nimmt dann das naechste.
    Es wegzuwerfen, nur weil das Feld fehlt, waere der teurere Fehler."""
    assert utok.tokens_from_credentials(_creds("tok-b")) == ["tok-b"]


def test_cli_token_auch_ohne_bekannten_container():
    """Das Dateiformat gehoert der CLI und ist nicht dokumentiert. Liegt das Token
    flach oder in snake_case, darf die Anzeige trotzdem nicht ausfallen."""
    assert utok.tokens_from_credentials({"accessToken": "flach"}) == ["flach"]
    assert utok.tokens_from_credentials(_creds("snake", key="claude_ai_oauth")) == ["snake"]
    assert utok.tokens_from_credentials({"claudeAiOauth": {"token": "alt"}}) == ["alt"]


def test_cli_token_muell_gibt_leere_liste():
    for muell in (None, {}, [], "text", {"claudeAiOauth": {}},
                  {"claudeAiOauth": {"accessToken": ""}}, {"claudeAiOauth": None}):
        assert utok.tokens_from_credentials(muell) == [], muell


def test_cli_credentials_pfad_folgt_der_umgebungsvariable():
    alt = os.environ.get("CLAUDE_CONFIG_DIR")
    try:
        os.environ["CLAUDE_CONFIG_DIR"] = os.path.join("X:", "woanders")
        assert utok.cli_credentials_path() == os.path.join("X:", "woanders",
                                                         ".credentials.json")
        os.environ.pop("CLAUDE_CONFIG_DIR")
        assert utok.cli_credentials_path().endswith(
            os.path.join(".claude", ".credentials.json"))
    finally:
        os.environ.pop("CLAUDE_CONFIG_DIR", None)
        if alt is not None:
            os.environ["CLAUDE_CONFIG_DIR"] = alt


def test_beide_tokenquellen_werden_zusammengelegt():
    """Der Kern des Fallbacks: faellt EINE Quelle aus, traegt die andere. Erst wenn
    beide nichts liefern, ist es ein Fehler – und der nennt beide Quellen."""
    ruhe = utok._token_cache
    cli, desk = utok._read_tokens_from_cli, utok._read_tokens_from_disk
    try:
        utok._token_cache = []
        utok._read_tokens_from_cli = lambda: ["cli"]
        utok._read_tokens_from_disk = lambda: ["desk"]
        assert utok.read_oauth_token(force=True) == ["cli", "desk"]   # CLI zuerst

        def weg():
            raise FileNotFoundError("kein Claude Desktop")

        utok._read_tokens_from_disk = weg
        assert utok.read_oauth_token(force=True) == ["cli"]           # Desktop fehlt -> egal

        utok._read_tokens_from_cli = weg
        try:
            utok.read_oauth_token(force=True)
            raise AssertionError("ohne jede Quelle muss NoTokenError fliegen")
        except utok.NoTokenError as e:
            assert "CLI" in str(e) and "Desktop" in str(e), str(e)
    finally:
        utok._read_tokens_from_cli, utok._read_tokens_from_disk = cli, desk
        utok._token_cache = ruhe
