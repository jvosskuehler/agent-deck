"""report-Hook: letzte User-Frage aus dem stdin, und welche Notification eine
echte Rueckfrage ist (der Hook feuert fuer acht Faelle, nur drei zaehlen).
"""

import helpers  # noqa: F401 - Import MIT Absicht: legt die Repo-Wurzel auf den

# sys.path und nagelt die Deck-Sprache auf Deutsch.
from deck.claude.hooks import report


def test_prompt_of():
    assert report._prompt_of({"prompt": "  Hallo Welt  "}) == "Hallo Welt"   # getrimmt
    assert report._prompt_of({"prompt": "   "}) is None                      # nur Whitespace
    assert report._prompt_of({}) is None                                     # kein Feld
    assert report._prompt_of({"prompt": 123}) is None                        # kein String
    long = "x" * 600
    got = report._prompt_of({"prompt": long})
    assert got.endswith("…") and len(got) == 501                             # gekuerzt auf 500 + …


# ── report: Notification -> echte Rueckfrage vs. blosser Leerlauf ─────────
def test_is_real_query():
    # notification_type ist das verlaessliche Kriterium.
    assert report._is_real_query({"notification_type": "permission_prompt"}) is True
    assert report._is_real_query({"notification_type": "elicitation_dialog"}) is True
    assert report._is_real_query({"notification_type": "agent_needs_input"}) is True
    assert report._is_real_query({"notification_type": "idle_prompt"}) is False   # nur Leerlauf
    assert report._is_real_query({"notification_type": "agent_completed"}) is False
    assert report._is_real_query({"notification_type": "auth_success"}) is False
    # Fallback ohne notification_type (aeltere Version): Permission-Meldung erkennen.
    assert report._is_real_query({"message": "Bash: Allow this command?"}) is True
    assert report._is_real_query({"message": "Claude needs your permission to use Bash"}) is True
    assert report._is_real_query({"message": "Claude is waiting for your input"}) is False
    assert report._is_real_query({}) is False                                    # nichts -> keine Rueckfrage


def test_statuszeile_beginnt_mit_slot_und_ticket():
    """Nebeneinander zeigt Windows Terminal nur den Titel des aktiven Panes - die
    Statuszeile steht in jedem, also gehoeren Slot und Ticket dorthin."""
    import os
    import tempfile
    from deck.claude.hooks import statusline as sl

    rec = {"model": "opus", "effort": "xhigh", "ctx_pct": 41.2, "cost_usd": 1.5,
           "session_name": "Schrift-Einstellung im Client"}
    head, body = sl._line(rec, "B2 · ORPH-4116").split("\n")
    assert "B2 · ORPH-4116 · Schrift-Einstellung im Client" in head
    assert head.startswith("\x1b[1m")                    # fett abgesetzt
    assert body.startswith("opus")
    # Ohne Slot und ohne Ueberschrift bleibt es bei der einen Zahlenzeile.
    assert sl._line({"model": "opus"}) == "opus"
    # Lange Ueberschriften werden gekuerzt, damit die Zeile im schmalen Pane bleibt.
    lang = sl._line({"session_name": "x" * 200}, "B1")
    assert len(lang) < 100 and "…" in lang
    base = tempfile.mkdtemp(prefix="sl_")
    with open(os.path.join(base, "Z9.ticket"), "w", encoding="utf-8") as fh:
        fh.write("ORPH-1\n")
    assert sl._label("Z9", base) == "Z9 · ORPH-1"    # vom Agenten gemeldet
    assert sl._label("Z8", base) == "Z8"             # ohne Ticket nur der Slot
    assert sl._label(None, base) == ""


