"""Agent-Deck statusLine: liefert Claude Code eine Statuszeile UND schreibt die
Live-Werte (Modell, Effort, Kontext-%, Nachricht-Tokens, Kosten, Ablauf des
Prompt-Caches) fuer das Deck in state/<slot>.live.json.

Konfiguration in ~/.claude/settings.json:
  "statusLine": { "type": "command",
                  "command": "python \"C:\\...\\agent-deck\\statusline.py\"" }

Claude Code ruft dieses Kommando bei jeder Assistant-Message / /compact /
Permission-Mode-Wechsel auf und uebergibt ein JSON per stdin. Das Skript laeuft
als Kind der Claude-Session -> Slot-Aufloesung ueber hookstate (AGENT_SLOT oder
pidmap ueber die Prozesskette), genau wie der report-Hook. Gibt IMMER eine Zeile
aus (auch ohne Slot), damit die Statuszeile im Terminal nicht leer ist. Darf nie
mit Fehler enden.
"""
import json
import os
import sys
import time
from datetime import datetime
from typing import Any

from deck.claude.hooks import resolve  # Slot-Aufloesung + State-Ordner (gemeinsamer Hook-Leaf)
from deck.domain import paths
from deck.domain import prompt_cache as pc

# Claude Code ruft die Statuszeile zusaetzlich alle REFRESH_S Sekunden auf
# ("refreshInterval", schreibt hook_setup). Ohne den Takt stuende der Cache-Countdown
# genau dann still, wenn er zaehlt: wenn im Chat nichts passiert.
REFRESH_S = 15
# So viel vom Transcript-Ende wird gelesen. Die letzte Anfrage steht fast immer in den
# ersten 64 KB; nur wenn grosse Tool-Ergebnisse dazwischen liegen, wird nachgelegt.
TAIL_STEPS = (64 << 10, 512 << 10, 4 << 20)


def _num(*vals: Any) -> float | None:
    for v in vals:
        if isinstance(v, (int, float)):
            return v
    return None


def _msg_tokens(cw: Any) -> float | None:
    """Tokens der aktuellen Nachricht (best effort ueber wechselnde Feldnamen)."""
    cur = cw.get("current_usage") or {}
    i = _num(cur.get("input_tokens"), cur.get("input"))
    o = _num(cur.get("output_tokens"), cur.get("output"))
    if i is not None or o is not None:
        return (i or 0) + (o or 0)
    return _num(cw.get("total_output_tokens"))


def _ctx_tokens(cw: Any) -> float | None:
    """Wie gross der Kontext gerade ist, in Tokens: alles, was die letzte Anfrage
    hineingegeben hat (frisch, neu gecacht, aus dem Cache gelesen). Das Deck
    kompaktiert erst ab einer Mindestgroesse (domain/prompt_cache); Prozent taugen
    dafuer nicht, weil das Fenster 200k oder 1M gross sein kann."""
    cur = cw.get("current_usage") or {}
    vals = [_num(cur.get(k)) for k in ("input_tokens", "cache_creation_input_tokens",
                                       "cache_read_input_tokens")]
    if any(v is not None for v in vals):
        return sum(v or 0 for v in vals)
    pct, size = _num(cw.get("used_percentage")), _num(cw.get("context_window_size"))
    return pct * size / 100 if pct is not None and size else None


def _epoch(value: Any) -> float | None:
    try:
        return datetime.fromisoformat(str(value).replace("Z", "+00:00")).timestamp()
    except ValueError:
        return None


