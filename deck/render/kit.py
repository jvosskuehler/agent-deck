"""Anzeige-Baukasten: Farbpalette, pure Farb-/Text-Helfer und wiederverwendbare
tk-Canvas-Primitive (abgerundete Kacheln, Pill-Leiste, Ghost-Button, Hover).

Frueher steckten diese Bausteine als Methoden in der AgentDeck-Gottklasse und
waren dadurch weder vom Button-Raster noch von Tests erreichbar. Jetzt teilen
sich Deck-Rendering und Button-Raster dieselben freien Funktionen; make_hoverable
loest die vierfach kopierte Enter/Leave-Faerbung ab.
"""
import math
import tkinter as tk
import tkinter.font as tkfont
from typing import Any

from deck.platform import dpi, monitor

# ── FROSTPANE-Palette: dunkles OS-Glas, heller Text ──────────────────────
BG          = "#121218"   # Panel-/Fensterkoerper (dunkel getoent)
CARD_FILL   = "#23232b"   # Graphit-Basis (idle/none); Status toent sie ein
CARD_BORDER = "#33333d"   # ruhige Kartenkante
INK         = "#ededf2"   # heller Haupttext (Modellname)
INK_2       = "#cfd3dc"   # heller Sekundaertext (lesbar auch auf getoenten Karten)
INK_3       = "#8b8b99"   # Hinweise / Statuszeile / dezente Icons


# ── Pure Farb-/Text-Helfer ───────────────────────────────────────────────
def hex_to_rgb(h: str) -> tuple[int, int, int]:
    h = h.lstrip("#")
    return int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16)


def mix(c1: str, c2: str, t: float) -> str:
    """Zwei Hexfarben linear mischen: t=0 -> c1, t=1 -> c2. Fuer den weichen
    Glow-Halo (Statusfarbe -> BG) und leicht getoente Kartenkanten."""
    t = 0.0 if t < 0 else 1.0 if t > 1 else t
    a, b = hex_to_rgb(c1), hex_to_rgb(c2)
    return "#{:02x}{:02x}{:02x}".format(*tuple(round(a[i] + (b[i] - a[i]) * t) for i in range(3)))


def short_model(s: str) -> str:
    """Modellname fuers Kartenlabel kuerzen: 'Opus 5 (1M context)' -> 'Opus 5 (1M)'.
    Kurze Namen ('Opus 5', 'Fable 5') bleiben unveraendert. Der Name kommt live von
    Claude Code (statusLine) – hier steht bewusst keine feste Version."""
    return str(s or "—").replace(" context)", ")")


def fit_label(font: Any, text: str, maxw: int, max_lines: int = 2) -> str:
    """Label so umbrechen/kuerzen, dass es in eine Kachel (maxw Breite, max_lines
    Zeilen) passt – sonst wuerde tkinter-Canvas-Text ueber die Kachel/den Rand
    hinauslaufen (Canvas bricht nur an Leerzeichen um und kuerzt nie). Lange
    Woerter werden hart getrennt; ueberzaehliger Rest bekommt am Ende '…'."""
    text = " ".join(str(text).split())          # Whitespace/Zeilenumbrueche glaetten
    if not text or font.measure(text) <= maxw:
        return text
    def fits(s: str) -> bool:
        return font.measure(s) <= maxw
    # In Stuecke zerlegen, die je fuer sich auf eine Zeile passen (lange Woerter hart trennen).
    pieces = []
    for w in text.split(" "):
        while len(w) > 1 and not fits(w):
            k = len(w)
            while k > 1 and not fits(w[:k]):
                k -= 1
            pieces.append(w[:k])
            w = w[k:]
        pieces.append(w)
    # Stuecke gierig in bis zu max_lines Zeilen packen.
    lines: list[str] = []
    cur, i = "", 0
    while i < len(pieces) and len(lines) < max_lines:
        cand = (cur + " " + pieces[i]).strip() if cur else pieces[i]
        if fits(cand):
            cur = cand
            i += 1
        elif cur:
            lines.append(cur)
            cur = ""
        else:                                    # Sicherheitsnetz (nach Hart-Trennung unnoetig)
            lines.append(pieces[i])
            i += 1
    if cur and len(lines) < max_lines:
        lines.append(cur)
        cur = ""
    if (i < len(pieces) or cur) and lines:       # Rest uebrig -> letzte Zeile ellipsieren
        last = lines[-1]
        while last and not fits(last + "…"):
            last = last[:-1]
        lines[-1] = last + "…"
    return "\n".join(lines)


