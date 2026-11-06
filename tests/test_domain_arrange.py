"""Die Regeln hinter „VS-Code-Fenster einsammeln".

Zwei Entscheidungen, und beide können dem Nutzer wehtun, wenn sie falsch fallen: WEN
der Knopf anfasst (ein Fehlgriff verschiebt ein fremdes Fenster) und WOHIN genau (ein
Rechteck ausserhalb der Arbeitsfläche legt das Fenster halb hinter die Taskleiste oder
gleich auf den Nachbarmonitor).

Die Titel hier sind echt abgeschrieben - mit dem Firefox-Fenster, das „Visual Studio
Code" im Titel trug, während sechs VS-Code-Fenster offen waren.
"""
import helpers  # noqa: F401  - legt die Repo-Wurzel auf den sys.path

from deck.domain import arrange as ar

CODE = r"C:\Users\jemand\AppData\Local\Programs\Microsoft VS Code\Code.exe"
FIREFOX = r"C:\Program Files\Mozilla Firefox\firefox.exe"

# Ein echter VS-Code-Fensterstil und ein echter Dialogstil (GWL_STYLE, unsigned).
STYLE_FENSTER = 0x15C70000      # WS_MAXIMIZEBOX gesetzt
STYLE_DIALOG = 0x94C80AC4       # ohne WS_MAXIMIZEBOX

VOLL_HD = (0, 0, 1920, 1040)    # Arbeitsfläche: 1080 minus Taskleiste


# ── Wen der Knopf anfasst ────────────────────────────────────────────────────

def test_der_prozess_entscheidet_nicht_der_titel():
    """Ein Browser mit „Visual Studio Code" im Titel bleibt liegen.

    Genau dieser Fall stand offen, als der Knopf entstand: ein Firefox auf der
    VS-Code-Doku. Am Titel gemessen wäre er mitgewandert und maximiert worden.
    """
    assert not ar.is_editor_window(
        FIREFOX, "Getting Started - Visual Studio Code - Mozilla Firefox")


def test_ein_eigener_fenstertitel_versteckt_das_fenster_nicht():
    """VS Codes `window.title` ist einstellbar - der Marker kann ganz fehlen."""
    assert ar.is_editor_window(CODE, "nur-mein-repo")


def test_gross_klein_und_pfad_sind_egal():
    assert ar.is_editor_window(r"D:\vscode\CODE.EXE", "")
    assert ar.is_editor_window("code.exe", "")


def test_insiders_und_vscodium_zaehlen_mit():
    assert ar.is_editor_window(r"C:\VS Code Insiders\Code - Insiders.exe", "")
    assert ar.is_editor_window(r"C:\VSCodium\codium.exe", "")


def test_ohne_prozesspfad_entscheidet_der_titel():
    """Windows gibt den Pfad nicht immer heraus - dann ist der Titel der Notnagel."""
    assert ar.is_editor_window("", "settings.json - agent-deck - Visual Studio Code")
    assert not ar.is_editor_window("", "Orpheus Client")
    assert not ar.is_editor_window("", "")


def test_ein_dialog_wird_nicht_maximiert():
    """Ohne WS_MAXIMIZEBOX ist Maximieren keine Aufräumhilfe, sondern graue Fläche."""
    assert ar.is_maximizable(STYLE_FENSTER)
    assert not ar.is_maximizable(STYLE_DIALOG)


# ── Wohin genau ──────────────────────────────────────────────────────────────

def test_das_wiederherstellungs_rechteck_liegt_zentriert():
    x, y, w, h = ar.normal_rect(1000, 600, VOLL_HD)
    assert (w, h) == (1000, 600)
    assert (x, y) == ((1920 - 1000) // 2, (1040 - 600) // 2)


def test_vom_grossen_monitor_gemerkte_groesse_wird_geklemmt():
    """4K-Größe auf einem Full-HD-Schirm: das Fenster darf nicht überstehen.

    Das ist der Weg, den der Knopf oft geht - die Fenster kommen vom grossen Monitor.
    """
    x, y, w, h = ar.normal_rect(3800, 2000, VOLL_HD)
    assert (x, y, w, h) == (0, 0, 1920, 1040)


def test_das_rechteck_bleibt_ganz_auf_der_arbeitsflaeche():
    """Auch auf einem Monitor LINKS des Hauptschirms (negative Koordinaten)."""
    area = (-2560, -300, 0, 1140)
    for size in ((800, 600), (5000, 5000), (0, 0)):
        x, y, w, h = ar.normal_rect(*size, area)
        assert area[0] <= x and x + w <= area[2], size
        assert area[1] <= y and y + h <= area[3], size


def test_eine_kaputte_normalgroesse_wird_nicht_zum_punkt():
    """rcNormalPosition kann 0x0 sein - dann ist das Fenster nach dem
    Wiederherstellen unauffindbar, und genau dieses Rechteck bekommt der Nutzer beim
    nächsten Klick auf „Wiederherstellen" zu sehen."""
    _, _, w, h = ar.normal_rect(0, 0, VOLL_HD)
    assert (w, h) == (ar.MIN_W, ar.MIN_H)


def test_eine_winzige_arbeitsflaeche_schlaegt_die_untergrenze():
    """Passt die Untergrenze nicht auf den Schirm, gewinnt der Schirm - sonst hängt
    das Fenster über dem Rand."""
    _, _, w, h = ar.normal_rect(0, 0, (0, 0, 320, 240))
    assert (w, h) == (320, 240)


# ── Kachelraster der Windows-Terminal-Agenten ───────────────────────────────

def test_bis_vier_agenten_stehen_gleich_breit_nebeneinander():
    kacheln = ar.tile_grid(3, (0, 0, 1200, 800))
    assert kacheln == [(0, 0, 400, 800), (400, 0, 400, 800), (800, 0, 400, 800)]


def test_ab_fuenf_agenten_gibt_es_reihen_mit_gleich_grossen_kacheln():
    kacheln = ar.tile_grid(5, (0, 0, 1200, 800))
    assert len(kacheln) == 5
    assert {(w, h) for _, _, w, h in kacheln} == {(400, 400)}
    assert [(x, y) for x, y, _, _ in kacheln] == [
        (0, 0), (400, 0), (800, 0), (0, 400), (400, 400)]


def test_die_kacheln_fuellen_die_flaeche_ohne_luecke_auch_bei_krummen_massen():
    area = (3840, 0, 3840 + 2561, 1397)
    kacheln = ar.tile_grid(3, area)
    for (x, _, w, _), (nx, _, _, _) in zip(kacheln, kacheln[1:]):
        assert x + w == nx
    x, _, w, h = kacheln[-1]
    assert x + w == area[2] and h == 1397
    assert max(k[2] for k in kacheln) - min(k[2] for k in kacheln) <= 1


def test_ein_einzelner_agent_bekommt_die_ganze_flaeche_und_keiner_nichts():
    assert ar.tile_grid(1, (10, 20, 110, 220)) == [(10, 20, 100, 200)]
    assert ar.tile_grid(0, (0, 0, 100, 100)) == []


def test_wie_viele_nebeneinander_haengt_an_breite_und_skalierung():
    assert ar.max_cols_for(3840, 1.5) == 4      # 4K bei 150 %
    assert ar.max_cols_for(2560, 1.0) == 4
    assert ar.max_cols_for(1920, 1.0) == 3
    assert ar.max_cols_for(1920, 1.25) == 2     # Laptop
    assert ar.max_cols_for(800, 2.0) == 1       # nie null Spalten
    assert ar.max_cols_for(10_000, 1.0) == ar.MAX_COLS
