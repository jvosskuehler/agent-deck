"""Kacheln aufbauen und zeichnen: eine je Claude-Terminal, dazu die „+“-Kachel.

Die Kachelliste wird IN PLACE aktualisiert (siehe _carry_tile_anim) - ein
Vollneubau mit delete('all') setzt Farbe und Statuswert zurück, und dann blitzen
beim Auf- und Zuklappen alle Kacheln neu auf.
"""
import tkinter as tk
from typing import Any

from deck import i18n
from deck.domain import config as cfg
from deck.domain import ordering
from deck.platform import dpi
from deck.render.glow import GLOW_RINGS
from deck.render.kit import INK, INK_3
from deck.ui.theme import RAIL_IDLE


class TilesMixin:
    """Wird in AgentDeck eingemischt (siehe panel.py)."""

    def _ordered_slots(self, w) -> Any:
        """Die Slots dieses Fensters in der vom Nutzer gewaehlten Reihenfolge (Drag&Drop).
        Basis ist die von der Extension gemeldete Liste (broker.terminals); die
        gespeicherte Reihenfolge (self.order) wird darueber gelegt, neue/unbekannte
        Slots haengen hinten in Melde-Reihenfolge an. So bestimmt allein das Deck die
        Anordnung – VS Code gibt die visuelle Pane-Reihenfolge nicht preis, also kann
        sie nicht gespiegelt, wohl aber hier frei getauscht werden."""
        return ordering.apply_order(self.order.get(w, []), self.broker.terminals(w))

    def _block_key(self, w) -> Any:
        """Woran die Block-Reihenfolge haengt: am REPO-Namen, nicht am Fenster-Buchstaben.

        Der Buchstabe ist fluechtig. Wird ein VS-Code-Fenster geschlossen, raeumt
        _cleanup_closed_windows seine Bindung ab, und das naechste geoeffnete Repo erbt
        ihn. Eine Reihenfolge aus Buchstaben waere also nach dem naechsten Fenster-Wechsel
        eine andere – und dass die Reihenfolge NICHT von allein wandert, ist der ganze
        Zweck des Ziehens.

        Ohne Bindung (der kurze Moment zwischen Verbinden und _sync_bindings) gibt es
        nichts zu merken, was einen Neustart ueberdauert -> der Buchstabe als
        Notschluessel."""
        return self.bindings.get(w) or w

    def _ordered_windows(self) -> Any:
        """Die Fenster-Buchstaben in der vom Nutzer gezogenen BLOCK-Reihenfolge (Drag&Drop
        am Repo-Namen, siehe ui/reorder_blocks.py).

        Gemerkt ist die Reihenfolge der Repo-NAMEN (siehe _block_key), gezeichnet wird
        nach Buchstaben – hier werden die beiden zusammengebracht: unbekannte Repos
        haengen hinten in cfg.WINDOWS-Reihenfolge an. `sorted` ist stabil, zwei Bloecke
        mit demselben Schluessel behalten also ihre relative Lage.

        Gefiltert wird bewusst NICHT – wer zeichnet, entscheidet selbst, welche Bloecke
        sichtbar sind (siehe _shown_windows)."""
        keys = [self._block_key(w) for w in cfg.WINDOWS]
        rank = {k: i for i, k in enumerate(ordering.apply_order(self.win_order, keys))}
        return sorted(cfg.WINDOWS, key=lambda w: rank[self._block_key(w)])

    def _shown_windows(self) -> Any:
        """Die Bloecke, die WIRKLICH gezeichnet werden, in ihrer Reihenfolge: gebunden
        oder verbunden. Eine Stelle fuer beide Zeichenwege (_slim_extent misst, was
        _render_agents_slim malt) – liefen sie auseinander, skalierte das Deck gegen eine
        falsche natuerliche Groesse."""
        return [w for w in self._ordered_windows()
                if self.bindings.get(w) or self.broker.connected(w)]

    def _layout_sig(self) -> Any:
        """Signatur des gewuenschten Layouts – nur bei Aenderung neu zeichnen. Nutzt die
        vom Nutzer gewaehlte Reihenfolge, damit ein Umsortieren einen Redraw ausloest –
        die der Kacheln IN einer Reihe wie die der Bloecke untereinander (letztere steckt
        in der Reihenfolge der Tupel selbst)."""
        return tuple(
            (w, self.bindings.get(w), self.broker.connected(w),
             tuple(self._ordered_slots(w)) if self.broker.connected(w) else ())
            for w in self._ordered_windows()
        )

    def _render_agents(self) -> None:
        """Pro verbundenem Fenster ein Block: kleiner Repo-Name als Kopf, darunter die
        Agenten-Kacheln (die schlanke, skalierende Ansicht in _render_agents_slim).
        Inhalt aendert sich (Agent/Fenster zu ODER auf): AKTUELLEN Zoom halten und das
        Fenster an den neuen Inhalt anpassen -> rechte/untere Kante schliessen auf (statt
        den Rest in ein fixes Fenster hochzuskalieren). Manuelles Ziehen laeuft nicht hier,
        sondern ueber _on_deck_configure (das skaliert in ein fixes Fenster)."""
        self._render_agents_slim(scale=self._slim_scale)
        self._fit_slim_window(self._slim_scale)

    # Slim-Layout in DESIGN-Einheiten (Faktor 1.0). Beim Zeichnen wird alles mit dem
    # Fit-Faktor multipliziert -> beim Verkleinern wird alles kleiner statt abgeschnitten.
    _SLIM_W, _SLIM_H, _SLIM_GAP, _SLIM_R, _SLIM_X0 = 148, 52, 10, 12, 12
    _SLIM_ADD_W = 34            # Breite der Geister-＋-Klickflaeche am Reihenende (Design-Einheiten)
    # Vertikale Gliederung der Repo-Bloecke. Diese vier Zahlen sind das, was
    # Zugehoerigkeit ueberhaupt erst lesbar macht, darum stehen sie beisammen:
    # der Glow-Halo ragt RING (= len(GLOW_RINGS)*2) ueber die Kachel hinaus, die
    # SICHTBARE Luft ist also immer der Abstand MINUS RING. Frueher war die Luft
    # unter dem Kopf 4 und ueber dem naechsten Kopf 6 – der Repo-Name stand damit
    # praktisch mittig zwischen der fremden Reihe darueber und seiner eigenen
    # darunter, und die Gruppierung war Auslegungssache. Jetzt 3 gegen 16.
    _SLIM_TOP, _SLIM_BOT = 6, 6      # Rand oben/unten
    _SLIM_HEAD_GAP  = 3              # sichtbare Luft Kopf -> EIGENE Kachelreihe
    _SLIM_BLOCK_GAP = 16             # sichtbare Luft ZWISCHEN zwei Repo-Bloecken
    _SLIM_RAIL_X, _SLIM_RAIL_W = 2, 2   # Schiene links: Abstand vom Canvasrand, Breite.
                                        # Bleibt links vom Halo (der beginnt bei X0-RING = 6).

    def _slim_extent(self) -> Any:
        """Natuerliche (ungescalte) Ausdehnung des Slim-Layouts in Design-Einheiten –
        Basis fuer den Fit-Faktor. Spiegelt exakt die y-/x-Schritte von _render_agents_slim
        bei Faktor 1.0 (Name-Zeile, Kachelreihe inkl. Glow-Halo, Platzhalter). Misst den
        Fensternamen bei Design-Groesse 12 (Font kurz darauf gestellt) – in PIXELN, damit
        die Messung im selben Raum wie die uebrigen Design-Einheiten liegt und nicht mit
        `tk scaling` (also der Monitor-Skalierung) mitwandert.

        ACHTUNG: die y-Schritte hier und in _render_agents_slim MUESSEN gleich bleiben –
        laufen sie auseinander, skaliert das Deck gegen eine falsche natuerliche Groesse
        (Inhalt abgeschnitten oder Fenster zu gross)."""
        W, H, GAP, _R, X0 = self._SLIM_W, self._SLIM_H, self._SLIM_GAP, self._SLIM_R, self._SLIM_X0
        nf = self._slim_name_font
        nf.configure(size=dpi.fontpx(12)[1])
        RING = len(GLOW_RINGS) * 2
        name_h = nf.metrics("linespace")
        y, maxx = self._SLIM_TOP, X0 + W
        shown = self._shown_windows()
        for i, w in enumerate(shown):
            if i:
                y += self._SLIM_BLOCK_GAP        # Luft zum vorigen Block
            repo = self.bindings.get(w) or f"{i18n.L('Fenster', 'Window')} {w}"
            maxx = max(maxx, X0 + nf.measure(repo))
            y += name_h + RING + self._SLIM_HEAD_GAP
            if self.broker.connected(w):
                x = X0
                for _slot in self.broker.terminals(w):
                    x += W + GAP
                    maxx = max(maxx, x - GAP)
                maxx = max(maxx, x + self._SLIM_ADD_W)   # Platz fuer das Geister-＋ am Reihenende
                y += H + RING                    # Blockende = Unterkante des Halos
            else:
                y += name_h
        y += self._SLIM_BOT
        if not shown:
            y += 26
        return maxx + X0, max(y, 40)

    def _render_agents_slim(self, scale=None) -> None:
        """Slim-Modus: pro verbundenem Fenster nur ein KLEINER Name (kein ⟳/✕/Punkt)
        und darunter die Agenten-Kacheln – kein Button-Raster, keine ＋-Kachel. Die
        Kacheln sind dieselben wie im Vollmodus (_draw_tile), Klick/Glow/Tooltip also
        unveraendert. So bleibt 'wirklich nur die Agenten' uebrig.

        Alles wird mit `scale` gezeichnet (Koordinaten, Offsets UND Font-Groessen), damit
        beim Verkleinern des Fensters alles kleiner wird statt abgeschnitten zu werden
        (tkinters canvas.scale wuerde Fonts NICHT mitnehmen -> darum echtes Neuzeichnen).
        scale=None -> aus aktueller Canvas-Flaeche und natuerlicher Groesse berechnen. Im
        Slim-Modus wird BEWUSST keine Canvas-Groesse gesetzt (das macht nur _seed_slim_size)."""
        c = self.deck
        self._hide_prompt_tip()
        # Anim-Zustand der aktuell gezeichneten Kacheln merken, BEVOR alles neu
        # aufgebaut wird: ueberlebende Slots (Fenster/Agent bleibt) sollen ihren
        # Farbton/Glow BEHALTEN, damit ein einzelnes Auf-/Zugehen nicht ALLE Kacheln
        # neu "aufleuchten" laesst (kein Farb-Refade, kein Bloom-Blitz -> kein Reload-Look).
        prev_tiles = dict(self.tiles)
        c.delete("all")
        self.tiles.clear()
        self.win_items.clear()      # Kopf-/Schienen-Items sterben mit dem delete('all')
        self._hot_win = None
        # 1) natuerliche Groesse ermitteln (Design-Einheiten) -> merken fuer den Fit-Handler.
        nat_w, nat_h = self._slim_extent()
        self._slim_nat = (nat_w, nat_h)
        if scale is None:
            scale = self._slim_fit_scale()
        self._slim_scale = scale
        s = scale
        # 2) skaliert zeichnen.
        W, H, GAP, R, X0 = (self._SLIM_W * s, self._SLIM_H * s, self._SLIM_GAP * s,
                            self._SLIM_R * s, self._SLIM_X0 * s)
        nf = self._slim_name_font
        # Pixelschrift (negative Groesse): folgt exakt dem Kachelraster, statt
        # zusaetzlich ueber `tk scaling` mit der Monitor-Skalierung zu wandern.
        nf.configure(size=dpi.fontpx(12, s)[1])
        RING = len(GLOW_RINGS) * 2 * s
        name_h = nf.metrics("linespace")
        small_font = dpi.fontpx(8, s)
        rail_x, rail_w = self._SLIM_RAIL_X * s, self._SLIM_RAIL_W * s
        y = self._SLIM_TOP * s
        shown = self._shown_windows()
        for i, w in enumerate(shown):
            if i:
                y += self._SLIM_BLOCK_GAP * s      # Luft zum vorigen Block
            y_top = y                              # Blockanfang – die Schiene beginnt hier
            # Jedes Item dieses Blocks traegt zusaetzlich b_<w>. Daran zieht BlockDrag den
            # ganzen Block als EINE Einheit senkrecht (ein c.move statt einer Liste von
            # Item-IDs, die beim naechsten Redraw ohnehin ungueltig waere).
            btag = "b_" + w
            repo = self.bindings.get(w) or f"{i18n.L('Fenster', 'Window')} {w}"
            connected = self.broker.connected(w)
            # EIN Text-Item je Name: verbunden hell, sonst gedimmt. (Frueher zeichenweise
            # fuer den Kopf-Schimmer – der ist raus, siehe glow_animator.)
            name = c.create_text(X0, y, anchor="nw", text=repo, font=nf,
                                 fill=INK if connected else INK_3)
            # Der Kopf ist der Griff des Blocks: Klick holt das VS-Code-Fenster nach vorn
            # (show_window), Ziehen sortiert die Bloecke um. Beides haengt an DIESEM einen
            # Press – ob es ein Klick oder ein Zug war, entscheidet BlockDrag.release
            # anhand der Bewegung (wie bei den Kacheln, siehe TileDrag).
            # Der Kopf ist die einzige Stelle im Deck, die das FENSTER meint und nicht
            # einen Agenten – bisher war er ein reines Etikett, obwohl "da hin" die
            # naheliegendste Geste darauf ist. Auch beim getrennten Block gebunden: die
            # Extension kann weg sein, während das Fenster noch offen ist (dann findet
            # es _raise_window über den Titel, und der Klick hilft gerade dort).
            # tag_bind nimmt die Item-ID direkt – der Kopf ist EIN Item, ein eigener Tag
            # brächte nichts.
            c.tag_bind(name, "<Button-1>", lambda e, k=w: self.blocks.press(k, e))
            c.tag_bind(name, "<Enter>", lambda e, k=w: self._head_enter(k))
            c.tag_bind(name, "<Leave>", lambda e: self._head_leave())
            # Knapp gehalten: der Kopf soll an SEINER Reihe kleben. Der Halo braucht RING,
            # darueber bleiben _SLIM_HEAD_GAP sichtbare Luft (siehe Konstanten).
            y += name_h + RING + self._SLIM_HEAD_GAP * s
            if connected:
                x = X0
                for slot in self._ordered_slots(w):
                    self.tile_renderer.draw_tile(c, slot, x, y, W, H, R,
                                                 scale=s, step=W + GAP)
                    c.addtag_withtag(btag, self.tiles[slot]["gtag"])
                    x += W + GAP
                # Geister-＋ am Reihenende: einziger Startweg im Slim-Modus (bewusst
                # klein/blass statt volle ＋-Kachel wie im Vollmodus).
                self.tile_renderer.draw_add(c, w, x, y, H, s)
                c.addtag_withtag(btag, "slimadd_" + w)
                y += H + RING
            else:
                off = c.create_text(X0, y, anchor="nw",
                                    text=i18n.L("— nicht verbunden —", "— not connected —"),
                                    fill="#52525b", font=small_font)
                c.addtag_withtag(btag, off)
                y += name_h
            # Schiene ZULETZT: erst jetzt steht die Unterkante des Blocks fest. Sie ist
            # der eigentliche Behaelter – Kopf und Kachelreihe haengen sichtbar an
            # derselben Linie, statt nur ungefaehr beieinander zu stehen.
            rail = c.create_rectangle(rail_x, y_top, rail_x + rail_w, y,
                                      fill=RAIL_IDLE, outline="")
            for it in (name, rail):
                c.addtag_withtag(btag, it)
            # y/h sind die Masse, mit denen BlockDrag rechnet: Oberkante des Blocks und
            # seine Hoehe bis zur Unterkante der Schiene. Sie stehen hier, weil hier die
            # Wahrheit entsteht – die Bloecke sind unterschiedlich hoch (verbunden =
            # Kopf plus Kachelreihe inkl. Halo, getrennt = zwei Zeilen), und geraten
            # duerfte man das nicht.
            self.win_items[w] = {"name": name, "rail": rail, "connected": connected,
                                 "tag": btag, "y": y_top, "h": y - y_top}
        if not shown:
            c.create_text(X0, y, anchor="nw", width=220 * s, fill="#52525b",
                          font=small_font,
                          text=i18n.L("Warte auf VS-Code-Fenster …", "Waiting for VS Code window …"))
        self._carry_tile_anim(prev_tiles)   # ueberlebende Kacheln erben ihren Zustand -> kein Reload-Blitz
        if self.active_slot and self.active_slot not in self.tiles:
            self.active_slot = None

    # Felder, die eine ueberlebende Kachel beim Neuaufbau erbt, damit sie optisch
    # RUHIG bleibt: die aktuell gefadete Fuellfarbe (fill_rgb/fill_hex), die Glow-
    # Ziele und – entscheidend – status_key. Ohne uebernommenen status_key haelte
    # _update_tiles jede Kachel faelschlich fuer "Statuswechsel" und zuendete bei
    # jedem Redraw einen bloom-Blitz. surge/press-Jobs werden NICHT geerbt: die
    # neue Kachel startet sauber im Ruhezustand (Defaults aus _draw_tile). Ihre
    # Timer laufen aber noch und tragen den ALTEN Record in der Closure – sie
    # duerfen die frischen Items nicht mehr anfassen; dafuer sorgt
    # GlowAnimator._stale (sonst stand der Kachel-Text danach schief).
    # border/border_w gehoeren dazu, seit die Kante im Bildmodus MITGERENDERT wird:
    # ohne sie faellt eine ausgewaehlte Kachel beim Neuaufbau fuer einen Frame auf
    # die Ruhekante zurueck (sichtbares Blinzeln der weissen Auswahl-Kante).
    _CARRY_FIELDS = ("fill_hex", "fill_target", "glow_color", "glow_intensity",
                     "glow_pulse", "status_key", "bloom", "border", "border_w")

    def _carry_tile_anim(self, prev) -> None:
        """Anim-Zustand ueberlebender Slots aus <prev> in die frisch gezeichneten
        Kacheln uebernehmen. Ein Slot, den es vorher NICHT gab (frisch per ＋ oder
        neu verbundenes Fenster), fehlt in <prev> -> er faedt bewusst normal ein.
        Die geerbte Fuellfarbe wird sofort auf die Flaeche gesetzt (sonst blitzt ein
        Frame CARD_FILL auf, bevor der Animator wieder eingreift)."""
        anim = getattr(self, "anim", None)
        for slot, ids in self.tiles.items():
            old = prev.get(slot)
            if not old:
                continue                      # frischer Agent -> normal einfaden
            for k in self._CARRY_FIELDS:
                if k in old:
                    ids[k] = old[k]
            ids["fill_rgb"] = list(old.get("fill_rgb") or ids["fill_rgb"])  # eigene Liste
            if ids.get("rect"):               # Polygon-Fallback: Flaeche direkt faerben
                try:
                    self.deck.itemconfig(ids["rect"], fill=ids["fill_hex"])
                except tk.TclError:
                    pass
            if anim:
                # Im Bildmodus malt das die geerbte Flaeche gleich mit.
                anim.apply_glow(slot, anim.pulse_factor())

