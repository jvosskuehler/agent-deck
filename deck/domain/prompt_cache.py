"""Wie lange der Prompt-Cache einer Session noch warm ist: eine Regel, zwei Anzeigen.

Die Statuszeile im Terminal und die Kachel im Deck zeigen dasselbe (`42 min`, knapp,
kalt). Beide fragen hier, damit Schwelle und Wortlaut nicht auseinanderlaufen. Die
Farben bleiben bei den Anzeigen: ANSI im Terminal, Theme-Farben auf der Kachel.

Warum es zaehlt: Laeuft der Cache ab, schreibt die naechste Nachricht den ganzen
Gespraechsanfang neu in den Cache, statt ihn guenstig zu lesen. Deshalb kompaktiert
das Deck einen wartenden Agenten kurz vor dem Ablauf (compact_due, ui/auto_compact).
"""

TTL_S = {"5m": 300, "1h": 3600}

WARM = "warm"
LOW = "low"      # knapp: gleich weiterschreiben, sonst wird es teuer
COLD = "cold"


def ttl_seconds(value: object) -> int | None:
    """'1h' / '5m' (so schreiben API und Claude Code die Laufzeit) -> Sekunden."""
    if isinstance(value, (int, float)) and not isinstance(value, bool) and value > 0:
        return int(value)
    return TTL_S.get(value) if isinstance(value, str) else None


def view(left_s: float, ttl_s: float, cold_word: str = "kalt") -> tuple[str, str]:
    """(Text, Stufe) fuer die restliche Laufzeit in Sekunden.

    Knapp ist es ab 5 Minuten vor Ablauf, beim 5-Minuten-Cache erst in der letzten
    Minute (sonst stuende er fast die ganze Zeit auf knapp). Abgerundet wird nach unten:
    `1 min` heisst mindestens eine Minute, nie weniger."""
    if left_s <= 0:
        return cold_word, COLD
    mins = int(left_s // 60)
    text = f"{mins} min" if mins else "<1 min"
    return text, (LOW if left_s < min(300, ttl_s / 5) else WARM)


# ── Automatisch kompaktieren, kurz bevor der Cache ablaeuft ─────────────────
# Grob in API-Preisen bei 200k Kontext: kommt die naechste Nachricht nach dem Ablauf,
# schreibt sie 200k neu in den 1-h-Cache (Faktor 2, also ~400k Input-Tokens). Ein
# Compact davor liest sie guenstig (Faktor 0,1), schreibt eine Zusammenfassung und
# laesst einen kurzen Stand zurueck: zusammen ~100k bis 150k. Kommt niemand zurueck,
# war es umsonst (~50k). Darum nur bei grossem Kontext und nur beim 1-h-Cache.
COMPACT_BEFORE_S = 300        # so viel Restzeit -> /compact
COMPACT_MIN_LEFT_S = 30       # knapper: das Compact laese womoeglich schon kalt
COMPACT_MIN_TOKENS = 60_000   # darunter spart es kaum etwas und kostet nur Details
TYPING_GRACE_S = 120          # so lange nach deiner letzten Eingabe sitzt du noch dran
COMPACT_READY = ("done", "idle")   # Agent ist fertig und wartet auf dich


def compact_due(status: object, until: object, ttl: object, ctx_tokens: object,
                now: float) -> bool:
    """Ist jetzt der Moment, diesen Agenten zu kompaktieren?

    `status` ist der ROHE Hook-Status, nicht der fuer die Anzeige geglaettete: ein
    'denkt' ohne frische Meldung zeigt die Kachel als idle, der Agent kann aber in
    einem langen Tool-Aufruf stecken. Dann landete /compact als Zwischennachricht.
    Eine offene Rueckfrage ('waiting') ist ebenso tabu: der Text ginge in die Antwort."""
    if status not in COMPACT_READY:
        return False
    if ttl != TTL_S["1h"] or not isinstance(until, (int, float)) or isinstance(until, bool):
        return False
    if not COMPACT_MIN_LEFT_S < until - now <= COMPACT_BEFORE_S:
        return False
    return isinstance(ctx_tokens, (int, float)) and ctx_tokens >= COMPACT_MIN_TOKENS


WORKING = ("thinking", "running")
WORKING_STALE_S = 900   # wie config.STALE_S: so lange ohne Meldung gilt "denkt" als eingeschlafen


def working(st: object, now: float) -> bool:
    """Arbeitet der Agent gerade? Dann zeigen Statuszeile und Kachel keinen Timer: jede
    Anfrage setzt ihn ohnehin neu, und auch nach einem langen Tool-Aufruf geht die
    naechste sofort raus. Der Timer zaehlt erst, wenn der Agent auf dich wartet.
    Ein 'denkt' ohne frische Meldung (z. B. mit Esc abgebrochen, dann feuert kein
    Stop-Hook) gilt nach WORKING_STALE_S nicht mehr, sonst bliebe der Timer fuer immer weg."""
    if not isinstance(st, dict) or st.get("status") not in WORKING:
        return False
    ts = st.get("ts")
    return isinstance(ts, (int, float)) and now - ts <= WORKING_STALE_S


def user_busy(front: bool, input_idle_s: float) -> bool:
    """Sitzt du gerade an genau diesem Agenten? Dann kein /compact: in VS Code holt das
    Senden den Terminal-Tab nach vorn, und was du tippst, mischte sich in den Befehl.
    Das Deck versucht es im naechsten Takt wieder, solange Restzeit bleibt."""
    return front and input_idle_s < TYPING_GRACE_S
