"""Farben, Statusstile und Anzeigetexte des Panels — das Aussehen, nicht die Schalter.

Diese Werte lagen früher auf der Modulebene von panel.py. Sie stehen hier, weil die
ui-Mixins sie brauchen: würden sie aus panel.py importieren, das die Mixins selbst
einbindet, entstünde ein Zirkelbezug.

Was hier NICHT mehr steht, sind die einstellbaren Werte (Takte, Fristen, Schalter).
Die hielt dieses Modul früher als Kopie aus `config` — `POLL_MS = getattr(cfg,
"POLL_MS", 400)`. Eine Kopie friert beim Import ein, und weil mehrere Module dieselbe
Zahl kopierten, gab es sie mehrfach: ein Testpatch am falschen Ort blieb still
wirkungslos. Solche Werte liest jetzt jede Stelle direkt als `cfg.POLL_MS`.
"""

from typing import Any

from deck import i18n
from deck.render.kit import BG, CARD_BORDER, CARD_FILL, INK_3
from deck.render.kit import mix as _mix

# ── FROSTPANE-Palette (BG/CARD_FILL/INK…) lebt jetzt in canvas_kit ───────
# Aus der schwarzen Kiste wird eine dunkel getönte Scheibe; jede Karte glüht ruhig
# in ihrer Statusfarbe. Bewusst WEGGELASSEN (nach Vorgabe): der Leucht-Streifen an
# der linken Kante und der Status-Punkt in der Ecke. Die Grundfarben sind oben aus
# canvas_kit importiert; die Status-/Glow-Farben bleiben unten deck-spezifisch.

# Status -> Anzeigetext, je (deutsch, englisch) – i18n.L() waehlt beim Zeichnen die
# aktuelle Sprache (interne Namen aus Hooks/report.py bleiben unveraendert;
# running+thinking teilen sich "denkt"/"thinking", done = ungelesene Antwort).
STATUS_LABEL = {
    "idle": ("idle", "idle"), "done": ("ungelesen", "unread"),
    "thinking": ("denkt", "thinking"), "running": ("denkt", "thinking"),
    "waiting": ("Rückfrage", "needs input"), "none": ("—", "—"),
    # Kein gemeldeter Status, sondern das ehrliche Eingeständnis eines Remote-Fensters:
    # dessen Hooks schreiben auf der Gegenstelle, hier kommt nichts an (status_model.
    # initial_status). Bewusst ein eigenes Wort statt "idle" – die Kachel ist da und
    # bedienbar, nur ihr Zustand ist von hier aus nicht zu sehen.
    "unknown": ("unbekannt", "unknown"),
}


def status_label(status) -> Any:
    """Lokalisierter Status-Anzeigetext (schaltet mit der Deck-Sprache)."""
    return i18n.L(*STATUS_LABEL.get(status, ("idle", "idle")))
# Status -> (Glow-Farbe, Glow-Intensität 0..1, atmet?, Füll-Tönung 0..1). Die Karte
# wird um die Füll-Tönung in ihre Statusfarbe eingefärbt (dunkles Glas), der Halo
# trägt den Status zusätzlich; "denkt"/"Rückfrage" pulsieren langsam, der Rest ruhig.
GLOW_STYLE = {
    "idle":     ("#8b8b99", 0.22, False, 0.06),  # grau  – fast neutral, "nichts zu tun"
    "done":     ("#6ee7a8", 0.85, False, 0.28),  # grün  – ungelesene Antwort
    "thinking": ("#7ecbff", 1.00, True,  0.28),  # cyan  – denkt (atmet)
    "running":  ("#7ecbff", 1.00, True,  0.28),  # cyan  – arbeitet (atmet)
    "waiting":  ("#ffc48a", 1.00, True,  0.30),  # amber – braucht dich (atmet)
    "unknown":  ("#8b8b99", 0.08, False, 0.02),  # grau, matt – Status nicht zu sehen
    "none":     ("#8b8b99", 0.00, False, 0.00),  # kein Glow, kein Farbton
}
LOST_FILL = 0.30   # Füll-Tönung für "getrennt" (rot)
# Rot ist KEIN gemeldeter Agent-Status, sondern wird im Panel berechnet: die
# Extension des Fensters haengt nicht (mehr) am Broker -> Verbindung verloren.
# Anzeigetext lokalisiert (i18n.L) direkt am Zeichen-Aufruf, damit er mit der
# Deck-Sprache umschaltet.
LOST_GLOW  = "#ff6b6b"

