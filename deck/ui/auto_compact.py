"""Automatisch /compact schicken, kurz bevor der Prompt-Cache eines Agenten ablaeuft.

Ob es soweit ist, entscheidet domain/prompt_cache (compact_due, user_busy). Hier steht
nur, woher die Zahlen kommen und wie gesendet wird: die Ablaufzeit und die Kontext-
groesse schreibt die Statuszeile in state/<slot>.live.json, den Status die Hooks.

Eigenes Objekt statt Mixin (CLAUDE.md, "Mixin oder eigenes Objekt?"): es braucht vom
Panel nur die Einstellungen, die Hosts und das Senden und laesst sich so mit Attrappen
pruefen, ohne Tk und ohne echte Fenster.

Je Cache-Stand wird hoechstens einmal gesendet. Klappt das Compact, verschwindet die
Ablaufzeit aus live.json (statusline._scan); klappt es nicht, bleibt sie stehen, und
dann soll das Deck nicht im Sekundentakt nachlegen.
"""
from collections.abc import Callable, Iterable, Mapping
from typing import Any

from deck.domain import prompt_cache as pc
from deck.domain import wt
from deck.ops import log
from deck.platform import focus as wf

SETTING = "auto_compact"     # deck_settings.json, Standard an
LIVE_STALE_S = 60            # live.json aelter: die Statuszeile laeuft nicht mehr, Claude ist zu


def enabled(settings: Mapping[str, Any]) -> bool:
    return bool(settings.get(SETTING, True))


class AutoCompactor:
    def __init__(self, settings: Mapping[str, Any], hosts: Any,
                 send_text: Callable[[str, str], bool], *,
                 front: Callable[[str], bool] | None = None,
                 input_idle: Callable[[], float] | None = None) -> None:
        self.settings = settings
        self.hosts = hosts                # ui/hosts.HostRouter: connected, remote, is_wt
        self.send_text = send_text
        self.front = front or self._front
        self.input_idle = input_idle or wf.input_idle_s
        self._sent: dict[str, float] = {}   # Slot -> Ablaufzeit, fuer die schon gesendet wurde

    def step(self, slots: Iterable[str], states: Mapping[str, Any],
             live: Mapping[str, Any], now: float) -> list[str]:
        """Einmal je Poll: die Agenten kompaktieren, bei denen es soweit ist. Gibt die
        Slots zurueck, an die /compact ging."""
        if not enabled(self.settings):
            return []
        sent = []
        for slot in slots:
            try:                          # laeuft in der Poll-Schleife: nie hochwerfen
                if self._maybe(slot, states.get(slot) or {}, live.get(slot) or {}, now):
                    sent.append(slot)
            except Exception:
                log.exc(f"auto_compact {slot}")
        return sent

    def _maybe(self, slot: str, st: Mapping[str, Any], lv: Mapping[str, Any],
               now: float) -> bool:
        until = lv.get("cache_until")
        if self._sent.get(slot) == until:
            return False
        ts = lv.get("ts")
        if not isinstance(ts, (int, float)) or now - ts > LIVE_STALE_S:
            return False
        if not pc.compact_due(st.get("status"), until, lv.get("cache_ttl"),
                              lv.get("ctx_tokens"), now):
            return False
        win = slot[:1]
        if self.hosts.remote(win) or not self.hosts.connected(win):
            return False
        if pc.user_busy(self.front(slot), self.input_idle()):
            return False
        if not self.send_text(slot, "/compact"):
            return False
        self._sent[slot] = until
        log.note(f"auto-compact {slot}: Cache laeuft in {int(until - now)} s ab, "
                 f"Kontext {int(lv.get('ctx_tokens') or 0)} Tokens")
        return True

    def _front(self, slot: str) -> bool:
        """Liegt das Fenster dieses Agenten vorn? In Windows Terminal hat jeder Agent
        ein eigenes Fenster mit dem Slot als Titel. Bei VS Code zaehlt jedes VS-Code-
        Fenster: welches Fenster welchen Buchstaben traegt, sagt der Titel nicht, und
        lieber einmal zu vorsichtig als dir in die Eingabe zu tippen."""
        try:
            fg = wf.foreground_hwnd()
            title = wf.title_of(fg) if fg else ""
        except Exception:
            return True
        if self.hosts.is_wt(slot[:1]):
            return wt.slot_from_title(title) == slot
        return "Visual Studio Code" in title
