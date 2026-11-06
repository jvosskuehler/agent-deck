"""Die Bilanz von „VS-Code-Fenster einsammeln".

Was hier gezählt wird, liest der Nutzer als Satz in der Statuszeile des Dialogs - ein
Zahlendreher ist darum keine Kosmetik, sondern eine falsche Auskunft („6 Fenster geholt",
während keines umgezogen ist). Die Auswahlregeln selbst stehen in test_domain_arrange;
hier geht es um die VERDRAHTUNG: dass beide Regeln zusammen gelten und dass die
Rückgabe zum tatsächlich Geschehenen passt.

Win32 wird durch Attrappen ersetzt - der Test verschiebt keine Fenster.
"""
import helpers  # noqa: F401  - legt die Repo-Wurzel auf den sys.path

from deck.ops import window_gather as wg
from deck.platform.arrange import Win

AREA = (0, 0, 1920, 1040)
CODE = r"C:\Programs\Microsoft VS Code\Code.exe"
FIREFOX = r"C:\Program Files\Mozilla Firefox\firefox.exe"
FENSTER = 0x15C70000        # mit WS_MAXIMIZEBOX
DIALOG = 0x94C80AC4         # ohne

# Ein realistischer Fenster-Zoo: zwei VS-Code-Fenster, ein Browser mit dem Marker im
# Titel, ein VS-Code-Dialog und das Deck selbst.
ZOO = [
    Win(11, "a.py - repo-eins - Visual Studio Code", CODE, FENSTER),
    Win(12, "Getting Started - Visual Studio Code - Firefox", FIREFOX, FENSTER),
    Win(13, "b.py - repo-zwei - Visual Studio Code", CODE, FENSTER),
    Win(14, "Änderungen speichern?", CODE, DIALOG),
    Win(15, "Agent Deck", r"C:\Python\pythonw.exe", FENSTER),
]


class _Fake:
    """Ersetzt die Win32-Aufrufe und schreibt mit, was verschoben WUERDE."""

    def __init__(self, wins=ZOO, area=AREA, klappt=True, size=(1000, 700)):
        self.wins, self.area, self.klappt, self.size = wins, area, klappt, size
        self.moved: list[tuple[int, tuple[int, int, int, int]]] = []

    def __enter__(self):
        self._alt = (wg.wa.visible_windows, wg.wa.normal_size, wg.wa.move_maximized,
                     wg.monitor.primary_work_area)
        wg.wa.visible_windows = lambda: list(self.wins)
        wg.wa.normal_size = lambda hwnd: self.size
        wg.monitor.primary_work_area = lambda: self.area

        def move(hwnd, rect):
            ok = self.klappt(hwnd) if callable(self.klappt) else self.klappt
            if ok:
                self.moved.append((hwnd, rect))
            return ok

        wg.wa.move_maximized = move
        return self

    def __exit__(self, *exc):
        (wg.wa.visible_windows, wg.wa.normal_size, wg.wa.move_maximized,
         wg.monitor.primary_work_area) = self._alt
        return False


def test_nur_maximierbare_vs_code_fenster_werden_eingesammelt():
    """Beide Regeln gelten zusammen: Prozess UND Maximierbarkeit.

    Fällt eine weg, wandert entweder der Browser mit oder ein Speichern-Dialog wird
    auf Bildschirmgröße gezogen.
    """
    with _Fake():
        assert [w.hwnd for w in wg.editor_windows()] == [11, 13]


def test_die_bilanz_nennt_umgezogene_und_gefundene():
    with _Fake() as f:
        done, total, err = wg.gather_to_primary()
    assert (done, total, err) == (2, 2, None)
    assert [hwnd for hwnd, _ in f.moved] == [11, 13]


def test_jedes_fenster_bekommt_ein_rechteck_auf_dem_zielmonitor():
    """Nicht bloss „irgendwohin": das Rechteck liegt ganz auf der Arbeitsfläche - sonst
    maximiert Windows auf dem Nachbarmonitor."""
    with _Fake() as f:
        wg.gather_to_primary()
    for _, (x, y, w, h) in f.moved:
        assert AREA[0] <= x and x + w <= AREA[2]
        assert AREA[1] <= y and y + h <= AREA[3]


def test_ein_gescheitertes_fenster_wird_gemeldet_nicht_verschwiegen():
    """Teilerfolg ist der heikle Fall: die Zahl stimmt, aber der Nutzer sucht sein
    sechstes Fenster. Also gehört der Rest in die Meldung."""
    with _Fake(klappt=lambda hwnd: hwnd == 11):
        done, total, err = wg.gather_to_primary()
    assert (done, total) == (1, 2)
    assert err and "1" in err


def test_ohne_vs_code_fenster_gibt_es_eine_meldung_statt_einer_null():
    """„0 Fenster geholt" ohne Grund lässt den Nutzer den Knopf für kaputt halten."""
    with _Fake(wins=[ZOO[1], ZOO[4]]):
        done, total, err = wg.gather_to_primary()
    assert (done, total) == (0, 0)
    assert err


def test_ohne_hauptmonitor_wird_nichts_verschoben():
    """Kein Arbeitsbereich (kein Windows, kaputte Anzeige) -> Fehler, aber kein Umzug
    auf ein geratenes Rechteck."""
    with _Fake(area=None) as f:
        done, total, err = wg.gather_to_primary()
    assert (done, total) == (0, 0) and err
    assert f.moved == []