def test_statuszeile_kommt_als_utf8_an_auch_mit_umlauten():
    """Claude Code liest die Statuszeile als UTF-8. Ueber sys.stdout schrieb Python
    unter Windows cp1252 - aus "für" wurde "f◆r". Echt als Prozess geprueft, weil genau
    die Kodierung der Pipe der Fehler war."""
    import json
    import os
    import subprocess
    import sys

    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    data = json.dumps({"model": {"display_name": "Opus"},
                       "session_name": "Eigenes Terminal für Claude Code"})
    import tempfile

    env = {k: v for k, v in os.environ.items()
           if k not in ("PYTHONIOENCODING", "PYTHONUTF8", "AGENT_SLOT")}
    # Zustandsordner umlenken: ueber die Prozesskette faende der Hook sonst den Slot
    # der Session, in der die Tests laufen, und ueberschriebe deren echte Live-Datei.
    env["LOCALAPPDATA"] = tempfile.mkdtemp(prefix="sl_state_")
    out = subprocess.run([sys.executable, os.path.join(root, "statusline.py")],
                         input=data.encode("utf-8"), capture_output=True, env=env,
                         cwd=root, timeout=30).stdout
    text = out.decode("utf-8")                       # wirft, wenn es kein UTF-8 ist
    assert "Eigenes Terminal für Claude Code" in text and "Opus" in text


# ── Statuszeile: wie lange der Prompt-Cache noch warm ist ─────────────────
def _zeile(**felder):
    import json
    return json.dumps(felder)


def _anfrage(ts, ttl="1h", quelle="repl_main_thread"):
    marken = [{"path": ["system", 1], "value": {"type": "ephemeral", "ttl": "1h"}},
              {"path": ["messages", 9], "value": {"type": "ephemeral", "ttl": ttl}}]
    return _zeile(type="api-request", timestamp=ts, querySource=quelle, cacheControl=marken)


def _antwort(ts, h1=0, m5=0, nebenzweig=False):
    usage = {"cache_creation": {"ephemeral_1h_input_tokens": h1,
                                "ephemeral_5m_input_tokens": m5}}
    return _zeile(type="assistant", timestamp=ts, isSidechain=nebenzweig,
                  message={"usage": usage})


def test_cache_zaehlt_ab_dem_absenden_der_letzten_anfrage():
    """Die Laufzeit beginnt, wenn die Anfrage rausgeht, nicht wenn die Antwort da ist.
    Die kuerzeste cache_control-Marke zaehlt: mit ihr laeuft das Gespraech ab."""
    from deck.claude.hooks import statusline as sl
    t0 = sl._epoch("2026-11-08T22:00:00Z")
    zeilen = [_anfrage("2026-11-08T21:00:00Z"), _antwort("2026-11-08T21:00:30Z", h1=5),
              _anfrage("2026-11-08T22:00:00Z"), _antwort("2026-11-08T22:01:00Z", h1=5),
              _anfrage("2026-11-08T22:05:00Z", quelle="sdk"),        # fremde Anfrage
              _antwort("2026-11-08T22:06:00Z", h1=5, nebenzweig=True)]  # Subagent
    assert sl._scan(zeilen) == (t0, 3600)
    assert sl._scan([_anfrage("2026-11-08T22:00:00Z", ttl="5m")]) == (t0, 300)


def test_ohne_mitgeschriebene_anfrage_gilt_die_letzte_antwort():
    from deck.claude.hooks import statusline as sl
    t = sl._epoch("2026-11-08T22:01:00Z")
    assert sl._scan([_antwort("2026-11-08T22:01:00Z", m5=7)]) == (t, 300)
    assert sl._scan([_antwort("2026-11-08T22:01:00Z", h1=7)]) == (t, 3600)
    assert sl._scan([_antwort("2026-11-08T22:01:00Z")]) is None   # nichts geschrieben
    assert sl._scan(["{kaputt", "", "null"]) is None


def test_cache_wird_auch_hinter_einem_riesigen_tool_ergebnis_gefunden():
    """Gelesen wird nur das Ende des Transcripts; ein Tool-Ergebnis groesser als der
    erste Happen darf die letzte Anfrage nicht verstecken."""
    import os
    import tempfile
    from deck.claude.hooks import statusline as sl
    pfad = os.path.join(tempfile.mkdtemp(prefix="sl_cache_"), "t.jsonl")
    riesig = _zeile(type="user", toolUseResult="x" * (sl.TAIL_STEPS[0] * 2))
    with open(pfad, "w", encoding="utf-8") as fh:
        fh.write("\n".join([_anfrage("2026-11-08T22:00:00Z"), riesig]) + "\n")
    bis, ttl = sl._cache_from_transcript(pfad)
    assert (bis, ttl) == (sl._epoch("2026-11-08T22:00:00Z") + 3600, 3600)
    assert sl._cache_from_transcript(pfad + ".fehlt") is None
    assert sl._cache_from_transcript(None) is None
    # Ohne prompt_cache im stdin (frisch fortgesetzte Session) gilt das Transcript.
    assert sl._cache_until({"transcript_path": pfad}) == (bis, ttl)