# Border-Akzente: heben Auswahl / Rückfrage / Verlust zusätzlich zum Glow hervor.
SEL_BORDER  = "#ffffff"
WAIT_BORDER = "#ffc48a"
# Ticket-ID auf der Karte (Zeile zwischen Modell und Status): dezentes Violett –
# klar von den Statusfarben (cyan/grün/amber/rot) unterscheidbar.
TICKET_INK  = "#b7a6ff"
# Selbst aus dem Chat GELESENE Ticket-ID (nicht zugewiesen -> kein worktree dahinter):
# dasselbe Violett, aber gedimmt – so ist auf einen Blick klar, ob die ID nur erkannt
# oder wirklich an den Agenten gebunden ist.
TICKET_AUTO_INK = _mix(TICKET_INK, CARD_FILL, 0.45)
# So viele Zeichen passen in die Zeile, bevor sie dem Effort rechts ins Gehege kommt
# ("PROJ-2691 #62" = 13 – Ticket UND PR gehen sich damit gerade noch aus).
TICKET_MAX_CHARS = 14
# (Frueher stand hier ein "Ticket"-Platzhalter als Klick-Aufforderung auf der Karte.
# Ticket zuweisen laeuft jetzt ausschliesslich ueber das Rechtsklick-Menue -> ohne
# Ticket bleibt die Zeile leer; ein Platzhalter waere eine Button-Attrappe ohne Funktion.)

# Schiene links neben einem Repo-Block: der sichtbare Behaelter, der Kopf (Repo-Name)
# und Kachelreihe zu EINER Gruppe zusammenbindet. Vorher trug die Zuordnung allein der
# Abstand – und der war nach oben (fremde Reihe) genauso gross wie nach unten (eigene
# Reihe), die Gruppierung also reine Auslegungssache.
# Warum eine Schiene und KEINE zweite Farbdimension je Repo: Farbe ist im Deck bereits
# der Status (Halo aussen, leuchtend, atmend). Ein zweiter Farbcode daneben wuerde genau
# den Kanal verwaessern, der die eigentliche Arbeit macht.
# Ruhezustand: leise, aber sichtbar. CARD_BORDER allein (#33333d auf BG #121218) war im
# Render-Vergleich nicht mehr wahrnehmbar – eine Schiene, die man nicht sieht, bindet
# nichts. Ein Schritt in Richtung INK_3 reicht; lauter darf sie nicht werden, sonst
# konkurriert der Behaelter mit seinem Inhalt.
RAIL_IDLE = _mix(CARD_BORDER, INK_3, 0.3)
RAIL_HOT  = "#7ecbff"            # gehoverte Gruppe (dieselbe Aufmerksamkeitsfarbe wie
                                 # angehobene Kachel beim Ziehen und Tooltip-Saum)
RAIL_DIM  = _mix(CARD_BORDER, BG, 0.55)   # fremde Gruppe, waehrend woanders gehovert wird
# GLOW_RINGS (Ring-Fade) lebt jetzt im GlowAnimator; oben importiert, weil _draw_tile
# die Ringe anlegt.

# Statuswechsel-Effekt: refresh() setzt bloom = BLOOM_ON_CHANGE; das eigentliche
# Faden/Atmen/Abklingen (FILL_EASE/BLOOM_DECAY im ANIM_MS-Timer) macht jetzt der
# GlowAnimator. Poll-Takt + Stale-Grenze bleiben hier (refresh gehoert dem Panel);
# die Fallback-Werte greifen nur, falls ein config-Eintrag fehlt.
# So kurz wird nachgefasst, wenn der Poll wegen einer laufenden Ein-/Ausklapp-Bewegung
# aussetzt (siehe refresh). Klein genug, dass die Anzeige direkt nach dem Aufklappen
# frisch ist, gross genug, dass das Nachfragen selbst keine Frames kostet.
SLIDE_RETRY_MS  = 50
PREFETCH_EVERY_S = 5.0    # so oft (s) den Prefetch-Scan laufen lassen (nicht jeden Poll)
