"""Reihenfolge und Umsortieren - die Rechnung hinter dem Ziehen.

VS Code gibt seine visuelle Reihenfolge nicht preis (weder die der Terminals noch die
der Fenster), also fuehrt das Deck seine eigene. Hier steht, was daran rechenbar ist,
und zwar ohne Bildschirm:

  • `apply_order` legt die gemerkte Wahl ueber die gemeldete Liste - dieselbe Frage bei
    den Kacheln einer Reihe wie bei den Repo-Bloecken untereinander.
  • `drop_index` / `reflow_tops` fuehren einen laufenden Zug: wohin gehoert das gezogene
    Element, und wo muessen die anderen hin, damit es Platz findet.
  • `move_to` schreibt das Ergebnis fest.

`drop_index`/`reflow_tops` rechnen mit VARIABLEN Hoehen, weil die Repo-Bloecke
unterschiedlich hoch sind (verbunden = Kopf plus Kachelreihe, getrennt = zwei
Textzeilen). Eine Kachelreihe kommt mit einem festen Schritt aus und rechnet darum
weiter in ui/reorder.py.
"""
from collections.abc import Sequence


def apply_order(saved: Sequence[str], live: Sequence[str]) -> list[str]:
    """Die gemerkte Reihenfolge <saved> ueber die gemeldete Liste <live> legen.

    Es gilt: was in beiden steht, kommt in der Reihenfolge von <saved>; was nur <live>
    meldet (neu oder unbekannt), haengt hinten in Melde-Reihenfolge an; was nur <saved>
    kennt (inzwischen weg), fliegt raus. Damit ist <live> die Wahrheit darueber, WAS es
    gibt, und <saved> die Wahrheit darueber, in welcher Reihenfolge - genau die
    Aufteilung, die das Deck braucht, weil nur der Nutzer die Reihenfolge kennt.

    Doppelte Eintraege in <saved> werden nur einmal beruecksichtigt (selbstheilend: eine
    von Hand verhunzte JSON-Datei soll kein Element zweimal einreihen).
    """
    live_set = set(live)
    out: list[str] = []
    seen: set[str] = set()
    for x in saved:
        if x in live_set and x not in seen:
            out.append(x)
            seen.add(x)
    return out + [x for x in live if x not in seen]


def merge_visible(saved: Sequence[str], visible: Sequence[str]) -> list[str]:
    """Die neue Reihenfolge der SICHTBAREN Elemente in die gemerkte Liste einweben.

    Gemerkt sind mehr Elemente, als gerade zu sehen sind - ein geschlossenes Repo steht
    weiter in der Datei. Wuerde man die sichtbaren einfach nach vorn schreiben und den
    Rest anhaengen, verloere jedes geschlossene Repo seinen Platz: es kaeme hinten wieder
    hoch, obwohl der Nutzer es nie dorthin gezogen hat.

    Darum: die PLAETZE, die sichtbare Elemente in <saved> belegen, werden von vorn nach
    hinten mit <visible> neu befuellt; alle anderen bleiben, wo sie sind. Sichtbare, die
    noch nicht in <saved> stehen, haengen vorher hinten an.
    """
    out = list(saved)
    for v in visible:
        if v not in out:
            out.append(v)
    fresh = iter(visible)
    vis = set(visible)
    return [next(fresh) if x in vis else x for x in out]


def slot_tops(heights: Sequence[float], gap: float, top: float,
              from_index: int) -> list[float]:
    """Die Oberkanten, die das gezogene Element an Position 0, 1, … haette.

    Gerechnet wird gegen den *zusammengeruecken* Stapel - also so, wie die anderen
    Elemente laegen, wenn das gezogene gar nicht dabei waere. Das ist der Grund, warum
    beim Ziehen nichts zittert: diese Kanten liegen fest, waehrend die Elemente auf dem
    Bildschirm noch zu ihren neuen Plaetzen gleiten. Wuerde man gegen die IST-Positionen
    pruefen, verschoebe jeder Reflow die naechste Entscheidung.

    Die Liste ist so lang wie <heights> (n-1 Zwischenraeume plus das Ende).
    """
    out: list[float] = []
    y = top
    for i, h in enumerate(heights):
        if i == from_index:
            continue
        out.append(y)
        y += h + gap
    out.append(y)
    return out


def drop_index(heights: Sequence[float], gap: float, top: float,
               from_index: int, drag_top: float) -> int:
    """Zielposition (0 .. len-1) des gezogenen Elements: die Position, deren Oberkante
    seiner aktuellen am naechsten liegt.

    <drag_top> ist die Oberkante des ELEMENTS, nicht die Zeigerposition - so bleibt der
    Griff-Offset korrekt (man hat es irgendwo angefasst, nicht an der Kante).

    Warum die Kante und nicht die Mitte: bei variablen Hoehen ist die Mitten-Regel
    ('einsetzen vor dem ersten, dessen Mitte tiefer liegt') gegen den zusammengeruecken
    Stapel nicht erreichbar. Ein hoher Block muesste dafuer mit seiner MITTE ueber die
    Mitte des ersten (flachen) Elements hinaus - also mit seiner Oberkante weit ueber den
    obersten Rand, wo kein Zeiger mehr hinkommt: Position 0 waere fuer ihn nicht
    erreichbar gewesen. Der Kanten-Vergleich hat zudem exakt dieselbe Vorstellung von
    'Position' wie `reflow_tops`, dessen Luecke genau an dieser Kante entsteht (die
    Kachelreihe rechnet mit ihrem festen Schritt genauso, siehe ui/reorder.py).
    """
    tops = slot_tops(heights, gap, top, from_index)
    return min(range(len(tops)), key=lambda p: abs(tops[p] - drag_top))


def reflow_tops(heights: Sequence[float], gap: float, top: float,
                from_index: int, target: int) -> dict[int, float]:
    """Ziel-Oberkanten der NICHT gezogenen Elemente: {index: y}.

    Sie ruecken zusammen und lassen an <target> genau eine Luecke frei, so hoch wie das
    gezogene Element - die klassische 'Platz machen'-Anordnung. Das gezogene Element
    selbst kommt nicht vor: es folgt dem Zeiger.

    Mit target == from_index ergibt das exakt den Ausgangsstapel; ein Zug, der wieder an
    seinem Anfang endet, bewegt also nichts.
    """
    want: dict[int, float] = {}
    y = top
    pos = 0
    for i, h in enumerate(heights):
        if i == from_index:
            continue
        if pos == target:
            y += heights[from_index] + gap     # Luecke fuer das gezogene Element
            pos += 1
        want[i] = y
        y += h + gap
        pos += 1
    return want


def move_to(order: Sequence[str], item: str, target: int) -> list[str]:
    """<item> aus <order> herausnehmen und an Position <target> wieder einsetzen.

    <target> wird auf den gueltigen Bereich geklemmt - der Zielindex kommt aus einer
    Zeigerposition, und die kennt keine Listengrenzen.
    """
    rest = [x for x in order if x != item]
    t = max(0, min(target, len(rest)))
    return [*rest[:t], item, *rest[t:]]