# ── tk-Canvas-Primitive ──────────────────────────────────────────────────
def rr_pts(x1: float, y1: float, x2: float, y2: float, r: float) -> list[float]:
    """Punktliste einer abgerundeten Kachel (fuer create_polygon/coords)."""
    return [x1 + r, y1, x2 - r, y1, x2, y1, x2, y1 + r, x2, y2 - r, x2, y2,
            x2 - r, y2, x1 + r, y2, x1, y2, x1, y2 - r, x1, y1 + r, x1, y1]


def round_rect(c: Any, x1: float, y1: float, x2: float, y2: float, r: float,
               **kw: Any) -> int:
    """Gefuellte, abgerundete Kachel auf dem Canvas (Polygon mit smooth)."""
    return c.create_polygon(rr_pts(x1, y1, x2, y2, r), smooth=True, **kw)


def plus_geom(cx: float, cy: float, arm: float,
              thick: float) -> tuple[float, float, int, int]:
    """Achsen, halbe Armlaenge und Strichdicke eines Plus – aufs Pixelraster gelegt.
    Liefert (ax, ay, a, w); reine Rechnung, damit sie ohne Canvas pruefbar ist.

    Der tk-Canvas antialiast Linien NICHT (gemessen: jedes Randpixel hat volle
    Deckung), er rastert hart: gefuellt wird von `round(anfang)` bis `round(ende)`
    ausschliesslich. Eine Linie liegt damit genau dann symmetrisch um ihre Achse,
    wenn Achse und Armlaenge GANZE Zahlen sind – deshalb werden hier beide gerundet
    statt durchgereicht. Uebrig bleibt nur die Rundung der Soll-Mitte selbst, also
    hoechstens ein halbes Pixel.

    Gerundet wird mit floor(v+0.5) statt round(), weil round() auf .5 zur geraden
    Zahl kippt: die Verschiebung haengt sonst davon ab, wo die Kachel gerade steht."""
    return (float(math.floor(cx + 0.5)), float(math.floor(cy + 0.5)),
            max(1, math.floor(arm + 0.5)), max(1, math.floor(thick + 0.5)))


def plus(c: Any, cx: float, cy: float, arm: float, thick: float,
         **kw: Any) -> tuple[Any, Any]:
    """Ein Plus aus zwei Strichen, zentriert um (cx, cy). Liefert beide Item-IDs.

    Warum nicht `create_text(text="＋")`: tk zentriert die ZEILENBOX (ascent+descent)
    um den Punkt – das Plus-Glyph sitzt darin aber auf der Mathe-Achse, und die liegt
    tiefer als die Zellmitte. Gemessen an Segoe UI Bold, 32 px: Zellmitte 13,5 px
    ueber der Grundlinie, Mathe-Achse 10 px – das Zeichen stand 3,5 px zu tief in
    seinem Kaestchen. Ein Ausgleich waere ein Schriftmass zum Nachpflegen; zwei
    Striche treffen die Mitte unabhaengig von Schrift, Font-Fallback und Hinting
    (und ohne die ClearType-Farbsaeume, die ein Glyph an diesen Kanten hinterlaesst)."""
    ax, ay, a, w = plus_geom(cx, cy, arm, thick)
    return (c.create_line(ax - a, ay, ax + a, ay, width=w, **kw),
            c.create_line(ax, ay - a, ax, ay + a, width=w, **kw))