def _ttl_of(entry: dict[str, Any]) -> int | None:
    """Laufzeit des Prompt-Caches in Sekunden, abgelesen an einer Transcript-Zeile.

    `api-request` (die Anfrage selbst): ihre cache_control-Marken. Die kuerzeste zaehlt,
    denn mit ihr laeuft der Gespraechsteil ab, der den Grossteil des Caches ausmacht.
    `assistant` (Rueckfall, falls Claude Code die Anfragen nicht mehr mitschreibt): die
    Antwort meldet, in welche Laufzeit sie den Cache geschrieben hat."""
    if entry.get("type") == "api-request":
        marks = entry.get("cacheControl")
        if not isinstance(marks, list):
            return None
        ttls = [pc.ttl_seconds((m.get("value") or {}).get("ttl", "5m"))
                for m in marks if isinstance(m, dict)]
        return min((t for t in ttls if t), default=None)
    usage = (entry.get("message") or {}).get("usage") or {}
    made = usage.get("cache_creation") or {}
    if made.get("ephemeral_1h_input_tokens"):
        return pc.TTL_S["1h"]
    if made.get("ephemeral_5m_input_tokens"):
        return pc.TTL_S["5m"]
    return None


COMPACTED = "compacted"    # nach einem Compact: kein Cache mehr, um den es ginge


def _scan(lines: list[str]) -> tuple[float, int] | str | None:
    """(Zeitpunkt, Laufzeit) der letzten Anfrage des Hauptgespraechs, von hinten gesucht.

    Jede Anfrage, die den Cache liest, setzt seine Laufzeit neu; massgeblich ist der
    Zeitpunkt, zu dem sie RAUSGING, nicht der ihrer Antwort. Den schreibt Claude Code
    als Zeile `api-request` mit (Stand 2.1.319, nicht dokumentiert). Fehlt sie, gilt die
    letzte Antwort: die kommt etwas spaeter, die Zeit ist also etwas zu grosszuegig.

    Steht hinter der letzten Anfrage ein Compact (`compact_boundary`, von Hand, vom Deck
    oder von Claude Code bei vollem Kontext), gibt es COMPACTED zurueck: der alte
    Gespraechsanfang wird nie wieder gelesen, und die naechste Nachricht schreibt den
    kurzen neuen Stand ohnehin neu. Die Compact-Anfrage selbst steht nicht als
    `api-request` im Transcript; ohne diese Regel zaehlte die Anzeige den Cache des
    alten Gespraechs weiter herunter (gesehen 2026-11-09: "1 min" direkt nach /compact)."""
    fallback = None
    for line in reversed(lines):
        if ('"api-request"' not in line and '"assistant"' not in line
                and '"compact_boundary"' not in line):
            continue
        try:
            entry = json.loads(line)
        except ValueError:
            continue                         # auch die angeschnittene erste Zeile
        if not isinstance(entry, dict) or entry.get("isSidechain"):
            continue
        kind = entry.get("type")
        if kind == "system" and entry.get("subtype") == "compact_boundary":
            return fallback or COMPACTED     # Antwort dahinter: es lief schon wieder eine
        if kind == "api-request" and entry.get("querySource",
                                               "repl_main_thread") != "repl_main_thread":
            continue
        if kind not in ("api-request", "assistant"):
            continue
        ttl, ts = _ttl_of(entry), _epoch(entry.get("timestamp"))
        if not (ttl and ts):
            continue
        if kind == "api-request":
            return ts, ttl
        if fallback is None:
            fallback = (ts, ttl)
    return fallback


def _cache_until(data: dict[str, Any]) -> tuple[float, int] | None:
    """(Ablaufzeit, Laufzeit) des Prompt-Caches dieser Session, oder None.

    Erste Quelle ist `prompt_cache` aus dem stdin: Claude Code fuehrt selbst Buch ueber
    seine Anfragen und kennt dabei auch Fehlgriffe (Stand 2.1.319, nicht dokumentiert).
    Das Feld fehlt aber, solange die Session in DIESEM Prozess noch keine Anfrage
    geschickt hat, also auch direkt nach `claude --resume`. Der Cache auf dem Server
    kann dann noch warm sein, und genau das steht im Transcript.

    Ein Compact hinter der letzten Anfrage schlaegt beide: dann gibt es keinen Timer,
    bis die naechste Anfrage rausgeht (siehe _scan). Darum wird das Transcript immer
    gelesen, auch wenn `prompt_cache` da ist."""
    scanned = _cache_from_transcript(data.get("transcript_path"))
    if scanned == COMPACTED:
        return None
    raw = data.get("prompt_cache")
    if isinstance(raw, dict):
        ttl = pc.ttl_seconds(raw.get("ttl"))
        until = _num(raw.get("expires_at"))
        if ttl and raw.get("warm") is False:
            return 0.0, ttl
        if ttl and until:
            return float(until), ttl
    return scanned if isinstance(scanned, tuple) else None