def test_prompt_cache_aus_dem_stdin_geht_vor_dem_transcript():
    """Claude Code fuehrt selbst Buch (prompt_cache). Das Transcript ist nur der
    Rueckfall, solange der Prozess noch keine Anfrage geschickt hat."""
    from deck.claude.hooks import statusline as sl
    daten = {"prompt_cache": {"warm": True, "ttl": "1h", "expires_at": 1794182611},
             "transcript_path": "gibt-es-nicht.jsonl"}
    assert sl._cache_until(daten) == (1794182611.0, 3600)
    kalt = {"prompt_cache": {"warm": False, "ttl": "5m", "expires_at": None}}
    assert sl._cache_until(kalt) == (0.0, 300)
    assert sl._cache_until({"prompt_cache": {"ttl": "1h"}}) is None   # ohne Zeit, ohne Transcript
    assert sl._cache_until({}) is None


def test_cache_anzeige_zaehlt_bis_zum_naechsten_takt_und_warnt_wenn_es_knapp_wird():
    from deck.claude.hooks import statusline as sl
    jetzt = 1_000_000.0
    takt = sl.REFRESH_S
    assert sl._cache_part(jetzt + 42 * 60 + takt + 5, 3600, jetzt) == "cache 42 min"
    knapp = sl._cache_part(jetzt + 4 * 60 + takt, 3600, jetzt)
    assert "cache 4 min" in knapp and knapp.startswith("\x1b[33m")       # gelb
    assert "cache <1 min" in sl._cache_part(jetzt + 30 + takt, 3600, jetzt)
    # Bis zum naechsten Takt laeuft er ab: lieber jetzt schon "kalt" als zu viel versprochen.
    kalt = sl._cache_part(jetzt + takt - 1, 3600, jetzt)
    assert "cache kalt" in kalt and kalt.startswith("\x1b[2m")           # gedimmt
    # Beim 5-Minuten-Cache wird erst in der letzten Minute gewarnt.
    assert sl._cache_part(jetzt + 3 * 60 + takt, 300, jetzt) == "cache 3 min"
    rec = {"model": "opus", "cache_until": jetzt + 600 + takt, "cache_ttl": 3600, "ts": jetzt}
    assert sl._line(rec) == "opus  ·  cache 10 min"


def _compact(ts):
    return _zeile(type="system", subtype="compact_boundary", timestamp=ts, isSidechain=False)


def test_nach_einem_compact_gibt_es_keinen_timer_bis_zur_naechsten_anfrage():
    """Nach /compact ist der alte Cache nichts mehr wert: die naechste Nachricht schreibt
    den kurzen neuen Stand ohnehin neu. Gesehen 2026-11-09: direkt nach /compact stand
    "1 min" da, weil die Compact-Anfrage nicht als api-request im Transcript steht."""
    import os
    import tempfile
    from deck.claude.hooks import statusline as sl
    vorher = [_anfrage("2026-11-08T22:06:00Z"), _antwort("2026-11-08T22:06:10Z", h1=5)]
    assert sl._scan(vorher + [_compact("2026-11-08T23:03:47Z")]) == sl.COMPACTED
    # Mit deiner naechsten Nachricht geht eine Anfrage raus: der Timer ist wieder da.
    neu = sl._epoch("2026-11-08T23:05:50Z")
    danach = vorher + [_compact("2026-11-08T23:03:47Z"), _anfrage("2026-11-08T23:05:50Z")]
    assert sl._scan(danach) == (neu, 3600)
    # Der Compact schlaegt auch Claude Codes eigene Buchfuehrung im stdin.
    pfad = os.path.join(tempfile.mkdtemp(prefix="sl_compact_"), "t.jsonl")
    with open(pfad, "w", encoding="utf-8") as fh:
        fh.write("\n".join(vorher + [_compact("2026-11-08T23:03:47Z")]) + "\n")
    daten = {"prompt_cache": {"warm": True, "ttl": "1h", "expires_at": 1794182611},
             "transcript_path": pfad}
    assert sl._cache_until(daten) is None
    rec = sl._extract(daten)
    assert rec["cache_until"] is None and "cache" not in sl._line(rec)


