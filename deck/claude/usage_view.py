"""Usage-Antwort auswerten und fuer Menschen aufbereiten - pur, headless testbar.

Parsen und Formatieren liegen zusammen, weil beide Schritte derselben Kette gehoeren und
beide ohne Netz, Token und Anzeige pruefbar sind: JSON rein, Snapshot raus - Snapshot
rein, Zeile raus.

Zahlen fuer Menschen brauchen einen festen Punkt als Dezimaltrenner; eine
locale-abhaengige Formatierung zeigt auf einem deutschen System sonst $0,15 statt $0.15.
"""
import time
from datetime import UTC, datetime
from typing import Any

from deck import i18n

# Ampelfarben (identisch zur Deck-Palette: done-gruen / waiting-amber / lost-rot),
# damit das Badge sich nahtlos einfuegt. severity kommt direkt aus der API.
_GREEN, _AMBER, _RED, _GRAY = "#6ee7a8", "#ffc48a", "#ff6b6b", "#8b8b99"
_SEVERITY_COLORS = {"normal": _GREEN, "warning": _AMBER, "critical": _RED}

# Ab diesem Datenalter nennt der Tooltip den Stand. Darunter ist Alter kein Befund:
# der gemeinsame Poller haelt seinen Cache regulaer 90 s und im 429-Backoff bis
# 10 Min. — eine Stand-Zeile bei jedem Hover waere Rauschen.
STALE_AFTER = 900


