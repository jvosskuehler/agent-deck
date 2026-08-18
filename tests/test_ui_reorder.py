"""Umsortieren per Ziehen - der Zug am Repo-Block, von Anfang bis Ende.

Die Rechnung dahinter steht in domain/ordering.py und ist dort geprueft; hier geht es um
das, was BlockDrag daraus macht: wann aus einem Press ein Zug wird, was beim Ablegen
gespeichert wird - und dass die gespeicherte Reihenfolge das Zeichnen wirklich bestimmt.

Ohne Tk: die Dragger fassen vom Canvas nur ein halbes Dutzend Methoden an, und die sind
hier Attrappen. Ein Zug, den man nur mit echtem Fenster pruefen koennte, waere gar nicht
geprueft.
"""

import helpers  # noqa: F401 - Import MIT Absicht: legt die Repo-Wurzel auf den

# sys.path und nagelt die Deck-Sprache auf Deutsch.
from deck.ui.reorder import bind_dragging
from deck.ui.reorder_blocks import BlockDrag
from deck.ui.tiles import TilesMixin


class _Ev:
    def __init__(self, x, y):
        self.x, self.y = x, y


class _Canvas:
    """So viel Canvas, wie die Dragger anfassen: verschieben, anheben, faerben, Cursor,
    binden. `moved` summiert das dy je Tag - daran laesst sich ablesen, was sich wirklich
    bewegt hat."""

    def __init__(self):
        self.moved = {}
        self.raised = None
        self.cursor = ""
        self.bound = {}

    def move(self, tag, dx, dy):
        self.moved[tag] = self.moved.get(tag, 0) + dy

    def tag_raise(self, tag):
        self.raised = tag

    def configure(self, cursor=None, **kw):
        if cursor is not None:
            self.cursor = cursor

    def itemconfig(self, item, **kw):
        pass

    def bind(self, seq, fn):
        self.bound[seq] = fn


class _Root:
    """after/after_cancel ohne Ereignisschleife: der Ease-Timer wird bestellt, laeuft hier
    aber nicht - sonst rekursierte der Test in die Animation."""

    def after(self, ms, fn):
        return "job"

    def after_cancel(self, job):
        pass


class _Store:
    def __init__(self):
        self.saves = 0

    def save_win_order(self):
        self.saves += 1


# Drei Bloecke, wie sie das Deck zeichnet: A oben (40 hoch), B (60), C (40), dazwischen
# je 10 Luft. Unterschiedliche Hoehen mit Absicht - genau daran scheitert ein fester
# Schritt (verbundener Block = Kopf plus Kachelreihe, getrennter = zwei Zeilen).
def _items():
    return {"A": {"tag": "b_A", "y": 0, "h": 40, "name": 1, "rail": 2},
            "B": {"tag": "b_B", "y": 50, "h": 60, "name": 3, "rail": 4},
            "C": {"tag": "b_C", "y": 120, "h": 40, "name": 5, "rail": 6}}


# Gemerkt werden REPO-Namen, gezeichnet wird nach Fenster-Buchstaben - hier die Bindung
# dazwischen, wie sie im Deck aus bindings.json kommt.
_REPO = {"A": "repo-a", "B": "repo-b", "C": "repo-c"}


def _drag(win_order=None, items=None, sichtbar=None):
    items = items if items is not None else _items()
    saved = win_order if win_order is not None else []
    reihe = sichtbar if sichtbar is not None else ["A", "B", "C"]
    canvas, store, log = _Canvas(), _Store(), []
    b = BlockDrag(_Root(), canvas, items, saved, store,
                  raise_window=lambda w: log.append(("show", w)),
                  repaint=lambda: log.append("paint"),
                  ordered_windows=lambda: [w for w in reihe if w in items],
                  block_key=lambda w: _REPO.get(w, w),
                  hide_tip=lambda: log.append("hide"))
    return b, saved, canvas, store, log


def test_press_ohne_bewegung_bleibt_ein_klick():
    """Der Kopf war und bleibt der Weg zum VS-Code-Fenster. Nur die Bewegung entscheidet,
    dass es ein Zug ist."""
    b, saved, _c, store, log = _drag()
    b.press("B", _Ev(20, 55))
    b.release(_Ev(20, 55))
    assert log == [("show", "B")]
    assert saved == [] and store.saves == 0


