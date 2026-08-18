"""ordering: die Rechnung hinter dem Umsortieren - welche Reihenfolge gilt, wohin ein
gezogenes Element gehoert und wie die anderen Platz machen.

Geprueft wird hier, was ohne Bildschirm entscheidbar ist. Dass eine Kachel dem Zeiger
folgt und ein Block sich anhebt, gehoert in den Sichttest.
"""

import helpers  # noqa: F401 - Import MIT Absicht: legt die Repo-Wurzel auf den

# sys.path und nagelt die Deck-Sprache auf Deutsch.
from deck.domain import ordering

# ── apply_order: gemerkte Wahl ueber gemeldete Liste ─────────────────────────

def test_die_gemerkte_reihenfolge_gewinnt_ueber_die_gemeldete():
    assert ordering.apply_order(["C", "A"], ["A", "B", "C"]) == ["C", "A", "B"]


def test_unbekanntes_haengt_hinten_in_melde_reihenfolge_an():
    """Ein neu geoeffnetes Fenster / ein frisch angelegter Agent stand noch nie in der
    gespeicherten Liste. Es darf nicht verschwinden und soll dort auftauchen, wo es
    gemeldet wurde - hinten."""
    assert ordering.apply_order(["B"], ["A", "B", "C"]) == ["B", "A", "C"]


def test_verschwundenes_faellt_raus():
    """Die gemeldete Liste ist die Wahrheit darueber, WAS es gibt. Ein geschlossener
    Agent bleibt in slot_order.json stehen, darf aber keine Kachel erzeugen."""
    assert ordering.apply_order(["A1", "A2", "A3"], ["A1", "A3"]) == ["A1", "A3"]


def test_doppelte_eintraege_reihen_nur_einmal_ein():
    """Selbstheilung gegen eine von Hand verhunzte JSON-Datei: sonst waere dasselbe
    Element zweimal in der Reihenfolge und die Anzeige zweimal drin."""
    assert ordering.apply_order(["A", "A", "B"], ["A", "B"]) == ["A", "B"]


def test_ohne_gemerkte_reihenfolge_bleibt_die_meldung_stehen():
    assert ordering.apply_order([], ["A", "B"]) == ["A", "B"]
    assert ordering.apply_order(["A", "B"], []) == []


# ── merge_visible: das Ergebnis eines Zuges einweben ─────────────────────────

def test_merge_visible_fuellt_die_plaetze_der_sichtbaren():
    """Geschlossene Repos stehen weiter in der Liste. Ein Zug unter den offenen darf ihren
    Platz nicht verschieben - sonst kaeme 'frontend' nach dem Wiederoeffnen unten hoch,
    obwohl niemand es dorthin gezogen hat."""
    assert ordering.merge_visible(["frontend", "deck", "backend"],
                                  ["backend", "deck"]) == ["frontend", "backend", "deck"]


def test_merge_visible_haengt_unbekannte_sichtbare_hinten_an():
    assert ordering.merge_visible(["deck"], ["deck", "neu"]) == ["deck", "neu"]
    assert ordering.merge_visible([], ["c", "a", "b"]) == ["c", "a", "b"]


def test_merge_visible_laesst_unsichtbare_stehen_wo_sie_sind():
    """Auch vorn: ein zugeklapptes Repo an Position 0 bleibt an Position 0."""
    assert ordering.merge_visible(["zu", "a", "b"], ["b", "a"]) == ["zu", "b", "a"]


# ── drop_index: wohin gehoert das Gezogene? ──────────────────────────────────

# Drei Bloecke mit UNTERSCHIEDLICHEN Hoehen (verbunden/getrennt) - genau der Fall, den
# ein fester Schritt nicht abbildet. Stapel ohne Luecken: 0..10, 15..45, 50..70.
_H = [10.0, 30.0, 20.0]
_GAP, _TOP = 5.0, 0.0


def test_die_slot_kanten_sind_der_zusammengerueckte_stapel():
    """Ohne Block 0 liegen die anderen bei 0..30 und 35..55; Block 0 kann also mit seiner
    Oberkante bei 0, 35 oder 60 landen. Diese drei Kanten sind die Kandidaten."""
    assert ordering.slot_tops(_H, _GAP, _TOP, 0) == [0.0, 35.0, 60.0]
    assert ordering.slot_tops(_H, _GAP, _TOP, 2) == [0.0, 15.0, 50.0]


def test_weit_oben_abgelegt_wird_position_null():
    assert ordering.drop_index(_H, _GAP, _TOP, 2, drag_top=-40.0) == 0


def test_weit_unten_abgelegt_wird_die_letzte_position():
    assert ordering.drop_index(_H, _GAP, _TOP, 0, drag_top=999.0) == 2


def test_es_gewinnt_die_naechstgelegene_slot_kante():
    """Kandidaten fuer Block 0 sind 0, 35 und 60 -> die Entscheidung kippt auf der Mitte
    zwischen zwei Kanten (17.5 und 47.5)."""
    assert ordering.drop_index(_H, _GAP, _TOP, 0, drag_top=17.4) == 0
    assert ordering.drop_index(_H, _GAP, _TOP, 0, drag_top=17.6) == 1
    assert ordering.drop_index(_H, _GAP, _TOP, 0, drag_top=47.4) == 1
    assert ordering.drop_index(_H, _GAP, _TOP, 0, drag_top=47.6) == 2


