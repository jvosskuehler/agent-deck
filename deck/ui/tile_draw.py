"""Eine einzelne Kachel zeichnen - und die '+'-Kachel.

Getrennt vom Anordnen (tiles.py), weil es zwei Fragen sind: WO liegt eine Kachel, und
WIE sieht sie aus. Hier steckt das Zweite.

Die Flaeche und der Halo kommen aus Pillow (render/card.py), nicht aus Canvas-Polygonen:
Tk kann kein Antialiasing, und eine harte Rundung sieht auf 150%-Displays ausgefranst
aus.
"""
from typing import Any

from deck.domain import config as cfg
from deck.platform import dpi
from deck.render import card as cr
from deck.render import kit as ck
from deck.render.glow import GLOW_RINGS
from deck.render.kit import BG, CARD_BORDER, CARD_FILL, INK, INK_2, INK_3
from deck.render.kit import hex_to_rgb as _hex_to_rgb
from deck.ui.theme import LOST_GLOW


class TileRenderer:
    """Zeichnet Kacheln - als eigenes Objekt, KEIN Mixin.

    Gemessen brauchte dieses Modul vom Panel genau zwei Dinge (das Kachel-Dict und die
    Breite der Geister-Kachel) und sechs Rückrufe. Die sechs sind kein Ballast, sondern
    die Aussage: SO VIELE Interaktionen hat eine Kachel. Als Mixin stand das nirgends;
    man musste die tag_bind-Zeilen im Rumpf suchen, um es zu wissen.

    `tiles` ist DASSELBE Dict wie im Panel und wird hier in place gefüllt. Das Panel ruft
    beim Neuzeichnen tiles.clear() statt es zu ersetzen - sonst zeigte diese Referenz
    (und die des GlowAnimators) auf ein totes Dict.
    """

    def __init__(self, tiles, add_width, *, on_new, on_close,
                 on_press, on_menu, on_enter, on_leave) -> None:
        self.tiles = tiles              # geteiltes Kachel-Dict (nie ersetzen, nur clear)
        self._SLIM_ADD_W = add_width    # Breite der Geister-＋ am Reihenende
        self.create_agent = on_new      # Klick auf ＋  -> neues Claude-Terminal
        self.close_agent = on_close     # Klick auf ✕  -> Agent schließen
        self._tile_press = on_press     # Linksklick    -> fokussieren / Drag beginnen
        self._card_menu = on_menu       # Rechtsklick   -> Modell/Ticket/Effort/Modus
        self._hover_enter = on_enter    # Zeiger drauf  -> Gruppe hervorheben, Tooltip
        self._hover_leave = on_leave    # Zeiger weg    -> beides zurücknehmen

    def draw_add(self, c, win, x, y, H, s) -> None:
        """Slim-Weg zum Starten eines Agenten (Mockup-Option #2 »Geister-＋ am Reihenende«):
        ein blasses, schmales ＋ hinter der letzten Kachel der Reihe – KEINE volle ＋-Kachel
        wie im Vollmodus, damit der ruhige Slim-Look bleibt. Die gefuellte (BG = im Ruhe-
        zustand unsichtbare) Box ist die Klickflaeche; Hover hellt das ＋ auf und blendet
        einen gestrichelten Rahmen ein. Klick -> create_agent(win): die Extension oeffnet
        EIN weiteres Claude-Terminal in genau diesem Fenster (Autofokus wie bei der Voll-
        Kachel). x kommt bereits um eine GAP hinter der letzten Kachel herein."""
        bw = self._SLIM_ADD_W * s
        bh = min(H, 34 * s)
        by = y + (H - bh) / 2
        box = ck.round_rect(c, x, by, x + bw, by + bh, 11 * s,
                            fill=BG, outline="", width=max(1, round(s)), dash=(3, 3))
        # Zwei Striche statt des Zeichens "＋": als Text sass das Plus 3,5 px zu tief
        # im Kaestchen (tk zentriert die Zeilenbox, das Glyph sitzt auf der Mathe-
        # Achse – Begruendung in ck.plus). Die Masse sind dem alten Glyph abgemessen,
        # damit sich am Aussehen sonst nichts aendert: 16 pt bold ergab bei s=1.5
        # 16 px Spannweite. Der Strich dort war senkrecht 4 px, waagerecht 3 px
        # (Hinting); gewaehlt sind 3 px – mit 4 px in BEIDE Richtungen wirkt das
        # Kreuz fetter als das Zeichen, und das Geister-＋ soll blass bleiben.
        plus = ck.plus(c, x + bw / 2, by + bh / 2, 5.4 * s, 2.2 * s, fill=INK_3)
        tag = "slimadd_" + win
        ptag = tag + "_plus"          # trifft beide Striche mit einem itemconfig
        for it in (box, *plus):
            c.addtag_withtag(tag, it)
        for it in plus:
            c.addtag_withtag(ptag, it)
        c.tag_bind(tag, "<Button-1>", lambda e, g=win: self.create_agent(g))
        c.tag_bind(tag, "<Enter>", lambda e, b=box, p=ptag:
                   (c.itemconfig(p, fill=INK), c.itemconfig(b, outline=CARD_BORDER),
                    c.configure(cursor="hand2")))
        c.tag_bind(tag, "<Leave>", lambda e, b=box, p=ptag:
                   (c.itemconfig(p, fill=INK_3), c.itemconfig(b, outline=""),
                    c.configure(cursor="")))

    def draw_tile(self, c, slot, x, y, W, H, R, scale=1.0, step=None) -> Any:
        # Frostpane-Karte: dunkle Graphitfläche, heller Text, ruhiger Status-GLOW
        # (weicher Halo ringsum). BEWUSST WEGGELASSEN (nach Vorgabe): der Leucht-
        # Streifen an der linken Kante und der Status-Punkt in der Ecke – den Status
        # trägt allein der Glow (+ betonte Kante bei Auswahl/Rückfrage).
        # Layout: Modell (oben links) · Cache-Restzeit + ✕ (oben rechts) · Effort
        # (Zeile 2 rechts) · Status (unten links) · Modus (unten rechts).
        # scale != 1.0 nur im Slim-Modus: x/y/W/H/R kommen bereits skaliert herein, die
        # internen Text-Offsets, Font-Groessen und Halo-Masse werden hier mit demselben
        # Faktor mitskaliert -> die ganze Kachel wird kleiner statt abgeschnitten.
        s = scale
        # Kachelschrift in PIXELN (dpi.fontpx): sie folgt damit exakt demselben
        # Faktor wie die Koordinaten. Eine Punktangabe wuerde zusaetzlich ueber
        # `tk scaling` mit der Monitor-Skalierung wachsen -> doppelt, und der Text
        # liefe aus der Karte.
        def fs(b, w=None) -> Any:
            return dpi.fontpx(b, s, weight=w)
        # Flaeche + Halo + Kante: EIN gerendertes Bild (weiche Rundung, echter
        # Verlauf) – Tk-Canvas selbst kann kein Antialiasing, seine Rundungen
        # treppen. Ohne Pillow ODER bei durchsichtigem Fenster (dort wuerde der
        # Bild-Hintergrund mit ausgestanzt) bleibt es beim bisherigen Weg:
        # Polygon + drei Ring-Umrisse.
        rings, rect, img = [], None, None
        geom = None
        if cr.AVAILABLE and not cfg.TRANSPARENT_BG:
            pad = cr.pad_for(s)
            geom = (max(1, round(W)), max(1, round(H)),
                    max(1, round(R)), pad)
            img = c.create_image(x - pad, y - pad, anchor="nw")
        else:
            for i in range(len(GLOW_RINGS)):
                d = (i + 1) * 2 * s
                rings.append(ck.round_rect(c, x - d, y - d, x + W + d, y + H + d,
                                           R + d, fill="", outline=BG, width=2 * s))
            rect = ck.round_rect(c, x, y, x + W, y + H, R,
                                 fill=CARD_FILL, outline=CARD_BORDER, width=1)
        model = c.create_text(x + 11 * s, y + 12 * s, anchor="w", text="—", fill=INK,
                              font=fs(10, "bold"))
        effort = c.create_text(x + W - 10 * s, y + 27 * s, anchor="e", text="", fill=INK_2,
                               font=fs(8))
        # Restlaufzeit des Prompt-Caches ("42 min"), oben rechts vor dem ✕. Text und
        # Farbe setzt _update_tiles je Poll; ohne Wert (frische Session) bleibt sie leer.
        cache = c.create_text(x + W - 26 * s, y + 12 * s, anchor="e", text="", fill=INK_3,
                              font=fs(8))
        # Ticket-ID (Zeile 2 links, dem Effort gegenueber): zugewiesenes Ticket dieses
        # Slots – unter dem Modell, ueber dem Status. Farbe/Text setzt _update_tiles je
        # Poll; ohne Ticket bleibt die Zeile leer (der frueher hier stehende "Ticket"-
        # Platzhalter war eine Klick-Aufforderung – die Zuweisung laeuft jetzt nur noch
        # ueber das Rechtsklick-Menue, darum keine Button-Attrappe mehr auf der Karte).
        ticket = c.create_text(x + 11 * s, y + 27 * s, anchor="w",
                               text="", fill=INK_3,
                               font=fs(8, "bold"))
        act = c.create_text(x + 11 * s, y + H - 11 * s, anchor="w", text="idle", fill=INK_2,
                            font=fs(8))
        mode = c.create_text(x + W - 10 * s, y + H - 11 * s, anchor="e", text="", fill=INK_2,
                             font=fs(8))
        tag = "t_" + slot
        # Die Ticket-Zeile gehoert jetzt zur normalen Kachel (t_-Tag): Linksklick
        # fokussiert die Kachel wie ueberall, Rechtsklick oeffnet das Kachel-Menue.
        # Ticket zuweisen/aendern laeuft ausschliesslich ueber dieses Rechtsklick-
        # Menue (Untermenue "Ticket") – die frueher direkte Klickflaeche auf der
        # Zeile ist bewusst entfernt.
        # Klick-/Hover-Flaeche: im Bildmodus traegt das Bild den Tag (es IST die
        # Kachelflaeche), sonst das Polygon.
        for it in ((rect or img), model, effort, cache, ticket, act, mode):
            c.addtag_withtag(tag, it)
        # Linksklick auf die Kachel: Klick ODER Ziehen. _tile_press merkt sich nur den
        # Start; ob es ein Klick (fokussieren) oder ein Drag (umsortieren) war, entscheidet
        # _tile_release anhand der Bewegung – die Motion/Release-Handler haengen EINMAL fest
        # am Canvas (siehe _build), damit die Events auch kommen, wenn der Zeiger die Kachel
        # beim Ziehen kurz verlaesst.
        c.tag_bind(tag, "<Button-1>", lambda e, s=slot: self._tile_press(s, e))
        # Rechtsklick irgendwo auf die Kachel -> Kachel-Menue (Ticket zuweisen/entfernen,
        # Agent schliessen).
        c.tag_bind(tag, "<Button-3>", lambda e, s=slot: self._card_menu(s, e))
        # Hover auf der Kachel -> nach kurzer Verzoegerung ein Tooltip mit einer KI-Kurz-
        # zusammenfassung des Chats (chat_summary; Session-Adresse in _update_tiles je Poll
        # aktualisiert, Erzeugung laeuft im Hintergrund). ACHTUNG:
        # der t_-Tag liegt auf mehreren gestapelten Items (rect + Textzeilen); Tk feuert
        # beim Wechsel zwischen ihnen Leave+Enter, OHNE dass die Kachel verlassen wird ->
        # _hover_enter/_hover_leave fangen das ab (Slot-Vergleich + verzoegertes Ausblenden),
        # sonst wuerde der Tooltip beim Bewegen ueber der Kachel flackern/nie erscheinen.
        c.tag_bind(tag, "<Enter>", lambda e, s=slot: self._hover_enter(s))
        c.tag_bind(tag, "<Leave>", lambda e: self._hover_leave())
        # Sichtbarer ✕-Button oben rechts: EIGENES Item/Tag, damit ein Klick darauf
        # NICHT zusaetzlich die Kachel fokussiert (das ✕ liegt ueber dem Rechteck und
        # traegt den t_-Tag NICHT). Klick -> Agent SOFORT schliessen (ohne Rueckfrage);
        # Hover faerbt rot. Feste Farbe (nicht im refresh()-Loop) -> Hover-Rot wird
        # nicht ueberschrieben. (Rechtsklick auf die Kachel zeigt weiter das Menue.)
        cls = c.create_text(x + W - 8 * s, y + 12 * s, anchor="e", text="✕", fill=INK_3,
                            font=fs(11, "bold"))
        xt = "x_" + slot
        c.addtag_withtag(xt, cls)
        c.tag_bind(xt, "<Button-1>", lambda e, s=slot: self.close_agent(s))
        c.tag_bind(xt, "<Enter>", lambda e, i=cls:
                   (c.itemconfig(i, fill=LOST_GLOW), c.configure(cursor="hand2")))
        c.tag_bind(xt, "<Leave>", lambda e, i=cls:
                   (c.itemconfig(i, fill=INK_3), c.configure(cursor="")))
        # Gruppen-Tag über ALLE Items der Kachel -> als Einheit skalierbar (Press & Pop).
        gtag = "g_" + slot
        for it in [*rings, rect, img, model, effort, cache, ticket, act, mode, cls]:
            if it is not None:
                c.addtag_withtag(gtag, it)
        # Anim-/Glow-State: Ziele setzt refresh(), gefadet wird im _anim_tick.
        # fill_rgb = aktuelle Füllfarbe (float, wird zur fill_target hin geeast);
        # bloom = kurzes Aufleuchten bei Statuswechsel; status_key erkennt den Wechsel.
        # surge/surge_job = laufender Glow-Surge (Klick-Feedback 02): Halo-Boost + Kanten-Blitz.
        # press_scale/-job = laufender Press&Pop-Zoom (Klick-Feedback), press_cx/cy = Zentrum.
        # img/geom/photo/img_key = der Bildweg (Flaeche+Halo+Kante als PhotoImage):
        # geom sind die Bildmasse in Pixeln, photo HAELT die Bildreferenz (der
        # Canvas tut das nicht – ohne sie verschwaende die Kachel, sobald der
        # Cache den Eintrag verdraengt), img_key merkt den zuletzt gemalten
        # Zustand, damit nicht jeder Frame ein identisches Bild neu setzt.
        # rect/rings sind im Bildmodus None bzw. leer (Polygon-Fallback).
        self.tiles[slot] = {"rect": rect, "model": model,
                            "effort": effort, "cache": cache, "ticket": ticket,
                            "act": act, "mode": mode,
                            "img": img, "geom": geom, "photo": None,
                            "img_key": None,
                            "border": CARD_BORDER, "border_w": 1,
                            "rings": rings, "glow_color": INK_3,
                            "glow_intensity": 0.0, "glow_pulse": False,
                            "fill_rgb": list(_hex_to_rgb(CARD_FILL)),
                            "fill_hex": CARD_FILL, "fill_target": CARD_FILL,
                            "bloom": 0.0, "surge": 0.0, "surge_job": None,
                            "status_key": None,
                            "gtag": gtag, "press_scale": 1.0, "press_job": None,
                            "press_cx": 0.0, "press_cy": 0.0,
                            # Geometrie fuer das Drag&Drop-Umsortieren: linke obere Ecke,
                            # Breite/Hoehe und der horizontale Schritt (Kachel+Abstand) zur
                            # naechsten Kachel. win = Fensterbuchstabe (nur innerhalb des
                            # eigenen Fensters wird getauscht).
                            "x": x, "y": y, "w": W, "h": H,
                            "step": step if step else W, "win": slot[0],
                            # Hover-Tooltip-Daten (je Poll in _update_tiles aktualisiert):
                            # letzte Frage (Fallback bei HOVER_SUMMARY=False) + session_id/
                            # cwd, mit denen chat_summary das Transcript findet.
                            "prompt": "", "session_id": "", "cwd": ""}