def test_ein_leichtes_verrutschen_hebt_den_block_nicht_an():
    """Unter der Schwelle bleibt es ein Klick - sonst waere jeder Klick auf den Repo-Namen
    ein Glueckspiel."""
    b, _o, canvas, _s, log = _drag()
    b.press("B", _Ev(20, 55))
    b.motion(_Ev(22, 58))
    assert b.dragging() is False
    assert canvas.moved == {} and log == []
    b.release(_Ev(22, 58))
    assert log == [("show", "B")]


def test_nach_oben_gezogen_steht_der_block_vorn_und_wird_gespeichert():
    b, saved, canvas, store, log = _drag()
    b.press("C", _Ev(20, 130))
    b.motion(_Ev(20, 120))                 # ueber die Schwelle -> Zug beginnt
    assert b.dragging() is True
    assert log[0] == "hide"                # kein Tooltip waehrend des Zuges
    b.motion(_Ev(20, 0))                   # Oberkante des Blocks liegt jetzt bei 0
    b.release(_Ev(20, 0))
    # Gemerkt werden REPO-Namen, nicht Fenster-Buchstaben (siehe tiles._block_key).
    assert saved == ["repo-c", "repo-a", "repo-b"]
    assert store.saves == 1
    assert log[-1] == "paint"              # Bloecke rasten in die neue Reihenfolge ein
    # Der Block folgt dem Zeiger ab dem ERSTEN Motion, nicht ab dem Press: die
    # Schwelle (hier 10 px) wird geschluckt, wie bei den Kacheln. Wichtig ist, dass
    # Optik und Rechnung dieselbe Strecke nehmen - sonst rastet er woanders ein, als er
    # liegt.
    assert canvas.moved["b_C"] == -120
    assert canvas.raised == "b_C"          # und lag dabei obenauf
    assert canvas.cursor == ""             # Zeigerform nach dem Ablegen zurueck


def test_der_griff_offset_bleibt_erhalten():
    """Angefasst wird der Kopf irgendwo, nicht an der Blockkante: gerechnet wird darum mit
    der Oberkante des BLOCKS, verschoben um den Zeigerweg. Wer stattdessen die
    Zeigerposition nimmt, laesst den Block beim ersten Pixel springen."""
    b, saved, canvas, _s, _log = _drag()
    b.press("C", _Ev(20, 155))             # weit unten im Block angefasst
    b.motion(_Ev(20, 145))
    b.motion(_Ev(20, 35))                  # Zeigerweg -110 -> Oberkante 120 -> 10
    assert canvas.moved["b_C"] == -110     # der Block, nicht der Zeiger, bestimmt die Strecke
    b.release(_Ev(20, 35))
    # 10 liegt an der Slot-Kante 0 (nicht bei 50) -> der Block gehoert nach vorn.
    assert saved == ["repo-c", "repo-a", "repo-b"]


def test_ein_zug_zurueck_an_den_anfang_speichert_nicht():
    """Sonst schriebe jedes versehentliche Anfassen die Datei neu - und ein Zug, der
    nichts aendert, saehe im Dateidatum wie eine Aenderung aus."""
    b, saved, _c, store, log = _drag()
    b.press("B", _Ev(20, 55))
    b.motion(_Ev(20, 45))
    b.motion(_Ev(20, 90))
    b.motion(_Ev(20, 55))                  # wieder dort, wo er herkam
    b.release(_Ev(20, 55))
    assert saved == [] and store.saves == 0
    assert log[-1] == "paint"              # aber einrasten muss er trotzdem


def test_ein_geschlossenes_repo_behaelt_seinen_platz():
    """In win_order stehen auch Repos, zu denen gerade kein Block gezeichnet wird (Fenster
    zu). Ein Zug unter den offenen darf sie nicht verschieben - sonst waere die Reihenfolge
    nach dem naechsten Wiederoeffnen wieder eine andere."""
    b, saved, _c, store, _log = _drag(
        win_order=["repo-zu", "repo-a", "repo-b", "repo-c"])
    b.press("C", _Ev(20, 130))
    b.motion(_Ev(20, 120))
    b.motion(_Ev(20, 0))
    b.release(_Ev(20, 0))
    assert saved == ["repo-zu", "repo-c", "repo-a", "repo-b"]
    assert store.saves == 1