def make_hoverable(canvas: Any, tag: str, recolors: Any, *, guard: Any = None) -> None:
    """Hover-Verhalten fuer ein Canvas-Tag: Hand-Cursor + Umfaerben beim Betreten,
    zuruecksetzen beim Verlassen. `recolors` = [(item_id, base_farbe, hover_farbe), …].
    `guard` = optionale Funktion; liefert sie True, wird der Hover unterdrueckt
    (z.B. waehrend eines laufenden Drags). Ersetzt die frueher 4x kopierte
    Enter/Leave-Boilerplate."""
    def enter(_e: Any) -> None:
        if guard and guard():
            return
        for it, _base, hov in recolors:
            canvas.itemconfig(it, fill=hov)
        canvas.configure(cursor="hand2")

    def leave(_e: Any) -> None:
        if guard and guard():
            return
        for it, base, _hov in recolors:
            canvas.itemconfig(it, fill=base)
        canvas.configure(cursor="")

    canvas.tag_bind(tag, "<Enter>", enter)
    canvas.tag_bind(tag, "<Leave>", leave)


class Tooltip:
    """Ein einzelnes, wiederverwendetes Hover-Tooltip (randlos, topmost, ohne Fokus)
    fuer Canvas-Kacheln. `show(x, y, text, dx=…, dy=…)` zeigt es am ANKER (x,y),
    versetzt um (dx,dy); leerer/None-Text -> ausgeblendet. `hide()` blendet aus. Das
    Toplevel wird LAZY beim ersten show() angelegt -> die Konstruktion (auch headless)
    oeffnet noch kein Fenster.

    (x, y) MUSS im selben Koordinatensystem wie wm_geometry liegen – am robustesten sind
    Zeiger-Koordinaten (winfo_pointerx/y): die stimmen ueber MEHRERE MONITORE und bei
    DPI-Skalierung. Geklemmt wird gegen die Arbeitsflaeche des Monitors unter dem Anker
    (screen_fit), NICHT gegen winfo_screenwidth/height: das liefert nur die PRIMAER-
    Monitor-Groesse und zog den Tooltip auf einem zweiten Monitor faelschlich auf den
    Hauptschirm zurueck."""

    def __init__(self, root: Any, *, wrap: int = 300) -> None:
        self.root = root
        self.wrap = wrap
        self._tip: Any = None
        self._lbl: Any = None

    def _ensure(self) -> None:
        if self._tip is not None:
            return
        tip = tk.Toplevel(self.root)
        tip.withdraw()
        tip.overrideredirect(True)          # kein Rahmen/Titel – reiner Tooltip
        try:
            tip.attributes("-topmost", True)
        except tk.TclError:
            pass
        tip.configure(bg="#7ecbff")         # duenner Cyan-Saum wie die Titelleiste
        # Schriftgroesse in PUNKTEN (Tk skaliert sie ueber `tk scaling` mit der
        # Monitor-Skalierung), Polster und Umbruchbreite dagegen in Pixeln – die
        # muessen wir selbst umrechnen, sonst klebt der Text bei 150 % am Rand.
        lbl = tk.Label(tip, justify="left", anchor="w", bg="#15151c", fg=INK_2,
                       font=("Segoe UI", 9), wraplength=dpi.px(self.wrap),
                       padx=dpi.px(10), pady=dpi.px(8))
        lbl.pack(padx=1, pady=1)            # 1px Inset -> der bg schaut als Saum durch
        self._tip, self._lbl = tip, lbl

    def show(self, x: int, y: int, text: str, *, dx: int = 0, dy: int = 0) -> None:
        text = (text or "").strip()
        if not text:
            self.hide()
            return
        self._ensure()
        self._lbl.configure(text=text)
        # Erst den Text setzen, dann platzieren: screen_fit braucht die fertige Groesse,
        # um am Bildschirmrand um den Anker zu klappen (update_idletasks steckt dort).
        # Alles noch im withdraw-Zustand -> kein 1x1-Blitz an der alten Stelle.
        monitor.place(self._tip, x, y, dx=dx, dy=dy)
        self._tip.deiconify()
        self._tip.lift()

    def hide(self) -> None:
        if self._tip is not None:
            self._tip.withdraw()


