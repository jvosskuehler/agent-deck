"""Wer braucht mich als Naechstes? - die Rechnung hinter der Sprung-Taste.

Mit fuenf, sechs Agenten nebeneinander ist nicht das Lesen der Engpass, sondern die
Frage, wer gerade auf dich wartet. Die Sprung-Taste (config.JUMP_HOTKEY) beantwortet
sie mit einem Griff: sie holt das Pane nach vorn, das am laengsten wartet.

Reihenfolge: erst Rueckfragen (`waiting` - der Agent steht, bis du antwortest), dann
fertige, noch ungelesene Antworten (`done`), innerhalb jeder Gruppe die aelteste
zuerst. Wer schon vorn ist, gibt beim naechsten Druck an den Naechsten weiter - so
geht man die Warteschlange reihum durch, statt immer wieder beim selben zu landen.
"""
from collections.abc import Iterable, Mapping
from typing import Any

PRIORITY = ("waiting", "done")


def queue(states: Mapping[str, Mapping[str, Any]], shown: Iterable[str]) -> list[str]:
    """Die wartenden Slots unter den angezeigten, in der Reihenfolge ihres Drankommens."""
    cands = []
    for slot in shown:
        st = states.get(slot) or {}
        status = st.get("status")
        if status in PRIORITY:
            try:
                ts = float(st.get("ts") or 0)
            except (TypeError, ValueError):
                ts = 0.0
            cands.append((PRIORITY.index(status), ts, slot))
    return [slot for _, _, slot in sorted(cands)]


def next_slot(states: Mapping[str, Mapping[str, Any]], shown: Iterable[str],
              current: str | None = None) -> str | None:
    """Wohin die Sprung-Taste fuehrt: der erste Wartende, oder - steht man schon bei
    einem Wartenden - der naechste nach ihm (reihum). None, wenn niemand wartet."""
    q = queue(states, shown)
    if not q:
        return None
    if current in q:
        return q[(q.index(current) + 1) % len(q)]
    return q[0]