def test_auch_der_hoechste_block_erreicht_position_null():
    """Der Grund fuer den Kanten- statt Mitten-Vergleich: mit der Mitten-Regel muesste
    ein hoher Block seine MITTE ueber die des ersten (flachen) Elements schieben, seine
    Oberkante also weit ueber den obersten Rand - Position 0 waere unerreichbar. Hier
    genuegt, ihn dorthin zu ziehen, wo er hingehoert."""
    assert ordering.drop_index(_H, _GAP, _TOP, 1, drag_top=_TOP) == 0


def test_ein_versatz_des_stapels_verschiebt_die_schwellen_mit():
    """top ist nicht immer 0: das Deck hat oben einen Rand, und der skaliert."""
    assert ordering.drop_index(_H, _GAP, 100.0, 0, drag_top=117.4) == 0
    assert ordering.drop_index(_H, _GAP, 100.0, 0, drag_top=117.6) == 1


# ── reflow_tops: wie machen die anderen Platz? ───────────────────────────────

def test_ein_zug_der_wieder_am_anfang_endet_bewegt_nichts():
    """target == from_index muss exakt den Ausgangsstapel ergeben (0, 15, 50) - sonst
    zuckte die Reihe schon beim Anheben, obwohl sich die Reihenfolge nicht aendert."""
    assert ordering.reflow_tops(_H, _GAP, _TOP, 1, 1) == {0: 0.0, 2: 50.0}


def test_die_luecke_ist_so_hoch_wie_das_gezogene_element():
    """Block 0 (Hoehe 10) wandert nach unten: Block 1 rutscht auf 0, Block 2 auf 35 -
    und ab Position 2 bleiben 10 + gap frei."""
    assert ordering.reflow_tops(_H, _GAP, _TOP, 0, 2) == {1: 0.0, 2: 35.0}


def test_nach_ganz_oben_gezogen_ruecken_alle_um_seine_hoehe_nach_unten():
    """Block 2 (Hoehe 20) nach vorn: die Luecke steht ganz oben, alles andere beginnt
    erst bei 20 + gap = 25."""
    assert ordering.reflow_tops(_H, _GAP, _TOP, 2, 0) == {0: 25.0, 1: 40.0}


def test_das_gezogene_element_kommt_in_den_zielpositionen_nicht_vor():
    """Es folgt dem Zeiger - wuerde es hier auftauchen, kaempften zwei Bewegungen um
    dasselbe Item."""
    for tgt in (0, 1, 2):
        assert 1 not in ordering.reflow_tops(_H, _GAP, _TOP, 1, tgt)


# ── move_to: das Ergebnis festschreiben ──────────────────────────────────────

def test_move_to_setzt_das_element_an_die_zielposition():
    assert ordering.move_to(["A", "B", "C"], "A", 2) == ["B", "C", "A"]
    assert ordering.move_to(["A", "B", "C"], "C", 0) == ["C", "A", "B"]
    assert ordering.move_to(["A", "B", "C"], "B", 1) == ["A", "B", "C"]


def test_move_to_klemmt_einen_zielindex_ausserhalb_der_liste():
    """Der Zielindex kommt aus einer Zeigerposition, und die kennt keine Listengrenzen."""
    assert ordering.move_to(["A", "B"], "A", 99) == ["B", "A"]
    assert ordering.move_to(["A", "B"], "B", -3) == ["B", "A"]


def test_drop_index_und_reflow_passen_zusammen():
    """Die drei Rechnungen muessen dieselbe Vorstellung von 'Position' haben: legt man
    einen Block genau auf die von reflow_tops freigelassene Luecke, muss drop_index
    wieder dieselbe Zielposition liefern. Laufen sie auseinander, springt die Luecke beim
    Ziehen um eine Stelle - und beim Ablegen landet der Block woanders als angezeigt.

    Geprueft ueber ALLE Kombinationen von Herkunft und Ziel, weil genau hier der erste
    Entwurf (Mitten-Vergleich) auseinanderlief: er stimmte fuer flache Bloecke und log
    fuer hohe."""
    for frm in range(len(_H)):
        for tgt in range(len(_H)):
            tops = ordering.reflow_tops(_H, _GAP, _TOP, frm, tgt)
            # Oberkante der Luecke = die Slot-Kante, an der der Block landen soll.
            gap_top = ordering.slot_tops(_H, _GAP, _TOP, frm)[tgt]
            # Sie muss auch wirklich frei sein: kein anderer Block liegt dort.
            for i, y in tops.items():
                assert y + _H[i] <= gap_top or y >= gap_top + _H[frm], (frm, tgt, i)
            assert ordering.drop_index(_H, _GAP, _TOP, frm, gap_top) == tgt, (frm, tgt)
