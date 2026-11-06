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
