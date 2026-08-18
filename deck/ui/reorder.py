"""Kacheln per Ziehen umsortieren.

VS Code gibt die Reihenfolge seiner Terminals nicht preis, darum ist die
Reihenfolge deck-eigen und liegt in slot_order.json.

Am Dateiende steht `bind_dragging` - die EINE Stelle, an der Motion und Release
am Canvas haengen. Es gibt zwei Dragger (Kacheln waagerecht, Repo-Bloecke
senkrecht, siehe reorder_blocks.py), aber nur ein Ereignis-Paar.
"""
import tkinter as tk
from typing import Any


class TileDrag:
    """Das Umsortieren als eigenes Objekt, KEIN Mixin.

    Es hat echten eigenen Zustand - den laufenden Drag (_tile_drag) - und war damit der
    beste Kandidat nach den beiden ersten: gemessen brauchte es vom Panel fünf Werte und
    vier Rückrufe, alles andere gehört ihm selbst.

    Nach außen bleibt AgentDeck._dragging() als schmale Fassade stehen. Vier Stellen
    fragen danach, ob gerade gezogen wird - darunter das Dock über app._dragging(), das
    seinen Zeiger-Poll währenddessen zurückhält. Diese Frage soll nicht wissen müssen,
    WO der Drag-Zustand liegt.
    """

    def __init__(self, root, canvas, tiles, order, store, *,
                 focus, repaint, ordered_slots, hide_tip) -> None:
        self.root = root                    # Tk-Root (für die Animations-Timer)
        self.deck = canvas                  # der Deck-Canvas, auf dem gezogen wird
        self.tiles = tiles                  # geteiltes Kachel-Dict (nie ersetzen)
        self.order = order                  # {win: [slot, …]} - die deck-eigene Reihenfolge
        self.store = store                  # BindStore, persistiert die Reihenfolge
        self.focus_slot = focus             # Press ohne Bewegung = Klick -> fokussieren
        self._paint_once = repaint          # nach dem Ablegen neu zeichnen
        self._ordered_slots = ordered_slots # aktuelle Reihenfolge eines Fensters
        self._hide_prompt_tip = hide_tip    # Tooltip weg, sobald es wirklich zieht
        self._tile_drag: dict[str, Any] | None = None   # laufender Drag (None = keiner)

    # VS Code gibt die visuelle Terminal-/Pane-Reihenfolge NICHT preis (kein Positions-/
    # Gruppen-API), also kann das Deck sie nicht spiegeln. Stattdessen ist das Deck die
    # Quelle der Wahrheit: die Kacheln lassen sich per Drag&Drop tauschen, die anderen
    # ruecken dabei zusammen und machen Platz (klassische Sortier-Animation). Die neue
    # Reihenfolge landet in self.order (persistiert via BindStore) und ueberlebt Neustarts.
    def dragging(self) -> Any:
        """True, sobald ein Kachel-Drag wirklich zieht (Bewegung ueber der Schwelle).
        Ein blosser Press ohne Bewegung zaehlt NICHT -> ein normaler Klick bleibt moeglich."""
        return bool(self._tile_drag and self._tile_drag.get("moved"))

    def press(self, slot, ev) -> None:
        """Maustaste auf einer Kachel gedrueckt: nur den Drag-Kandidaten merken (noch
        kein Drag). Bewegt sich der Zeiger nicht ueber die Schwelle, wertet _tile_release
        es als Klick -> focus_slot (Verhalten wie zuvor)."""
        self._tile_drag = {"slot": slot, "win": slot[0],
                           "sx": ev.x, "sy": ev.y, "moved": False}

    def motion(self, ev) -> None:
        d = self._tile_drag
        if not d:
            return
        if not d["moved"]:
            if abs(ev.x - d["sx"]) + abs(ev.y - d["sy"]) < 8:
                return                     # unter der Schwelle noch als Klick werten
            if not self._begin_tile_drag(d, ev):
                self._tile_drag = None     # nichts sinnvoll zu ziehen -> abbrechen
                return
        rec = self.tiles.get(d["slot"])
        if not rec:
            return
        # Gezogene Kachel folgt dem Zeiger – nur horizontal, damit sie in ihrer Reihe bleibt.
        self.deck.move(rec["gtag"], ev.x - d["lastx"], 0)
        d["lastx"] = ev.x
        self.deck.tag_raise(rec["gtag"])   # ueber den anderen Kacheln bleiben
        tgt = self._drag_target_index(d, ev.x)
        if tgt != d["target"]:
            d["target"] = tgt
            self._reflow_drag(d)           # Ziel-Positionen der anderen Kacheln neu setzen

    def _begin_tile_drag(self, d, ev) -> Any:
        """Ersten echten Zug vorbereiten: Reihenfolge + Geometrie der Reihe erfassen,
        Kachel optisch anheben, den Sanft-Ease der Nachbarn starten. False, wenn es nichts
        zu ziehen gibt (Kachel inzwischen weg -> als Klick behandeln)."""
        win = d["win"]
        order = [s for s in self._ordered_slots(win) if s in self.tiles]
        if d["slot"] not in order:
            return False
        rec = self.tiles[d["slot"]]
        idx = order.index(d["slot"])
        d.update({
            "moved": True,
            "order": order,
            "from": idx,
            "target": idx,
            "x0": self.tiles[order[0]]["x"],
            "step": rec["step"] or rec["w"],
            "home_x": rec["x"],
            "begin_x": ev.x,
            "lastx": ev.x,
            "curx": {s: self.tiles[s]["x"] for s in order},
            "want": {},
            "job": None,
        })
        self._hide_prompt_tip()            # kein Tooltip waehrend des Ziehens
        try:
            self.deck.itemconfig(rec["rect"], outline="#7ecbff", width=2)  # angehoben
        except tk.TclError:
            pass
        self.deck.configure(cursor="hand2")
        self._drag_anim()
        return True

    def _drag_target_index(self, d, ev_x) -> Any:
        """Aktuelle Zielposition (0..n-1) aus der Lage der gezogenen Kachel: ihre linke
        Kante relativ zum Reihenanfang, auf die naechste Spalte gerundet. Bezug auf die
        Kachel (nicht den blossen Zeiger) -> der Griff-Offset bleibt korrekt."""
        step = d["step"] or 1
        cur_left = d["home_x"] + (ev_x - d["begin_x"])
        raw = (cur_left - d["x0"]) / step
        return max(0, min(len(d["order"]) - 1, round(raw)))

    def _reflow_drag(self, d) -> None:
        """Ziel-x aller NICHT gezogenen Kacheln fuer die aktuelle Einfuege-Position
        berechnen: sie ruecken zusammen und lassen an d['target'] genau eine Luecke fuer
        die gezogene Kachel frei (die klassische 'Platz machen'-Anordnung)."""
        tgt = d["target"]
        want = {}
        p = 0
        for s in d["order"]:
            if s == d["slot"]:
                continue
            if p == tgt:
                p += 1                     # Luecke fuer die gezogene Kachel auslassen
            want[s] = d["x0"] + p * d["step"]
            p += 1
        d["want"] = want

    def _drag_anim(self) -> None:
        """Sanftes Nachziehen der Nachbarkacheln zu ihren Ziel-x (ease), im 16-ms-Takt,
        solange gezogen wird. Die gezogene Kachel selbst folgt in _tile_motion direkt dem
        Zeiger; hier bewegen sich nur die anderen, um Platz zu machen bzw. wieder zu
        schliessen, wenn man zuruueckzieht."""
        d = self._tile_drag
        if not d or not d.get("moved"):
            return
        c = self.deck
        try:
            for s, wx in d.get("want", {}).items():
                rec = self.tiles.get(s)
                if not rec:
                    continue
                cur = d["curx"].get(s, rec["x"])
                nx = wx if abs(wx - cur) < 0.5 else cur + (wx - cur) * 0.35
                if nx != cur:
                    c.move(rec["gtag"], nx - cur, 0)
                    d["curx"][s] = nx
        except tk.TclError:
            pass
        d["job"] = self.root.after(16, self._drag_anim)

    def release(self, ev) -> None:
        """Loslassen: war es ein Klick (keine Bewegung), fokussieren; war es ein Drag, die
        neue Reihenfolge festschreiben, speichern und die Reihe sauber einrasten (auch bei
        No-Op zurueck an den Start -> Kachel schnappt in ihr Raster)."""
        d = self._tile_drag
        self._tile_drag = None
        if not d:
            return                          # Release ohne Kachel-Press (z.B. auf ✕/Kopf)
        if d.get("job"):
            try:
                self.root.after_cancel(d["job"])
            except Exception:
                pass
        if not d.get("moved"):
            self.focus_slot(d["slot"])      # reiner Klick -> wie zuvor
            return
        self.deck.configure(cursor="")
        win = d["win"]
        order = [s for s in d["order"] if s != d["slot"]]
        tgt = max(0, min(d.get("target", d["from"]), len(order)))
        order.insert(tgt, d["slot"])
        if order != d["order"]:             # nur bei echter Aenderung speichern
            self.order[win] = order
            self.store.save_order()
        self._paint_once()                  # Kacheln in die (neue) Reihenfolge einrasten


def bind_dragging(canvas, draggers) -> None:
    """Motion und Release EINMAL fest am Canvas verdrahten, fuer alle Dragger.

    Warum am Canvas und nicht je Kachel: sonst entsteht bei jedem Neuzeichnen ein
    Handler-Stapel, und die Ereignisse kaemen nicht mehr an, sobald der Zeiger das
    gezogene Item kurz verlaesst - beim Ziehen der Normalfall.

    Warum ALLE Dragger dasselbe Ereignis bekommen: jeder ist untaetig, solange er keinen
    Press gesehen hat, und ein Press liegt immer nur auf EINEM Griff (Kachel oder
    Blockkopf). Die Weiche steckt also im Press, nicht hier.
    """
    def motion(ev) -> None:
        for d in draggers:
            d.motion(ev)

    def release(ev) -> None:
        for d in draggers:
            d.release(ev)

    canvas.bind("<B1-Motion>", motion)
    canvas.bind("<ButtonRelease-1>", release)
