"""Die Teile, die aus den Mixins herausgelöst wurden, sind ohne Panel benutzbar.

Das ist der eigentliche Gewinn der Umstellung, und er ist nur hier prüfbar: ein Mixin
kann man nicht einzeln bauen — es braucht die ganze Klasse und damit ein Tk-Fenster,
einen Broker, einen BindStore. Ein Kollaborateur nennt seine Abhängigkeiten im
Konstruktor, also lässt er sich mit Attrappen aufbauen.

Geprüft wird bewusst die SIGNATUR und die Konstruktion, nicht das Zeichnen: sobald
show() läuft, ist ein echtes Tk-Fenster im Spiel, und das gehört in den Sichttest.
"""
import inspect

import helpers  # noqa: F401 - setzt sys.path und die Deck-Sprache

from deck.ui.reorder import TileDrag
from deck.ui.reorder_blocks import BlockDrag
from deck.ui.settings_dialog import SettingsDialog
from deck.ui.tile_draw import TileRenderer


class _Ev:
    """Ein Maus-Ereignis, so viel davon wie die Dragger anfassen."""

    def __init__(self, x, y):
        self.x, self.y = x, y


def test_settings_dialog_nennt_seine_abhaengigkeiten_im_konstruktor():
    """Vorher stand nirgends, was der Dialog anfasst - man musste die 375 Zeilen von
    AgentDeck lesen. Jetzt steht es in der Signatur, und dieser Test hält es fest."""
    sig = inspect.signature(SettingsDialog.__init__)
    params = [p for p in sig.parameters if p != "self"]
    assert params == ["root", "settings", "store", "dock",
                      "set_modal", "restart", "place"], params
    # Die drei Rückrufe sind keyword-only: beim Aufruf steht damit am Aufrufort, WAS
    # verdrahtet wird, statt vier gleich aussehender Positionsargumente.
    kwonly = [p for p, v in sig.parameters.items()
              if v.kind is inspect.Parameter.KEYWORD_ONLY]
    assert kwonly == ["set_modal", "restart", "place"], kwonly


def test_settings_dialog_baut_ohne_panel():
    """Konstruktion mit Attrappen - kein Tk, kein Broker, kein BindStore.

    Genau das war als Mixin unmöglich: die Methode hing an einer Klasse, die im
    __init__ einen Broker startet, ein Tk-Fenster aufbaut und die DPI-Anmeldung macht.
    """
    gerufen = []
    dlg = SettingsDialog(
        root=None,
        settings={"glow": False, "jira_prefix": "PROJ"},
        store=type("S", (), {"save_settings": lambda self: gerufen.append("save")})(),
        dock=None,
        set_modal=lambda v: gerufen.append(("modal", v)),
        restart=lambda: gerufen.append("restart"),
        place=lambda w: gerufen.append("place"),
    )
    assert dlg.settings["jira_prefix"] == "PROJ"
    # Die Rückrufe liegen als Attribute bereit und sind aufrufbar, ohne dass ein Panel
    # existiert - der Dialog kann also isoliert geprüft werden.
    dlg._set_modal(True)
    dlg.restart()
    dlg.store.save_settings()
    assert gerufen == [("modal", True), "restart", "save"]


def test_kein_mixin_mehr_fuer_den_dialog():
    """AgentDeck mischt den Dialog nicht mehr ein, sondern baut ihn.

    Sonst bliebe die alte Kopplung bestehen und niemand würde es merken: ein
    zusätzliches Mixin in der Vererbungsliste fällt nicht auf.
    """
    from deck.ui.panel import AgentDeck
    namen = [b.__name__ for b in AgentDeck.__bases__]
    assert "SettingsMixin" not in namen, namen
    assert not any("Settings" in n for n in namen), namen
    # Der Einstieg bleibt aber erhalten - die Bottom-Bar hängt ihren ⚙-Knopf daran.
    assert callable(AgentDeck._open_settings)


def test_tile_renderer_macht_die_sechs_interaktionen_sichtbar():
    """Eine Kachel reagiert auf sechs Dinge. Vorher stand das nur in tag_bind-Zeilen
    mitten im Zeichencode; jetzt in der Signatur.

    Der Test hält die ZAHL fest, nicht die Namen der Handler: kommt eine siebte
    Interaktion dazu, ist das eine Entscheidung, die man hier bestätigen muss.
    """
    sig = inspect.signature(TileRenderer.__init__)
    kwonly = [p for p, v in sig.parameters.items()
              if v.kind is inspect.Parameter.KEYWORD_ONLY]
    assert kwonly == ["on_new", "on_close", "on_press",
                      "on_menu", "on_enter", "on_leave"], kwonly


