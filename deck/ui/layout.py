"""Groesse und Skalierung des Panels - die schlanke Ansicht ist die einzige.

Das Fenster folgt seinem Inhalt: die Kachelreihen bestimmen die Groesse, nicht umgekehrt.
Beim Andocken darf es dabei nie ueber den Arbeitsbereich des Monitors hinauswachsen,
sonst rutscht der Griff aus dem Bild.
"""
import time
from typing import Any

from deck.domain import slot_state as dc
from deck.platform import dpi


class LayoutMixin:
    """Wird in AgentDeck eingemischt (siehe panel.py)."""

    # ── Layout (nur noch die schlanke Ansicht) ──────────
    def _apply_slim_layout(self) -> None:
        """Die Rahmen in fester Reihenfolge packen: das Deck (Agenten + kleiner Name)
        fuellt das Fenster (fill both/expand), die untere Leiste (Usage + Einstellungen)
        bleibt mit fester Hoehe am unteren Rand. Erst loesen, dann neu packen -> die
        Reihenfolge bleibt korrekt, auch wenn die Methode erneut aufgerufen wird."""
        for f in (self.agent_area, self.bottom_bar):
            f.pack_forget()
        self.deck.pack_forget()
        # Untere Leiste ZUERST mit side="bottom" -> sie reserviert den unteren Streifen;
        # das Deck bekommt danach die restliche Flaeche (fill both/expand) und <Configure>
        # feuert beim Resize (_on_deck_configure skaliert das Deck, statt abzuschneiden).
        # Ohne Rand (padx/pady=0) reicht die Leiste bis an die Fensterkanten -> ein
        # durchgehender Streifen statt eines eingerueckten Kastens.
        self.bottom_bar.pack(side="bottom", fill="x")
        self.agent_area.pack(side="top", padx=dpi.px(10),
                             pady=(dpi.px(6), dpi.px(4)), fill="both", expand=True)
        self.deck.pack(fill="both", expand=True)

    def _paint_once(self) -> None:
        """Deck einmal komplett neu zeichnen UND fuellen, ohne den Poll-Timer erneut zu
        schedulen (der laeuft weiter). Fuer sofortiges Neuzeichnen nach dem Moduswechsel."""
        self._render_agents()
        self._last_sig = self._layout_sig()
        self._repaint_tiles()

    def _repaint_tiles(self) -> None:
        """Zustaende einlesen und die AKTUELL gezeichneten Kacheln einfaerben/beschriften,
        ohne den Poll-Timer anzufassen. Nach jedem Neuzeichnen ausserhalb von refresh()
        noetig (Moduswechsel/Resize), damit die frischen Kacheln nicht bis zum naechsten
        Poll blank bleiben."""
        states = dc.read_all()
        live = dc.read_live()
        self._found = dc.read_found_tickets()
        cycle = self._mode_cycle()
        self._update_tiles(states, live, time.time(), cycle)

    def _sync_ui_scale(self, redraw=True) -> Any:
        """Skalierung des Monitors unter dem Deck holen und die Oberflaeche darauf
        umstellen. Laeuft beim Start und in der Poll-Schleife – Tk kennt kein
        WM_DPICHANGED, ein zwischen 150-%- und 100-%-Monitor geschobenes Fenster
        muesste sich sonst selbst nicht anpassen und waere dort zu gross.

        Der vom Nutzer gezogene Zoom (Verhaeltnis Ist-Faktor zu Monitorfaktor)
        bleibt dabei erhalten – umgestellt wird nur die Basis. Rueckgabe: True,
        wenn sich wirklich etwas geaendert hat."""
        f = dpi.factor_for_window(getattr(self, "my_hwnd", 0))
        old = dpi.ui()
        if abs(f - old) < 0.01:
            return False
        dpi.set_ui(f)
        dpi.sync_tk_scaling(self.root)
        self._slim_scale = self._slim_scale * f / old if old else f
        for part in (getattr(self, "bottombar", None), getattr(self, "dock", None)):
            apply = getattr(part, "apply_ui_scale", None)
            if apply:
                apply()
        if redraw:
            self._render_agents()      # zeichnet neu UND passt die Fenstergroesse an
            self._repaint_tiles()
        return True

    def _seed_slim_size(self) -> None:
        """Beim Start das Fenster auf die natuerliche Inhaltsgroesse bringen –
        natuerlich heisst hier: eine Design-Einheit = dpi.ui() Geraetepixel, bei
        150 % also 1.5. Danach setzt die Fenstergroesse nur noch _fit_slim_window
        (bei Inhaltsaenderung); das reine Resize-Rendering (_on_deck_configure)
        fasst die Fenstergroesse bewusst nie an (sonst Resize-Loop / Kampf gegen die
        vom Nutzer gewaehlte Groesse)."""
        self._render_agents_slim(scale=dpi.ui())
        self._repaint_tiles()
        self._fit_slim_window(dpi.ui())

    def _fit_slim_window(self, scale) -> None:
        """Fenster + Canvas exakt auf den Inhalt bei <scale> setzen: rueckt beim Schliessen
        eines Agents/Fensters rechts+unten nach (und waechst beim Hinzufuegen) – oben links
        bleibt verankert. Explizites root.geometry, weil Tk ein vom Nutzer schon einmal
        angefasstes Fenster NICHT von allein verkleinert. +2 px Rundungsluft, damit der
        daraus folgende <Configure> denselben Faktor zurueckrechnet (kein Zitter-Redraw).
        Manuelles Ziehen laeuft NICHT hier durch (das setzt nie eine Fenstergroesse)."""
        self._slim_scale = scale
        nat_w, nat_h = self._slim_nat
        if nat_w <= 0 or nat_h <= 0:
            return
        self._slim_relayout = True
        try:
            self.deck.configure(width=round(nat_w * scale) + 2,
                                height=round(nat_h * scale) + 2)
            self.root.update_idletasks()      # requested size neu berechnen (inkl. Padding)
            self.root.geometry(
                f"{self.root.winfo_reqwidth()}x{self.root.winfo_reqheight()}")
        finally:
            self._slim_relayout = False
        # Angedockt + aufgeklappt: nach der Groessenaenderung wieder buendig an den
        # Rand ruecken (sonst liefe das gewachsene Fenster ueber die Kante hinaus).
        if self.dock is not None:
            self.dock.on_resized()

    def _slim_fit_scale(self) -> Any:
        """Fit-Faktor aus aktueller Canvas-Flaeche / natuerlicher Groesse: min(Breite,
        Hoehe), damit NICHTS verdeckt wird – in beide Richtungen (auch > 1, Zoom). 2px
        Sicherheitsluft gegen Rundung der Font-Groessen. Vor der ersten Realisierung
        (Flaeche 1x1) faellt er auf 1.0 zurueck; harte Untergrenze nur als Crash-Schutz."""
        nat_w, nat_h = self._slim_nat
        aw, ah = self.deck.winfo_width(), self.deck.winfo_height()
        if aw <= 1 or ah <= 1 or nat_w <= 0 or nat_h <= 0:
            return 1.0
        return max(0.05, min((aw - 2) / nat_w, (ah - 2) / nat_h))

    def _on_deck_configure(self, ev) -> None:
        """Fenster/Canvas resized – nur im Slim-Modus relevant: Fit-Faktor neu berechnen
        und das Deck skaliert neu zeichnen. Guard gegen Re-Entrancy + eine kleine Schwelle
        gegen Zitter-Redraws. Im Slim-Modus setzt das Rendern KEINE Canvas-Groesse -> kein
        Resize-Loop."""
        if self._slim_relayout or self._dragging():
            return
        nat_w, nat_h = self._slim_nat
        if nat_w <= 0 or nat_h <= 0 or ev.width <= 1 or ev.height <= 1:
            return
        new_scale = max(0.05, min((ev.width - 2) / nat_w, (ev.height - 2) / nat_h))
        # Schwelle > Rundungsluft der +2/-2 in _fit_slim_window -> das durch dessen
        # geometry ausgeloeste <Configure> rechnet ~denselben Faktor und triggert hier
        # keinen erneuten Redraw (kein Zittern/Loop). Zugleich fein genug fuers Ziehen.
        if abs(new_scale - self._slim_scale) < 0.005:
            return
        self._slim_relayout = True
        try:
            self._render_agents_slim(scale=new_scale)
            self._repaint_tiles()
        finally:
            self._slim_relayout = False
