"""Repo-Bloecke per Ziehen umsortieren - senkrecht, was die Kacheln waagerecht tun.

Gezogen wird am Repo-Namen. Der ist ohnehin schon der Griff des Blocks (Klick = Fenster
nach vorn), und ein Press ohne Bewegung bleibt genau das - dieselbe Aufteilung wie bei
den Kacheln, damit die Geste an beiden Stellen gleich funktioniert.

Der Unterschied zur Kachelreihe ist die Geometrie: Bloecke sind UNTERSCHIEDLICH hoch
(verbunden = Kopf plus Kachelreihe, getrennt = zwei Zeilen), es gibt also keinen festen
Schritt, mit dem man Zielposition und Luecke ausrechnen koennte. Diese Rechnung steht in
domain/ordering.py und ist dort getestet; hier bleibt das Bewegen.
"""
import tkinter as tk
from typing import Any

from deck.domain import ordering
from deck.render.kit import INK
from deck.ui.theme import RAIL_HOT

# Zeigerweg (px), ab dem aus dem Press ein Ziehen wird - wie bei den Kacheln. Darunter
# bleibt es ein Klick, damit ein leichtes Verrutschen den Blockkopf nicht anhebt.
_THRESHOLD = 8
_EASE = 0.35        # Anteil, um den ein weichender Block je Frame seinem Ziel-y naeherkommt