def test_kontextgroesse_in_tokens_fuer_das_auto_compact():
    """Prozent taugen nicht als Schwelle: das Fenster ist 200k oder 1M gross."""
    from deck.claude.hooks import statusline as sl
    cw = {"current_usage": {"input_tokens": 3, "cache_creation_input_tokens": 1000,
                            "cache_read_input_tokens": 199_000, "output_tokens": 500},
          "used_percentage": 20, "context_window_size": 1_000_000}
    assert sl._ctx_tokens(cw) == 200_003                     # Ausgabe zaehlt nicht mit
    assert sl._ctx_tokens({"used_percentage": 20, "context_window_size": 1_000_000}) == 200_000
    assert sl._ctx_tokens({}) is None
    assert sl._extract({"context_window": cw})["ctx_tokens"] == 200_003


# ── Statuszeile: Kontingent und PR ────────────────────────────────────────
def test_kontingent_steht_in_der_zahlenzeile_und_faellt_hinter_dem_reset_weg():
    """Claude Code kennt nur den Stand der letzten Antwort. Nach dem Reset waere die
    Zahl eine Behauptung, also faellt sie weg (wie usage_view.expire_limits)."""
    from deck.claude.hooks import statusline as sl
    jetzt = 1_794_180_000.0
    roh = {"five_hour": {"used_percentage": 18, "resets_at": jetzt + 600},
           "seven_day": {"used_percentage": 3, "resets_at": jetzt + 86400},
           "spend_limit": {"used_percentage": 99}}            # unbekannt: nicht anzeigen
    limits = sl._extract({"rate_limits": roh})["limits"]
    assert set(limits) == {"five_hour", "seven_day"}
    assert sl._limits_part(limits, jetzt) == "5h 18% · 7d 3%"
    assert sl._limits_part(limits, jetzt + 601) == "7d 3%"      # 5h-Fenster ist um
    hoch = sl._limits({"five_hour": {"used_percentage": 63}, "seven_day": {"used_percentage": 85}})
    teil = sl._limits_part(hoch, jetzt)
    assert "\x1b[33m5h 63%" in teil and "\x1b[31m7d 85%" in teil   # gelb / rot
    assert sl._limits_part({}, jetzt) == "" and sl._limits_part(None, jetzt) == ""
    rec = {"model": "opus", "cost_usd": 1.0, "ts": jetzt, "limits": limits}
    assert sl._line(rec) == "opus  ·  5h 18% · 7d 3%  ·  $1.00"


def test_pr_steht_in_der_kopfzeile_und_kuerzt_die_ueberschrift():
    from deck.claude.hooks import statusline as sl
    pr = sl._extract({"pr": {"number": 347, "url": "u", "review_state": "approved",
                             "kind": "x"}})["pr"]
    assert pr == {"number": 347, "url": "u", "review_state": "approved"}
    head = sl._header("B2 · ORPH-4116", "Schrift", pr)
    assert head.startswith("\x1b[1mB2 · ORPH-4116 · PR #347 \x1b[32mfreigegeben\x1b[39m · Schrift")
    assert head.count("\x1b[0m") == 1                       # fett bleibt bis zum Ende
    lang_ohne = sl._header("B2", "x" * 200)
    lang_mit = sl._header("B2", "x" * 200, {"number": 1, "review_state": "changes_requested"})
    sichtbar = lambda s: len(s.replace("\x1b[33m", "").replace("\x1b[39m", ""))
    assert sichtbar(lang_mit) <= sichtbar(lang_ohne)        # Zeile wird nicht laenger
    assert "Änderungen gewünscht" in lang_mit
    assert "PR #5 offen" in sl._header("", "", {"number": 5, "review_state": "pending"})
    assert sl._header("", "", {"number": 5}) == "\x1b[1mPR #5\x1b[0m"
    assert sl._extract({"pr": {"number": "347"}})["pr"] is None   # keine Zahl -> kein PR
