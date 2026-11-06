"""Agent-Deck statusLine: liefert Claude Code eine Statuszeile UND schreibt die
Live-Werte (Modell, Effort, Kontext-%, Nachricht-Tokens, Kosten) fuer das Deck
in state/<slot>.live.json.

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
from typing import Any

from deck.claude.hooks import resolve  # Slot-Aufloesung + State-Ordner (gemeinsamer Hook-Leaf)
from deck.domain import paths


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
    return {
        "model": model,
        "effort": effort,
        "ctx_pct": _num(cw.get("used_percentage")),
        "msg_tokens": _msg_tokens(cw),
        "cost_usd": _num(cost.get("total_cost_usd")),
        "session_name": data.get("session_name") if isinstance(data.get("session_name"), str) else None,
        "ts": time.time(),
    }


SEP = "  ·  "
TITLE_MAX = 70


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


def _header(label: str, title: Any) -> str:
    """Die Kopfzeile des Chats: 'B2 · ORPH-4116 · Ueberschrift', fett.

    Gewuenscht war sie OBEN ueber jedem Chat. Das geht heute nirgends: Claude Code hat
    oben keinen Platz fuer eigene Zeilen (die Statuszeile sitzt immer unten), und
    Windows Terminal zeigt den Titel nur fuer das aktive Pane im Tab (Kopfzeilen je
    Pane sind dort in Arbeit, PR #20068, Stand 2026-11). Die Statuszeile steht dagegen
    in JEDEM Pane, also bekommt sie eine eigene erste Zeile. Die Ueberschrift ist
    `session_name`: der Titel, den Claude aus dem Gespraech erzeugt, oder /rename."""
    t = " ".join(str(title or "").split())
    if len(t) > TITLE_MAX:
        t = t[:TITLE_MAX - 1] + "…"
    text = " · ".join(p for p in (label, t) if p)
    return f"\x1b[1m{text}\x1b[0m" if text else ""


def _line(rec: dict[str, Any], label: str = "") -> str:
    """Statuszeile fuers Terminal: erst die Kopfzeile (Slot, Ticket, Ueberschrift),
    darunter die Zahlen. Ohne Slot und ohne Ueberschrift nur die Zahlenzeile."""
    head = _header(label, rec.get("session_name"))
    parts = []
    if rec.get("model"):
        parts.append(str(rec["model"]))
    if rec.get("effort"):
        parts.append(f"effort {rec['effort']}")
    if rec.get("ctx_pct") is not None:
        parts.append(f"ctx {round(rec['ctx_pct'])}%")
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