class BlockDrag:
    """Das Umsortieren der Bloecke als eigenes Objekt, KEIN Mixin - wie TileDrag, und aus
    demselben Grund: es hat echten eigenen Zustand (den laufenden Zug) und nennt seine
    fuenf Abhaengigkeiten in der Signatur.

    Nach aussen fragt man ueber AgentDeck._dragging(), ob gerade gezogen wird; die Fassade
    dort deckt beide Dragger ab, damit kein Aufrufer wissen muss, dass es zwei gibt.
    """

    def __init__(self, root, canvas, win_items, win_order, store, *,
                 raise_window, repaint, ordered_windows, block_key, hide_tip) -> None:
        self.root = root                      # Tk-Root (fuer den Animations-Timer)
        self.deck = canvas                    # der Deck-Canvas, auf dem gezogen wird
        self.win_items = win_items            # {win: {"tag","y","h",…}} – geteilt, nie ersetzen
        self.win_order = win_order            # ["repo-b", …] – die deck-eigene Reihenfolge
        self.store = store                    # BindStore, persistiert sie
        self.show_window = raise_window       # Press ohne Bewegung = Klick -> Fenster nach vorn
        self._paint_once = repaint            # nach dem Ablegen neu zeichnen
        self._ordered_windows = ordered_windows   # aktuelle Block-Reihenfolge (Buchstaben)
        self._block_key = block_key           # Buchstabe -> Merk-Schluessel (der Repo-Name)
        self._hide_prompt_tip = hide_tip      # Tooltip weg, sobald es wirklich zieht
        self._drag: dict[str, Any] | None = None   # laufender Zug (None = keiner)

    def dragging(self) -> Any:
        """True, sobald ein Block-Zug wirklich zieht (Bewegung ueber der Schwelle). Ein
        blosser Press zaehlt NICHT -> der Klick auf den Repo-Namen bleibt moeglich."""
        return bool(self._drag and self._drag.get("moved"))

    def press(self, win, ev) -> None:
        """Maustaste auf dem Repo-Namen: nur den Kandidaten merken (noch kein Zug)."""
        self._drag = {"win": win, "sx": ev.x, "sy": ev.y, "moved": False}

    def motion(self, ev) -> None:
        d = self._drag
        if not d:
            return
        if not d["moved"]:
            if abs(ev.x - d["sx"]) + abs(ev.y - d["sy"]) < _THRESHOLD:
                return                     # unter der Schwelle noch als Klick werten
            if not self._begin(d, ev):
                self._drag = None          # nichts sinnvoll zu ziehen -> abbrechen
                return
        c = self.deck
        # Gezogener Block folgt dem Zeiger – nur senkrecht, waagerecht gibt es nichts zu
        # tauschen (die Bloecke stehen untereinander).
        try:
            c.move(d["tag"], 0, ev.y - d["lasty"])
            c.tag_raise(d["tag"])          # ueber den anderen Bloecken bleiben
        except tk.TclError:
            return                         # Canvas gerade neu gezeichnet -> Rest erledigt release
        d["lasty"] = ev.y
        # Bezug ist die Oberkante des BLOCKS (nicht der Zeiger) – man hat ihn irgendwo
        # angefasst, und dieser Griff-Offset soll erhalten bleiben.
        drag_top = d["home_y"] + (ev.y - d["begin_y"])
        tgt = ordering.drop_index(d["heights"], d["gap"], d["top"], d["from"], drag_top)
        if tgt != d["target"]:
            d["target"] = tgt
            # Ziel-Oberkanten der anderen Bloecke neu setzen; dorthin gleiten sie in _anim.
            d["want"] = ordering.reflow_tops(d["heights"], d["gap"], d["top"],
                                             d["from"], tgt)

    def _begin(self, d, ev) -> Any:
        """Ersten echten Zug vorbereiten: Reihenfolge und Geometrie der Bloecke erfassen,
        den gezogenen optisch anheben, das Weichen der anderen starten.

        False, wenn es nichts zu ziehen gibt – Block inzwischen weg, oder es ist der
        EINZIGE: bei einem Block gibt es keine Reihenfolge, und ein Kopf, der sich unter
        dem Zeiger loest und nirgends hin kann, sieht nach einem Fehler aus."""
        items = self.win_items
        order = [w for w in self._ordered_windows() if w in items]
        if d["win"] not in order or len(order) < 2:
            return False
        idx = order.index(d["win"])
        heights = [items[w]["h"] for w in order]
        top = items[order[0]]["y"]
        # Der Abstand zwischen zwei Bloecken ist konstant (_SLIM_BLOCK_GAP), aber
        # skaliert – darum aus den GEZEICHNETEN Positionen ablesen statt ihn nachzurechnen
        # und beim naechsten Zoom daneben zu liegen.
        gap = max(0.0, items[order[1]]["y"] - (top + heights[0]))
        d.update({
            "moved": True,
            "order": order,
            "from": idx,
            "target": idx,
            "heights": heights,
            "top": top,
            "gap": gap,
            "tag": items[d["win"]]["tag"],
            "home_y": items[d["win"]]["y"],
            "begin_y": ev.y,
            "lasty": ev.y,
            "cury": {w: items[w]["y"] for w in order},
            "want": {},
            "job": None,
        })
        self._hide_prompt_tip()            # kein Tooltip waehrend des Ziehens
        self._lift(d["win"])
        self.deck.configure(cursor="hand2")
        self._anim()
        return True

    def _lift(self, win) -> None:
        """Den gezogenen Block optisch anheben: Kopf hell, Schiene im Akzent – dasselbe
        Cyan, das eine angehobene Kachel bekommt. Zurueckgesetzt wird nichts, weil nach
        dem Ablegen ohnehin neu gezeichnet wird; bis dahin haelt _highlight_group die
        Hover-Optik still (sonst nahm ein Leave die Hervorhebung sofort wieder weg)."""
        it = self.win_items.get(win)
        if not it:
            return
        try:
            self.deck.itemconfig(it["name"], fill=INK)
            self.deck.itemconfig(it["rail"], fill=RAIL_HOT)
        except tk.TclError:
            pass

    def _anim(self) -> None:
        """Sanftes Weichen der anderen Bloecke zu ihren Ziel-Oberkanten (ease), im
        16-ms-Takt, solange gezogen wird. Der gezogene Block folgt in motion() direkt dem
        Zeiger; hier bewegen sich nur die anderen, um Platz zu machen bzw. die Luecke
        wieder zu schliessen, wenn man zurueckzieht."""
        d = self._drag
        if not d or not d.get("moved"):
            return
        c = self.deck
        try:
            for i, wy in d.get("want", {}).items():
                w = d["order"][i]
                it = self.win_items.get(w)
                if not it:
                    continue
                cur = d["cury"].get(w, it["y"])
                ny = wy if abs(wy - cur) < 0.5 else cur + (wy - cur) * _EASE
                if ny != cur:
                    c.move(it["tag"], 0, ny - cur)
                    d["cury"][w] = ny
        except tk.TclError:
            pass
        d["job"] = self.root.after(16, self._anim)

    def release(self, ev) -> None:
        """Loslassen: war es ein Klick (keine Bewegung), das VS-Code-Fenster nach vorn
        holen; war es ein Zug, die neue Reihenfolge festschreiben, speichern und das Deck
        einmal sauber neu zeichnen (auch bei No-Op – dann rasten die Bloecke in ihr
        Raster zurueck)."""
        d = self._drag
        self._drag = None
        if not d:
            return                          # Release ohne Kopf-Press (z.B. auf einer Kachel)
        if d.get("job"):
            try:
                self.root.after_cancel(d["job"])
            except Exception:
                pass
        if not d.get("moved"):
            self.show_window(d["win"])      # reiner Klick -> wie zuvor
            return
        self.deck.configure(cursor="")
        order = ordering.move_to(d["order"], d["win"], d.get("target", d["from"]))
        if order != d["order"]:             # nur bei echter Aenderung speichern
            # Gemerkt werden REPO-Namen, nicht Buchstaben (Begruendung in _block_key), und
            # eingewebt statt vorangestellt: geschlossene Repos stehen weiter in der Liste
            # und sollen ihren Platz behalten. In place – Panel und BindStore halten
            # DIESELBE Liste.
            self.win_order[:] = ordering.merge_visible(
                self.win_order, [self._block_key(w) for w in order])
            self.store.save_win_order()
        self._paint_once()                  # Bloecke in die (neue) Reihenfolge einrasten