def _cache_from_transcript(transcript: Any) -> tuple[float, int] | str | None:
    """(Ablaufzeit, Laufzeit) aus dem Ende des Transcripts, COMPACTED oder None."""
    if not isinstance(transcript, str) or not transcript:
        return None
    try:
        with open(transcript, "rb") as f:
            size = f.seek(0, os.SEEK_END)
            for step in TAIL_STEPS:
                f.seek(max(0, size - step))
                lines = f.read().decode("utf-8", "replace").splitlines()
                hit = _scan(lines)
                if hit or step >= size:
                    break
    except OSError:
        return None
    if isinstance(hit, tuple):
        return hit[0] + hit[1], hit[1]
    return hit


_ANSI = {pc.WARM: "", pc.LOW: "\x1b[33m", pc.COLD: "\x1b[2m"}   # normal / gelb / gedimmt


def _cache_part(until: float, ttl: int, now: float) -> str:
    """'cache 42 min'. Gezaehlt wird bis zum NAECHSTEN Takt der Statuszeile: so
    zeigt die Zeile nie mehr Restzeit an, als bis zu ihrer Erneuerung tatsaechlich
    bleibt. Wortlaut und Schwelle kommen aus domain/prompt_cache, dieselben wie auf
    der Kachel."""
    from deck import i18n
    text, level = pc.view(until - now - REFRESH_S, ttl, i18n.L("kalt", "cold"))
    color = _ANSI[level]
    return f"{color}cache {text}\x1b[0m" if color else f"cache {text}"


def _extract(data: dict[str, Any]) -> dict[str, Any]:
    model = None
    m = data.get("model")
    if isinstance(m, dict):
        model = m.get("display_name") or m.get("id")
    elif isinstance(m, str):
        model = m
    eff = data.get("effort")
    effort = eff.get("level") if isinstance(eff, dict) else (eff if isinstance(eff, str) else None)
    raw_cw = data.get("context_window")
    cw: dict[str, Any] = raw_cw if isinstance(raw_cw, dict) else {}
    raw_cost = data.get("cost")
    cost: dict[str, Any] = raw_cost if isinstance(raw_cost, dict) else {}
    cache = _cache_until(data)
    raw_pr = data.get("pr")
    pr = raw_pr if isinstance(raw_pr, dict) and isinstance(raw_pr.get("number"), int) else None
    return {
        "model": model,
        "effort": effort,
        "ctx_pct": _num(cw.get("used_percentage")),
        "ctx_tokens": _ctx_tokens(cw),
        "msg_tokens": _msg_tokens(cw),
        "cost_usd": _num(cost.get("total_cost_usd")),
        "session_name": data.get("session_name") if isinstance(data.get("session_name"), str) else None,
        "cache_until": cache[0] if cache else None,
        "cache_ttl": cache[1] if cache else None,
        "limits": _limits(data.get("rate_limits")),
        "pr": {k: pr.get(k) for k in ("number", "url", "review_state")} if pr else None,
        "ts": time.time(),
    }


SEP = "  ·  "
TITLE_MAX = 70

# Kontingent aus `rate_limits` im stdin: Claude Code liest es aus den Antwort-Headern
# der API mit, kostet also keinen Abruf (anders als der Usage-Endpoint des Decks, der
# eng limitiert ist). Schwellen wie im Usage-Badge des Decks: ab 50 % gelb, ab 80 % rot.
LIMITS = (("five_hour", "5h"), ("seven_day", "7d"))