def pill_bar(parent: Any, items: Any, side: str = "left") -> Any:
    """Reihe runder Pill-Chips auf einem Canvas (passend zum Deck-Look).
    side='right' haengt die Leiste rechts in <parent> ein (z.B. Enter unten rechts)."""
    font = tkfont.Font(family="Segoe UI", size=9)
    H, PADX, GAP, R = 28, 15, 7, 14
    # Frost-Chips: dunkle Glasflaeche, heller Text, dezenter Rand.
    base, base_bd, base_fg = "#20202a", "#3a3a45", INK
    hov, hov_fg = "#2a2a36", "#ffffff"
    widths = [font.measure(lbl) + 2 * PADX for lbl, _ in items]
    total = (sum(widths) + GAP * (len(items) - 1)) if items else 1
    c = tk.Canvas(parent, bg=BG, highlightthickness=0,
                  height=H + 6, width=max(total + 2, 1))
    c.pack(side="right", anchor="e") if side == "right" else c.pack(anchor="w")
    x = 1
    for i, ((lbl, cb), w) in enumerate(zip(items, widths, strict=False)):
        rect = round_rect(c, x, 3, x + w, 3 + H, R,
                          fill=base, outline=base_bd, width=1)
        txt = c.create_text(x + w / 2, 3 + H / 2, text=lbl, fill=base_fg, font=font)
        tag = f"pill{i}"
        c.addtag_withtag(tag, rect)
        c.addtag_withtag(tag, txt)
        c.tag_bind(tag, "<Button-1>", lambda _e, cb=cb: cb())   # type: ignore[misc]
        make_hoverable(c, tag, [(rect, base, hov), (txt, base_fg, hov_fg)])
        x += w + GAP
    return c


def ghost_btn(c: Any, x: float, y: float, text: str, cmd: Any, *, size: int = 11,
              bold: bool = True, fg: str = INK, h: int = 26, padx: int = 12,
              tag: str | None = None) -> Any:
    """Transparenter (Ghost-)Button: Fuellung = Panel-BG (durchsichtig, aber
    klickbar), duenner Rand, Hover hebt leicht an. Gibt die Breite zurueck."""
    font = tkfont.Font(family="Segoe UI", size=size,
                       weight=("bold" if bold else "normal"))
    w = font.measure(text) + 2 * padx
    tag = tag or f"gb_{int(x)}_{int(y)}"
    rect = round_rect(c, x, y, x + w, y + h, 8, fill=BG,
                      outline=CARD_BORDER, width=1)
    txt = c.create_text(x + w / 2, y + h / 2, text=text, fill=fg, font=font)
    c.addtag_withtag(tag, rect)
    c.addtag_withtag(tag, txt)
    c.tag_bind(tag, "<Button-1>", lambda e: cmd())
    make_hoverable(c, tag, [(rect, BG, "#20202a")])
    return w


def btn(parent: Any, text: str, cmd: Any) -> Any:
    """Kleiner Standard-tk.Button im Deck-Look (fuer Dialoge).

    Gibt das Widget zurueck - Aufrufer, die ihren Knopf spaeter umbenennen (die
    zweistufige Bestaetigung im Einstellungs-Dialog), brauchen ihn in der Hand.
    """
    b = tk.Button(parent, text=text, command=cmd, bg="#3f3f46", fg="#fafafa",
                  activebackground="#52525b", activeforeground="#fff",
                  relief="flat", font=("Segoe UI", 9), padx=8, pady=4)
    b.pack(side="left", padx=3)
    return b
