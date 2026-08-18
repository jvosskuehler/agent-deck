"""Untere Status-Leiste des Decks: EIN durchgehender Streifen am Fensterrand.

Loest die frueheren zwei freistehenden Frost-Chips ab (Usage-Badge links +
Einstellungs-Pill rechts). Statt zweier schwebender Pillen ist das jetzt eine
zusammenhaengende Leiste: gefuellte Flaeche ueber die volle Breite + duenne
Trennlinie oben, links flach die Claude-Nutzung (Ampelpunkt + "Claude 91 %"),
rechts flach das "⚙ Einstellungen"-Element. Beide Elemente sind FLACH in die
Leiste gezeichnet (keine Pillen-Umrandung mehr) und heben beim Hover nur dezent
ihren Hintergrund an; Klick oeffnet die Nutzungsseite bzw. die Einstellungen.

Alles liegt auf EINEM Canvas, das mit dem Fenster mitwaechst (<Configure> zeichnet
neu, das rechte Element bleibt rechtsbuendig). Die Nutzungsdaten liefert
claude_usage.UsagePoller aus einem Hintergrund-Thread; gelesen/gezeichnet wird NUR
im Tk-Thread (eigener after()-Timer), damit jeder Tk-Zugriff im Hauptthread bleibt.

Defensiv gebaut wie zuvor das Badge: ein fehlendes/kaputtes Usage-Modul oder ein
nicht laufendes Claude Desktop darf das Deck NIE am Start hindern -> dann bleibt
self.poller None, die Leiste samt Zahnrad bleibt aber bestehen.
"""
import tkinter as tk
import tkinter.font as tkfont
import webbrowser
from typing import Any

from deck import i18n
from deck.platform import dpi
from deck.render.kit import INK_2, INK_3, Tooltip, round_rect

# ── Leisten-Palette (Frost, aber flach statt Pille) ──────────────────────
# Alle Masse hier sind DESIGN-Einheiten (Mass bei 100 %); gezeichnet wird mit
# dpi.px(), damit die Leiste auf einem 150-%-Schirm in echten Pixeln waechst
# statt hochgerechnet zu werden. Die Schriftgroessen stehen dagegen in PUNKTEN –
# die skaliert Tk selbst ueber `tk scaling` (siehe dpi.py).
_BAR_BG  = "#181820"   # Leistenflaeche: dezent heller als der Fensterkoerper (BG)
_BORDER  = "#2c2c36"   # feine Trennlinie oben, setzt die Leiste vom Deck ab
_HOVER   = "#26262f"   # Hover-Highlight hinter dem gerade angefahrenen Element
_H       = 30          # Leistenhoehe
_PADX    = 12          # Innenabstand links/rechts bis zum ersten/letzten Element
_GAP     = 6           # Luecke zwischen Punkt / Label / Wert
_DOT     = 8           # Durchmesser des Ampelpunkts
_HLPADX  = 8           # seitliches Polster des Hover-Highlights ums Element
_HLH     = 22          # Hoehe des Hover-Highlights (zentriert in der Leiste)
_HLR     = 6           # Eckenradius des Hover-Highlights
_URL     = "https://claude.ai/settings/usage"