def test_die_liste_wird_in_place_geaendert():
    """Panel, BindStore und BlockDrag halten DIESELBE Liste. Wuerde release sie ersetzen,
    schriebe der Store weiter die alte Reihenfolge - und die Anzeige zeigte eine, die
    keinen Neustart ueberlebt."""
    b, saved, _c, _s, _log = _drag()
    vorher = saved
    b.press("C", _Ev(20, 130))
    b.motion(_Ev(20, 120))
    b.motion(_Ev(20, 0))
    b.release(_Ev(20, 0))
    assert saved is vorher and saved


def test_bind_dragging_bedient_beide_dragger():
    """Es gibt EIN Motion/Release-Paar am Canvas und zwei Dragger. Faellt einer aus der
    Verdrahtung, laesst sich seine Geste nicht mehr beenden - der Block bliebe schweben."""
    canvas = _Canvas()
    gesehen = []

    class _D:
        def __init__(self, name):
            self.name = name

        def motion(self, ev):
            gesehen.append(("motion", self.name))

        def release(self, ev):
            gesehen.append(("release", self.name))

    bind_dragging(canvas, (_D("kacheln"), _D("bloecke")))
    canvas.bound["<B1-Motion>"](_Ev(0, 0))
    canvas.bound["<ButtonRelease-1>"](_Ev(0, 0))
    assert gesehen == [("motion", "kacheln"), ("motion", "bloecke"),
                       ("release", "kacheln"), ("release", "bloecke")]


# ── Und wirkt die gespeicherte Reihenfolge auch beim Zeichnen? ───────────────

class _Broker:
    def __init__(self, terms):
        self._terms = terms

    def connected(self, w):
        return w in self._terms

    def terminals(self, w):
        return self._terms.get(w, [])


class _Deck(TilesMixin):
    """Nur die Attribute, die die Reihenfolge-Methoden lesen."""

    def __init__(self, win_order, bindings, terms, order=None):
        self.win_order = win_order
        self.bindings = bindings
        self.broker = _Broker(terms)
        self.order = order or {}


def test_die_gezogene_reihenfolge_bestimmt_die_bloecke():
    d = _Deck(["repo-c", "repo-a"], {"A": "repo-a", "B": "repo-b", "C": "repo-c"}, {})
    assert d._ordered_windows()[:3] == ["C", "A", "B"]
    assert d._shown_windows() == ["C", "A", "B"]


def test_die_reihenfolge_haengt_am_repo_nicht_am_fenster_buchstaben():
    """Der Kern der Persistenz: schliesst man ein VS-Code-Fenster, wird sein Buchstabe
    frei und beim naechsten Oeffnen anders vergeben. Die gezogene Reihenfolge muss
    trotzdem dieselbe REPO-Folge zeigen - sonst waere sie jeden Morgen neu zu ziehen.

    Hier stehen dieselben zwei Repos einmal auf A/C und einmal getauscht auf C/A; oben
    steht beide Male repo-c, nur unter einem anderen Buchstaben."""
    gezogen = ["repo-c", "repo-a"]
    heute = _Deck(gezogen, {"A": "repo-a", "C": "repo-c"}, {})
    morgen = _Deck(gezogen, {"A": "repo-c", "C": "repo-a"}, {})
    assert heute._shown_windows() == ["C", "A"]
    assert morgen._shown_windows() == ["A", "C"]
    for d in (heute, morgen):
        assert [d.bindings[w] for w in d._shown_windows()] == gezogen


def test_gezeichnet_wird_nur_gebundenes_oder_verbundenes():
    """Ungenutzte Fenster-Buchstaben kosten nichts (cfg.WINDOWS hat Reserve) - sie duerfen
    aber auch keinen leeren Block erzeugen."""
    d = _Deck([], {"B": "repo-b"}, {"D": ["D1"]})
    assert d._shown_windows() == ["B", "D"]


def test_ein_umsortieren_loest_einen_redraw_aus():
    """_layout_sig ist der Waechter gegen unnoetiges Neuzeichnen. Stuende die
    Block-Reihenfolge nicht darin, bliebe eine getauschte Reihenfolge bis zur naechsten
    anderen Aenderung unsichtbar."""
    binds, terms = {"A": "repo-a", "C": "repo-c"}, {}
    vorher = _Deck(["repo-a", "repo-c"], binds, terms)._layout_sig()
    nachher = _Deck(["repo-c", "repo-a"], binds, terms)._layout_sig()
    assert vorher != nachher