def _limits(raw: Any) -> dict[str, dict[str, float | None]]:
    """{'five_hour': {'pct': 18, 'resets_at': 1794181200}, ...} - nur, was lesbar ist."""
    out: dict[str, dict[str, float | None]] = {}
    for key, _short in LIMITS:
        lim = raw.get(key) if isinstance(raw, dict) else None
        pct = _num(lim.get("used_percentage")) if isinstance(lim, dict) else None
        if pct is not None:
            out[key] = {"pct": pct, "resets_at": _num(lim.get("resets_at"))}
    return out


def _limits_part(limits: Any, now: float) -> str:
    """'5h 18% · 7d 3%'. Ein Wert hinter seinem Reset faellt weg: Claude Code kennt
    nur den Stand der letzten Antwort, und nach Stunden Leerlauf waere der laengst
    ueberholt (dieselbe Regel wie usage_view.expire_limits im Deck)."""
    parts = []
    for key, short in LIMITS:
        lim = limits.get(key) if isinstance(limits, dict) else None
        if not isinstance(lim, dict) or not isinstance(lim.get("pct"), (int, float)):
            continue
        reset = lim.get("resets_at")
        if isinstance(reset, (int, float)) and reset <= now:
            continue
        pct = lim["pct"]
        text = f"{short} {round(pct)}%"
        color = "" if pct < 50 else ("\x1b[33m" if pct < 80 else "\x1b[31m")
        parts.append(f"{color}{text}\x1b[0m" if color else text)
    return " · ".join(parts)


# review_state, wie Claude Code ihn aus GitHub ableitet (Stand 2.1.319): draft, approved,
# changes_requested, sonst pending (offen, Review fehlt oder ist nicht verlangt).
# Gefaerbt wird nur mit 39 (Vordergrund zuruecksetzen) statt 0: die Kopfzeile ist fett,
# ein volles Zuruecksetzen wuerde den Rest der Zeile duenn machen.
_PR_STATE = {
    "approved": (("freigegeben", "approved"), "\x1b[32m"),
    "changes_requested": (("Änderungen gewünscht", "changes requested"), "\x1b[33m"),
    "pending": (("offen", "open"), ""),
    "draft": (("Entwurf", "draft"), ""),
}


def _pr_part(pr: Any) -> tuple[str, str]:
    """('PR #347 freigegeben', dieselbe Fassung mit Farbe). Leer ohne PR."""
    if not isinstance(pr, dict) or not isinstance(pr.get("number"), int):
        return "", ""
    words, color = _PR_STATE.get(pr.get("review_state"), (None, ""))
    if words:
        from deck import i18n
        state = i18n.L(*words)
        return (f"PR #{pr['number']} {state}",
                f"PR #{pr['number']} {color}{state}\x1b[39m" if color
                else f"PR #{pr['number']} {state}")
    return f"PR #{pr['number']}", f"PR #{pr['number']}"


def _label(slot: str | None, base: str) -> str:
    """'B2 · ORPH-4116': welche Kachel dieses Pane ist und woran es arbeitet. Ticket: im
    Deck zugewiesen (tickets.json) oder vom Agenten selbst gemeldet (state/<slot>.ticket)."""
    if not slot:
        return ""
    ticket = (paths.load_json(os.path.join(paths.REPO_ROOT, "tickets.json"), {})
              or {}).get(slot) or ""
    if not ticket:
        try:
            with open(os.path.join(base, slot + ".ticket"), encoding="utf-8") as f:
                ticket = (f.read().strip().splitlines() or [""])[0][:40]
        except OSError:
            ticket = ""
    return f"{slot} · {ticket}" if ticket else slot