class BottomBar:
    """Selbststaendige untere Leiste. `on_settings` wird beim Klick auf das
    Zahnrad gerufen. Das packbare Widget ist `self.canvas` (das Deck packt es
    selbst mit side='bottom', fill='x')."""

    def __init__(self, parent, root, *, on_settings, show_usage=True,
                 poll_seconds=120, refresh_ms=1000, url=_URL) -> None:
        self.root = root
        self.on_settings = on_settings
        self.url = url
        self.refresh_ms = refresh_ms
        self._lbl_font = tkfont.Font(family="Segoe UI", size=9)
        self._val_font = tkfont.Font(family="Segoe UI", size=9, weight="bold")
        self._set_font = tkfont.Font(family="Segoe UI", size=9)
        self._sig = None                     # zuletzt gezeichnete Nutzungs-Signatur
        self._last_w = 0                     # zuletzt gezeichnete Breite (Resize-Filter)
        self._hovering = None                # 'usage' | 'settings' | None
        self._alive = True

        self.canvas = tk.Canvas(parent, bg=_BAR_BG, highlightthickness=0,
                                height=dpi.px(_H), width=dpi.px(240),
                                takefocus=0)
        self.canvas.bind("<Configure>", self._on_configure)
        self.canvas.bind("<Destroy>", lambda e: self._on_destroy())

        # Klick/Hover haengen an den TAGS ("usage" / "settings"); die Tag-Bindungen
        # ueberleben das delete("all") und greifen bei jedem neu gezeichneten Element.
        c = self.canvas
        c.tag_bind("usage", "<Enter>", lambda e: self._enter("usage"))
        c.tag_bind("usage", "<Leave>", lambda e: self._leave("usage"))
        c.tag_bind("usage", "<Button-1>", lambda e: self._open_usage())
        c.tag_bind("settings", "<Enter>", lambda e: self._enter("settings"))
        c.tag_bind("settings", "<Leave>", lambda e: self._leave("settings"))
        c.tag_bind("settings", "<Button-1>", lambda e: self._click_settings())

        # Nutzungs-Poller defensiv starten – faellt er aus, bleibt die Leiste heil.
        self.poller = None
        self._snap: Any = None
        if show_usage:
            try:
                from deck.claude.usage import UsagePoller
                self.poller = UsagePoller(poll_seconds=poll_seconds)
                self.poller.start()
            except Exception:
                self.poller = None
        try:
            self._tip = Tooltip(root, wrap=320)
        except Exception:
            self._tip = None

        self._draw()
        self.root.after(self.refresh_ms, self._tick)

    # ── Anzeige-Timer ────────────────────────────────────
    def _tick(self) -> None:
        """Snapshot lesen und nur bei geaenderter Anzeige neu zeichnen (Signatur-
        Vergleich). Reschedule wie die uebrigen Deck-Timer (after)."""
        if not self._alive:
            return
        if self.poller is not None:
            snap = self.poller.snapshot()
            # Die Frische gehoert MIT in die Signatur. Sie kippt, ohne dass sich
            # Zustand, Prozent oder Ampel aendern (der Wert altert ja nur still
            # weiter) - ohne sie im Vergleich bliebe das Badge fuer immer hell,
            # und der ganze Sinn von badge_view waere weggefiltert.
            from deck.claude.usage_view import is_stale
            sig = (snap.get("state"), snap.get("session_percent"),
                   snap.get("session_severity"), is_stale(snap))
            if sig != self._sig:
                self._sig = sig
                self._snap = snap
                self._draw()
                if self._hovering == "usage":     # offener Tooltip -> Text mitziehen
                    self._show_tip()
            else:
                self._snap = snap                 # Reset-Countdown im Tooltip frisch halten
        self.root.after(self.refresh_ms, self._tick)

    # ── Layout / Zeichnen ────────────────────────────────
    def _on_configure(self, e) -> None:
        # Nur bei echter Breitenaenderung neu zeichnen (kein Redraw-Sturm).
        if e.width != self._last_w:
            self._draw()

    def apply_ui_scale(self) -> None:
        """Nach einem Monitorwechsel (andere Skalierung) die Leiste neu vermessen:
        Hoehe in Geraetepixeln setzen und neu zeichnen. Das Panel ruft das aus
        _sync_ui_scale."""
        try:
            self.canvas.configure(height=dpi.px(_H))
        except tk.TclError:
            return
        self._draw()

    def _draw(self) -> None:
        c = self.canvas
        w = c.winfo_width()
        if w <= 1:                               # vor der ersten Realisierung
            w = c.winfo_reqwidth()
        self._last_w = w
        c.delete("all")
        # Design-Einheiten -> Geraetepixel (einmal je Zeichnen, danach nur noch H, PADX …)
        H, PADX, GAP = dpi.px(_H), dpi.px(_PADX), dpi.px(_GAP)
        DOT, HLPADX = dpi.px(_DOT), dpi.px(_HLPADX)
        HLH, HLR = dpi.px(_HLH), dpi.px(_HLR)

        # Leistenflaeche + feine Trennlinie am oberen Rand.
        c.create_rectangle(0, 0, w, H, fill=_BAR_BG, outline="")
        c.create_line(0, 0, w, 0, fill=_BORDER)

        cy = H / 2

        # ── links: Claude-Nutzung (nur wenn ein Poller laeuft) ──
        if self.poller is not None:
            snap = self._snap
            from deck.claude.usage_view import badge_view
            value, color = badge_view(snap or {})
            wl = self._lbl_font.measure("Claude")
            wv = self._val_font.measure(value)
            inner = DOT + GAP + wl + GAP + wv

            fill = _HOVER if self._hovering == "usage" else _BAR_BG
            round_rect(c, PADX - HLPADX, cy - HLH / 2,
                       PADX + inner + HLPADX, cy + HLH / 2, HLR,
                       fill=fill, outline="", tags=("usage", "usage_hl"))
            x = PADX
            dr = DOT / 2
            c.create_oval(x, cy - dr, x + DOT, cy + dr, fill=color, outline="",
                          tags=("usage",))
            x += DOT + GAP
            c.create_text(x, cy, text="Claude", anchor="w", fill=INK_3,
                          font=self._lbl_font, tags=("usage",))
            x += wl + GAP
            c.create_text(x, cy, text=value, anchor="w", fill=color,
                          font=self._val_font, tags=("usage",))

        # ── rechts: ⚙ Einstellungen (immer da) ──
        st = i18n.L("⚙ Einstellungen", "⚙ Settings")
        ws = self._set_font.measure(st)
        sx2 = w - PADX
        fill = _HOVER if self._hovering == "settings" else _BAR_BG
        txt_fg = "#ffffff" if self._hovering == "settings" else INK_2
        round_rect(c, sx2 - ws - HLPADX, cy - HLH / 2,
                   sx2 + HLPADX, cy + HLH / 2, HLR,
                   fill=fill, outline="", tags=("settings", "settings_hl"))
        c.create_text(sx2, cy, text=st, anchor="e", fill=txt_fg,
                      font=self._set_font, tags=("settings", "settings_txt"))

    # ── Hover / Klick ────────────────────────────────────
    def _enter(self, which) -> None:
        self._hovering = which
        self.canvas.configure(cursor="hand2")
        if which == "usage":
            self.canvas.itemconfig("usage_hl", fill=_HOVER)
            self._show_tip()
        else:
            self.canvas.itemconfig("settings_hl", fill=_HOVER)
            self.canvas.itemconfig("settings_txt", fill="#ffffff")

    def _leave(self, which) -> None:
        self._hovering = None
        self.canvas.configure(cursor="")
        if which == "usage":
            self.canvas.itemconfig("usage_hl", fill=_BAR_BG)
            if self._tip:
                self._tip.hide()
        else:
            self.canvas.itemconfig("settings_hl", fill=_BAR_BG)
            self.canvas.itemconfig("settings_txt", fill=INK_2)

    def _show_tip(self) -> None:
        if not self._tip:
            return
        try:
            from deck.claude.usage_view import tooltip_text
            # Anker = Ecke der Leiste; der Versatz geht als dx/dy mit, damit der Tooltip
            # am unteren Bildschirmrand nach OBEN klappt statt halb hinter der Taskleiste
            # zu verschwinden (screen_fit).
            self._tip.show(self.canvas.winfo_rootx(), self.canvas.winfo_rooty(),
                           tooltip_text(getattr(self, "_snap", {}) or {}),
                           dx=dpi.px(_PADX), dy=dpi.px(_H + 2))
        except tk.TclError:
            pass
        except Exception:
            pass

    def _open_usage(self) -> None:
        try:
            webbrowser.open(self.url)
        except Exception:
            pass

    def _click_settings(self) -> None:
        try:
            self.on_settings()
        except Exception:
            pass

    def _on_destroy(self) -> None:
        self._alive = False
        if self.poller is not None:
            try:
                self.poller.stop()
            except Exception:
                pass
