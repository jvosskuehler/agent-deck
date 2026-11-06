"""Welche Kachel verschwindet - und welche NIE von allein.

Die Slot-Buchfuehrung der Extension entscheidet, was das Panel zu sehen bekommt:
gemeldet wird genau ihre Map, und was darin steht, ist eine Kachel. Ihre eine
harte Regel liegt darum in `extension/detect.js` (`deadSlots`) und wird hier ueber
`node` geprueft - sie steht zwischen zwei Fehlern, die beide wehtun.

Der eine ist die LEICHE: ein geschlossenes Terminal, das nach einer langen
await-Pause (Prozess-Scan bis 6 s, Split bis 3 s) wieder in die Map geraet. Zu ihm
feuert danach kein onDidCloseTerminal mehr - die Kachel bliebe stehen, und weil ihr
Chat weg ist, laesst sie sich nicht einmal mehr schliessen.

Der andere ist der umgekehrte: eine Kachel abraeumen, deren Chat noch laeuft. Der
kostet mehr, denn hinter ihr steht Arbeit. Darum raeumt die Regel nur ab, was VS
Code selbst nicht mehr fuehrt, und bei ausbleibender Auskunft gar nichts.

Fehlt `node`, wird die Datei uebersprungen - sichtbar, mit Meldung, nicht still.
"""
import json
import os
import shutil
import subprocess

import helpers  # noqa: F401  - legt die Repo-Wurzel auf den sys.path

DETECT = os.path.join(helpers.ROOT, "extension", "detect.js")
NODE = shutil.which("node")

# Wie in test_extension_killable.py: Skript und Daten getrennt ueber argv, damit
# nichts zu escapen ist und der Pfad mit Umlauten heil bleibt.
_DRIVER = (
    "const d = require(process.argv[2]);"
    "const i = JSON.parse(process.argv[1]);"
    "process.stdout.write(JSON.stringify(d[i.fn].apply(null, i.args)));"
)

# Terminal-Objekte sind fuer die Regel undurchsichtige Referenzen - sie werden nur
# verglichen, nie gelesen. Stellvertreter sind darum schlichte Zeichenketten: ein
# Set entscheidet bei ihnen nach demselben has(), nur eben ueber den Wert.
T1, T2, T3 = "term-1", "term-2", "term-3"


def _dead(entries, live):
    payload = json.dumps({"fn": "deadSlots", "args": [entries, live]})
    out = subprocess.run([NODE, "-e", _DRIVER, payload, DETECT],
                         capture_output=True, text=True, encoding="utf-8", timeout=30)
    assert out.returncode == 0, f"node scheiterte: {out.stderr.strip()}"
    return json.loads(out.stdout)


# ── Die Leiche ──────────────────────────────────────────────────────────────

def test_ein_geschlossenes_terminal_gibt_seinen_slot_frei():
    assert _dead([["A1", T1], ["A2", T2]], [T1]) == ["A2"]


def test_ein_fenster_ohne_terminals_gibt_alle_slots_frei():
    # Das leere Array ist eine ECHTE Auskunft (alles zu), kein Ausfall.
    assert _dead([["A1", T1], ["A2", T2]], []) == ["A1", "A2"]


def test_zwei_slots_auf_demselben_toten_terminal_fallen_beide():
    # Sollte nicht mehr vorkommen (ensureSlot vergibt keinen belegten Namen mehr),
    # waere aber genau der Rest, der sich sonst festsetzt - also raeumt die Regel
    # auf, was sie vorfindet, statt beim ersten Treffer aufzuhoeren.
    assert _dead([["A1", T1], ["A2", T1]], [T2]) == ["A1", "A2"]


# ── Der teurere Gegenfehler ─────────────────────────────────────────────────

def test_laufende_chats_werden_nie_abgeraeumt():
    assert _dead([["A1", T1], ["A2", T2], ["A3", T3]], [T1, T2, T3]) == []


def test_ein_terminal_ohne_slot_stoert_nicht():
    # In VS Code steht auch, was dem Deck nicht gehoert (eine gewoehnliche Shell).
    assert _dead([["A1", T1]], [T1, T2]) == []


def test_ohne_auskunft_wird_nichts_abgeraeumt():
    # Kommt statt der Terminalliste nichts Brauchbares, ist das kein "alle tot":
    # lieber eine tote Kachel stehen lassen als eine lebende loeschen.
    assert _dead([["A1", T1], ["A2", T2]], None) == []


def test_eine_leere_buchfuehrung_ist_kein_fehler():
    assert _dead([], [T1]) == []
    assert _dead(None, [T1]) == []


# Ohne node laesst sich JS nicht ausfuehren. Die Tests dann rot zu machen hiesse,
# jeden Lauf auf einem Rechner ohne node zu blockieren; sie STILL bestehen zu lassen
# waere die schlimmere Luege. Also: bestehen, aber laut - die Zeile steht ueber der
# Bilanz von tests/run.py.
if not NODE:
    print("  !!  extension/detect.js UNGEPRUEFT — node ist nicht im PATH")
    for _n in [n for n in list(globals()) if n.startswith("test_")]:
        globals()[_n] = lambda: None

if __name__ == "__main__":
    for name, fn in sorted(globals().items()):
        if name.startswith("test_") and callable(fn):
            fn()
            print(f"  ok  {name}")
