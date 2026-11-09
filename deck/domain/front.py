"""Alle nach vorn - und mit demselben Druck wieder zurück.

Die Taste (config.FRONT_HOTKEYS) holt jedes Agentenfenster in Windows Terminal nach vorn
und legt sie als Raster auf den Monitor unter der Maus (ops/wt_tiles.gather). Hier steht
die Rechnung dahinter, ohne Fenster:

- WER den Fokus bekommt: der Agent, der am längsten wartet (dieselbe Warteschlange wie
  die Sprung-Taste, domain/attention.py), sonst der zuletzt gewählte, sonst der erste.
  Anders als die Sprung-Taste geht es hier nicht reihum: wer alle holt, will beim
  dringendsten landen, egal wo er vorher stand.
- WOHIN ein zweiter Druck führt (BackToggle): zurück in das Fenster, aus dem man kam.
  So wird aus der Taste ein Blick hinüber und wieder zurück, wie Win+D für den Desktop.
  Die Agenten bleiben dabei liegen, statt minimiert zu werden: minimierte Fenster lässt
  das automatische Kacheln aussen vor (ops/wt_tiles.tile).
"""
from collections.abc import Callable, Iterable, Mapping
from typing import Any

from deck.domain import attention


def combos(value: Any) -> list[str]:
    """Die Einstellung "front_hotkeys" als Liste: ein Text ist eine Kombination, eine
    Liste mehrere; Leeres und Doppeltes fällt weg. Leere Liste = Taste aus."""
    items = [value] if isinstance(value, str) else list(value or [])
    out: list[str] = []
    for item in items:
        text = str(item or "").strip()
        if text and text.lower() not in (o.lower() for o in out):
            out.append(text)
    return out


def focus_target(states: Mapping[str, Mapping[str, Any]], shown: Iterable[str],
                 active: str | None = None) -> str | None:
    """Wer nach dem Holen den Fokus bekommt: der am längsten Wartende, sonst der
    zuletzt gewählte (sofern er unter den geholten ist), sonst der erste. None, wenn
    es niemanden zu holen gibt."""
    shown = list(shown)
    waiting = attention.queue(states, shown)
    if waiting:
        return waiting[0]
    if active in shown:
        return active
    return shown[0] if shown else None


class BackToggle:
    """Merkt sich das Fenster, aus dem die Taste gedrückt wurde.

    `press` bekommt das Vordergrundfenster (0 = keins oder eines des Decks selbst, an
    das niemand zurück will) und ob es ein Agent ist. Antwort: das Fenster, in das
    zurückgesprungen werden soll, oder None = alle nach vorn holen.

    Zurück geht es nur, wenn man gerade bei einem Agenten steht UND das gemerkte
    Fenster noch lebt. Steht man woanders, ist das der neue Ausgangspunkt: man hat
    die Agenten inzwischen selbst verlassen, ein Druck holt sie also wieder."""

    def __init__(self) -> None:
        self.back_to = 0

    def press(self, fg: int, on_agent: bool, alive: Callable[[int], bool]) -> int | None:
        if on_agent:
            back, self.back_to = self.back_to, 0
            return back if back and alive(back) else None
        self.back_to = fg or 0
        return None