def _header(label: str, title: Any, pr: Any = None) -> str:
    """Die Kopfzeile des Chats: 'B2 · ORPH-4116 · PR #347 freigegeben · Ueberschrift',
    fett. Der PR steht vor der Ueberschrift und kuerzt sie: die Zeile soll nicht
    laenger werden als ohne PR, sonst bricht sie im schmalen Pane um.

    Gewuenscht war sie OBEN ueber jedem Chat. Das geht heute nirgends: Claude Code hat
    oben keinen Platz fuer eigene Zeilen (die Statuszeile sitzt immer unten), und
    Windows Terminal zeigt den Titel nur fuer das aktive Pane im Tab (Kopfzeilen je
    Pane sind dort in Arbeit, PR #20068, Stand 2026-11). Die Statuszeile steht dagegen
    in JEDEM Pane, also bekommt sie eine eigene erste Zeile. Die Ueberschrift ist
    `session_name`: der Titel, den Claude aus dem Gespraech erzeugt, oder /rename."""
    pr_plain, pr_text = _pr_part(pr)
    budget = TITLE_MAX - (len(pr_plain) + 3 if pr_plain else 0)
    t = " ".join(str(title or "").split())
    if len(t) > budget:
        t = t[:max(0, budget - 1)] + "…"
    text = " · ".join(p for p in (label, pr_text, t) if p)
    return f"\x1b[1m{text}\x1b[0m" if text else ""


def _line(rec: dict[str, Any], label: str = "") -> str:
    """Statuszeile fuers Terminal: erst die Kopfzeile (Slot, Ticket, PR, Ueberschrift),
    darunter die Zahlen. Ohne Slot, PR und Ueberschrift nur die Zahlenzeile."""
    now = rec.get("ts") or time.time()
    head = _header(label, rec.get("session_name"), rec.get("pr"))
    parts = []
    if rec.get("model"):
        parts.append(str(rec["model"]))
    if rec.get("effort"):
        parts.append(f"effort {rec['effort']}")
    if rec.get("ctx_pct") is not None:
        parts.append(f"ctx {round(rec['ctx_pct'])}%")
    if rec.get("cache_until") and rec.get("cache_ttl"):
        parts.append(_cache_part(rec["cache_until"], rec["cache_ttl"], now))
    limits = _limits_part(rec.get("limits"), now)
    if limits:
        parts.append(limits)
    if rec.get("cost_usd") is not None:
        parts.append(f"${rec['cost_usd']:.2f}")
    body = SEP.join(parts)
    return f"{head}\n{body}" if head else body


def main() -> None:
    try:
        # Rohe Bytes selbst als UTF-8 dekodieren (siehe report._read_stdin_json):
        # sys.stdin.read() nimmt auf Windows die ANSI-Codepage und zerstoert Umlaute.
        if sys.stdin is None or sys.stdin.isatty():
            raw = ""
        else:
            raw = sys.stdin.buffer.read().decode("utf-8", "replace")
        data = json.loads(raw) if raw.strip() else {}
    except Exception:
        data = {}

    rec = _extract(data)

    label = ""
    try:
        base = resolve.state_dir()
        slot = os.environ.get("AGENT_SLOT") or resolve.slot_from_procs(base)
        if slot:
            paths.save_json(os.path.join(base, slot + ".live.json"), rec)
            label = _label(slot, base)
            # Arbeitet der Agent, kein Timer (Regel in prompt_cache.working). Nur die
            # Anzeige: live.json behaelt die Ablaufzeit, das Deck entscheidet selbst.
            if pc.working(paths.load_json(os.path.join(base, slot + ".json"), {}),
                          time.time()):
                rec = dict(rec, cache_until=None)
    except Exception:
        pass  # Deck-State ist Beiwerk -> niemals die Statuszeile kaputt machen

    _emit(_line(rec, label))


def _emit(text: str) -> None:
    """Als UTF-8-BYTES ausgeben, nie ueber sys.stdout.write. Unter Windows schreibt
    Python in eine Pipe mit der ANSI-Codepage (cp1252), Claude Code liest aber UTF-8:
    aus "·" und "für" wurde so "◆" und "f◆r" (gesehen 2026-11-06, sobald die
    Kopfzeile Umlaute aus der Chat-Ueberschrift zeigte). Gegenstueck zur Regel fuer
    stdin (CLAUDE.md, Vertrag 5)."""
    try:
        sys.stdout.buffer.write(text.encode("utf-8"))
        sys.stdout.flush()
    except (AttributeError, OSError):
        sys.stdout.write(text)


if __name__ == "__main__":
    try:
        main()
    except Exception:
        pass