def test_tile_renderer_teilt_das_kachel_dict_statt_es_zu_ersetzen():
    """Der Renderer füllt DASSELBE Dict, das Panel und GlowAnimator lesen.

    Das ist die Zusage, an der die Animation hängt: würde hier ein neues Dict entstehen,
    zeigte der GlowAnimator auf ein totes und alle Kacheln blieben stumm grau. Darum wird
    im Panel tiles.clear() gerufen und nie neu zugewiesen.
    """
    geteilt = {}
    r = TileRenderer(geteilt, 34, on_new=lambda w: None, on_close=lambda s: None,
                     on_press=lambda s, e: None, on_menu=lambda s, e: None,
                     on_enter=lambda s: None, on_leave=lambda: None)
    assert r.tiles is geteilt
    geteilt["A1"] = {"rect": 1}
    assert r.tiles["A1"] == {"rect": 1}      # dieselbe Referenz, nicht eine Kopie


def test_kein_mixin_mehr_fuers_zeichnen():
    from deck.ui.panel import AgentDeck
    namen = [b.__name__ for b in AgentDeck.__bases__]
    assert not any("TileDraw" in n for n in namen), namen


def test_tile_drag_haelt_seinen_zustand_selbst():
    """TileDrag ist der erste herausgelöste Teil mit ECHTEM eigenem Zustand.

    Der laufende Drag lag vorher als self._tile_drag im Panel - unter 50 anderen
    Attributen, und jede der elf Mixins hätte ihn anfassen können. Jetzt gehört er dem
    Objekt, das ihn führt.
    """
    d = TileDrag(None, None, {}, {}, None, focus=lambda s: None,
                 repaint=lambda: None, ordered_slots=lambda w: [], hide_tip=lambda: None)
    assert d.dragging() is False            # ohne Press wird nicht gezogen
    d.press("A1", type("E", (), {"x": 5, "y": 5})())
    assert d.dragging() is False            # Press allein ist noch KEIN Drag ...
    d._tile_drag["moved"] = True
    assert d.dragging() is True             # ... erst Bewegung über der Schwelle


def _block_drag(win_items=None, win_order=None, gerufen=None):
    """BlockDrag mit Attrappen - ohne Tk, Broker, BindStore. Genau das ist der Sinn der
    Objekt-Form: der Zug ist ohne Panel prüfbar."""
    gerufen = gerufen if gerufen is not None else []
    items = win_items if win_items is not None else {}
    return BlockDrag(
        None, None, items, win_order if win_order is not None else [], None,
        raise_window=lambda w: gerufen.append(("show", w)),
        repaint=lambda: gerufen.append("paint"),
        ordered_windows=lambda: list(items),
        block_key=lambda w: w,
        hide_tip=lambda: gerufen.append("hide"),
    ), gerufen


def test_block_drag_haelt_seinen_zustand_selbst():
    """Wie TileDrag: der laufende Zug gehört dem Objekt, das ihn führt - nicht dem Panel
    mit seinen fünfzig Attributen."""
    b, _ = _block_drag()
    assert b.dragging() is False            # ohne Press wird nicht gezogen
    b.press("A", _Ev(5, 5))
    assert b.dragging() is False            # Press allein ist noch KEIN Zug ...
    b._drag["moved"] = True
    assert b.dragging() is True             # ... erst Bewegung über der Schwelle


def test_ein_einzelner_repo_block_wird_nicht_gezogen():
    """Bei einem Block gibt es keine Reihenfolge. Ein Kopf, der sich unter dem Zeiger
    löst und nirgends hin kann, sähe nach einem Fehler aus - also bleibt es beim Klick.

    Der Test kommt ohne Canvas aus, und das ist die eigentliche Zusage: die Absage muss
    fallen, BEVOR das erste Item angefasst wird (canvas ist hier None - würde er zeichnen,
    flöge ein AttributeError)."""
    b, gerufen = _block_drag({"A": {"tag": "b_A", "y": 0, "h": 40}})
    b.press("A", _Ev(0, 0))
    b.motion(_Ev(0, 40))                    # weit über der Schwelle
    assert b.dragging() is False
    assert gerufen == []                    # kein hide_tip, kein Anheben


def test_das_deck_fragt_beide_dragger_ob_gezogen_wird():
    """_dragging() ist die Fassade für BEIDE Gesten (Kacheln waagerecht, Blöcke
    senkrecht). Fehlt einer, zeichnet der Poll-Takt mitten im Zug neu (delete('all')) und
    zerreißt ihn - ein Fehlerbild, das man dem Rechner zuschreibt, nicht dem Code."""
    import inspect

    from deck.ui.panel import AgentDeck
    quelle = inspect.getsource(AgentDeck._dragging)
    assert "self.drag.dragging()" in quelle, quelle
    assert "self.blocks.dragging()" in quelle, quelle


def test_kein_drag_zustand_mehr_im_panel():
    """Das Panel darf den Drag nicht mehr doppelt halten - sonst gäbe es zwei Wahrheiten
    und die Frage, welche gilt."""
    import inspect

    from deck.ui import panel
    quelle = inspect.getsource(panel.AgentDeck.__init__)
    assert "self._tile_drag" not in quelle, "Drag-Zustand liegt wieder im Panel"