def _fmt_span(seconds: float) -> str:
    """Sekunden -> 'X Tg. Y Std.' / 'X Std. Y Min.' / 'X Min.'; die gemeinsame
    Zerlegung von fmt_reset (Zukunft) und fmt_age (Vergangenheit)."""
    d, h, m = int(seconds // 86400), int((seconds % 86400) // 3600), int((seconds % 3600) // 60)
    if d:
        return i18n.L(f"{d} Tg. {h} Std.", f"{d}d {h}h") if h else i18n.L(f"{d} Tg.", f"{d}d")
    if h:
        return i18n.L(f"{h} Std. {m} Min.", f"{h}h {m}min") if m else i18n.L(f"{h} Std.", f"{h}h")
    return i18n.L(f"{m} Min.", f"{m}min")


def _as_utc(iso: str | None) -> datetime | None:
    """ISO-Zeit -> tz-aware datetime; None bei leer/kaputt. Naive Zeiten gelten als
    UTC (die API liefert Offsets, aber der Parser soll daran nicht haengen)."""
    if not iso:
        return None
    try:
        dt = datetime.fromisoformat(iso)
    except (ValueError, TypeError):
        return None
    return dt.replace(tzinfo=UTC) if dt.tzinfo is None else dt


def fmt_reset(iso: str | None, now: Any = None) -> str:
    """ISO-Zeit -> 'X Tg. Y Std.' / 'X Std. Y Min.' / 'X Min.' relativ zu now
    (tz-aware datetime; Default = jetzt UTC). Leer/kaputt -> ''; Vergangenheit ->
    'jetzt'. now injizierbar, damit Tests nicht von der Wanduhr abhaengen."""
    dt = _as_utc(iso)
    if dt is None:
        return ""
    delta = (dt - (now or datetime.now(UTC))).total_seconds()
    if delta <= 0:
        return i18n.L("jetzt", "now")
    return _fmt_span(delta)


def fmt_age(ts: Any, now: Any = None, min_seconds: float = STALE_AFTER) -> str:
    """Unix-Sekunden -> 'vor X Std. Y Min.'. Leer, wenn kein Zeitstempel vorliegt
    oder die Daten juenger als min_seconds sind (siehe STALE_AFTER)."""
    if not isinstance(ts, (int, float)):
        return ""
    if isinstance(now, datetime):
        now_s = now.timestamp()
    elif isinstance(now, (int, float)):
        now_s = float(now)
    else:
        now_s = time.time()
    age = now_s - ts
    if age < min_seconds:
        return ""
    return i18n.L(f"vor {_fmt_span(age)}", f"{_fmt_span(age)} ago")


def severity_color(severity: str | None, percent: float | None) -> str:
    """Hex-Farbe fuers Badge. Zuerst die API-severity (normal/warning/critical);
    fehlt sie, per Schwellwert (wie der Usage-Monitor: <50 gruen, <80 amber, sonst
    rot). Ohne Wert grau."""
    if severity in _SEVERITY_COLORS:
        return _SEVERITY_COLORS[severity]
    if percent is None:
        return _GRAY
    if percent < 50:
        return _GREEN
    if percent < 80:
        return _AMBER
    return _RED


def is_stale(snap: dict[str, Any], now: Any = None) -> bool:
    """Steht im Snapshot ein Wert, der nachweislich nicht mehr frisch ist?

    Prueffaden ist allein das ALTER (ab STALE_AFTER), nicht der Fehlertext: ein
    429, der gerade eben aufgetreten ist, laesst einen 90 s alten Wert stehen —
    der ist brauchbar. Umgekehrt faellt so auch der stille Tod auf: stirbt der
    Poll-Thread weg, bleibt 'error' leer und nur der Zeitstempel altert.

    Bewusst DERSELBE Schwellwert, den fmt_age fuers Hover benutzt: das Badge wird
    genau dann matt, wenn der Tooltip den Stand nennt. Eine Regel, zwei Orte —
    sonst zeigt das eine 'frisch' und das andere 'vor 20 Min.'."""
    return bool(fmt_age(snap.get("ts"), now))


def _dim(hex_color: str, factor: float = 0.45) -> str:
    """Farbe abdunkeln (Richtung Leistenhintergrund). Kein Grau: die Ampel traegt
    Information, die soll nicht verschwinden — sie soll nur nicht mehr wie ein
    frischer Messwert leuchten."""
    try:
        r, g, b = (int(hex_color[i:i + 2], 16) for i in (1, 3, 5))
    except (ValueError, IndexError):
        return hex_color
    return "#" + "".join(f"{int(v * factor):02x}" for v in (r, g, b))


def badge_view(snap: dict[str, Any], now: Any = None) -> tuple[str, str]:
    """(Text, Farbe) fuer das Nutzungs-Badge der unteren Leiste.

    Das ist die Lehre vom 2026-08-05 einen Schritt weitergedacht. Damals wurde
    ein Wert HINTER SEINEM RESET entwertet — richtig, aber es deckt nur den Fall
    ab, in dem ein neues Fenster begonnen hat. Am 2026-08-18 stand das Badge
    stattdessen 18 Minuten lang auf gruenen '0 %', weil der gemeinsame Cache im
    429-Backoff eingefroren war; der Reset lag noch in der Zukunft, die
    Entwertung griff also nicht, und live waren es laengst 3 %. Zahl plausibel,
    Farbe beruhigend, Anzeige tot — das sieht wie ein kaputtes Programm aus.

    Darum: ein alter Wert wird weiter GEZEIGT (er ist die beste vorhandene
    Auskunft), aber matt. Warum nicht ausblenden? Weil '—' den Nutzer nichts
    fragen laesst, waehrend eine matte Zahl zum Hover einlaedt — und dort steht
    der Grund."""
    pct = snap.get("session_percent")
    color = severity_color(snap.get("session_severity"), pct)
    text = f"{pct} %" if pct is not None else "—"
    if pct is not None and is_stale(snap, now):
        return text, _dim(color)
    return text, color


def _limit_label(lim: dict[str, Any]) -> str:
    """Menschlicher (deutscher) Name eines API-Limits fuers Hover."""
    kind = (lim.get("kind") or "").lower()
    if kind == "session":
        return "Session"
    if "opus" in kind:
        return i18n.L("Opus (Woche)", "Opus (week)")
    if "sonnet" in kind:
        return i18n.L("Sonnet (Woche)", "Sonnet (week)")
    if kind == "weekly_scoped":
        scope = lim.get("scope") or {}
        model = (scope.get("model") or {}).get("display_name") if isinstance(scope, dict) else None
        return i18n.L(f"{model} (Woche)", f"{model} (week)") if model \
            else i18n.L("Woche (Modell)", "Week (model)")
    if kind.startswith("weekly"):
        return i18n.L("Woche", "Week")
    return kind.replace("_", " ").title() or "Limit"


def _pct(v: Any) -> int | None:
    return round(v) if isinstance(v, (int, float)) else None


def parse_usage(data: dict[str, Any]) -> dict[str, Any]:
    """Rohe API-Antwort -> normalisiertes Dict:
        {"session": <limit|None>, "limits": [<limit>, …]}
    Ein <limit> ist {kind, group, label, percent, severity, resets_at, active}.
    Nutzt bevorzugt das moderne 'limits'-Array; faellt sonst auf die aelteren
    Felder five_hour/seven_day zurueck."""
    limits = []
    raw = data.get("limits") if isinstance(data, dict) else None
    if isinstance(raw, list) and raw:
        for lim in raw:
            if not isinstance(lim, dict):
                continue
            limits.append({
                "kind": lim.get("kind") or "",
                "group": lim.get("group") or "",
                "label": _limit_label(lim),
                "percent": _pct(lim.get("percent")),
                "severity": lim.get("severity") or "",
                "resets_at": lim.get("resets_at"),
                "active": bool(lim.get("is_active")),
            })
    else:                                   # aeltere Antwort ohne 'limits'
        five = (data.get("five_hour") if isinstance(data, dict) else None) or {}
        seven = (data.get("seven_day") if isinstance(data, dict) else None) or {}
        if five.get("utilization") is not None:
            limits.append({"kind": "session", "group": "session", "label": "Session",
                           "percent": _pct(five["utilization"]), "severity": "",
                           "resets_at": five.get("resets_at"), "active": True})
        if seven.get("utilization") is not None:
            limits.append({"kind": "weekly_all", "group": "weekly",
                           "label": i18n.L("Woche", "Week"),
                           "percent": _pct(seven["utilization"]), "severity": "",
                           "resets_at": seven.get("resets_at"), "active": False})
    return {"session": session_of(limits), "limits": limits}


def session_of(limits: list[dict[str, Any]]) -> dict[str, Any] | None:
    """Das Session-Limit aus einer Limit-Liste (oder None). Eigene Funktion, weil
    die Auswahl nach dem Entwerten (expire_limits) noch einmal gebraucht wird."""
    return next((lim for lim in limits
                 if lim.get("group") == "session" or lim.get("kind") == "session"), None)


def expire_limits(limits: list[dict[str, Any]],
                  now: Any = None) -> list[dict[str, Any]]:
    """Limits, deren Reset-Zeitpunkt VORBEI ist, verlieren Prozentwert und Ampel
    (und werden mit stale=True markiert).

    Das ist keine Vorsicht, sondern Arithmetik: hinter dem Reset zaehlt die API ein
    neues Fenster ab 0, ein zwischengespeicherter Wert von davor beschreibt also
    nichts mehr. Ihn weiter anzuzeigen ist schlimmer als '—', denn er steht
    typischerweise zu HOCH — genau darum war der Fehler am 2026-08-05 nicht zu
    sehen: das Deck zeigte 48 % aus dem Fenster von gestern, waehrend das laufende
    bei 13 % stand. Eine Zahl, die plausibel aussieht, meldet sich nicht selbst.

    Der Reset-Zeitpunkt ist der Prueffaden, NICHT das Datenalter: ein 20 Minuten
    alter Wochenwert ist brauchbar, ein 20 Minuten alter Session-Wert hinter dem
    Reset nicht. Ist kein Reset bekannt (resets_at fehlt), bleibt das Limit stehen —
    Unwissen ist kein Grund zu entwerten.

    now injizierbar (tz-aware datetime), damit Tests nicht an der Wanduhr haengen.
    """
    now = now or datetime.now(UTC)
    out = []
    for lim in limits:
        dt = _as_utc(lim.get("resets_at"))
        if dt is not None and (dt - now).total_seconds() <= 0:
            lim = dict(lim, percent=None, severity="", stale=True)
        out.append(lim)
    return out


def _keep_in_tooltip(lim: dict[str, Any]) -> bool:
    """Welche Limits im Hover erscheinen: Session + Wochen-Gesamt immer, modell-
    spezifische Wochenlimits nur, wenn sie Signal tragen (Prozent > 0 oder aktiv).
    So bleibt der Tooltip aufgeraeumt, wenn ein Modell-Limit noch bei 0 % steht."""
    if lim["group"] == "session" or lim["kind"] == "weekly_all":
        return True
    return bool(lim["percent"]) or lim["active"]


def tooltip_text(snap: dict[str, Any], now: Any = None) -> str:
    """Mehrzeiliger Hover-Text aus einem Poller-Snapshot (siehe UsagePoller)."""
    head = i18n.L("Claude – Nutzung", "Claude – usage")
    limits = [lim for lim in (snap.get("limits") or []) if _keep_in_tooltip(lim)]
    if not limits:
        return f"{head}\n{snap.get('error') or i18n.L('warte auf Daten…', 'waiting for data…')}"
    lines = [head]
    for lim in limits:
        pct = f"{lim['percent']} %" if lim["percent"] is not None else "— %"
        reset = fmt_reset(lim["resets_at"], now)
        line = f"{lim['label']}: {pct}"
        if reset:
            line += i18n.L(f"  ·  Reset in {reset}", f"  ·  resets in {reset}")
        lines.append(line)
    # Fusszeile: Stand und/oder Fehlergrund. Sie ist die einzige Stelle, an der ein
    # '— %' seinen Grund nennt — ohne sie sieht ein eingefrorener Wert genauso aus
    # wie ein frischer.
    age = fmt_age(snap.get("ts"), now)
    err = snap.get("error")
    if age or err:
        note = i18n.L(f"Stand {age}", f"as of {age}") if age \
            else i18n.L("letzter Wert", "last value")
        lines.append(f"({note} – {err})" if err else f"({note})")
    return "\n".join(lines)
