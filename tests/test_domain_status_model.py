"""status_model: Statusinterpretation ohne Anzeige.
"""

import helpers  # noqa: F401 - Import MIT Absicht: legt die Repo-Wurzel auf den
from helpers import _CYCLE, _GLOW

# sys.path und nagelt die Deck-Sprache auf Deutsch.
from deck.dock import metrics as dockm
from deck.domain import status_model as sm
from deck.render import kit as ck


def test_is_fresh():
    assert sm.is_fresh({"ts": 100}, 100, 900) is True
    assert sm.is_fresh({"ts": 100}, 1001, 900) is False
    assert sm.is_fresh(None, 100, 900) is False


def test_normalize_status():
    assert sm.normalize_status("thinking", False, _GLOW) == "idle"   # eingeschlafen
    assert sm.normalize_status("thinking", True, _GLOW) == "thinking"
    assert sm.normalize_status("running", False, _GLOW) == "idle"
    assert sm.normalize_status("waiting", False, _GLOW) == "waiting"  # nicht thinking/running
    assert sm.normalize_status("bogus", True, _GLOW) == "idle"        # unbekannt
    assert sm.normalize_status("none", True, _GLOW) == "none"


def test_is_lost():
    assert sm.is_lost("thinking", True, False) is True
    assert sm.is_lost("thinking", True, True) is False               # verbunden -> nicht lost
    assert sm.is_lost("none", True, False) is False                  # none nie lost
    assert sm.is_lost("idle", False, False) is False                 # nicht frisch -> nicht lost


def test_dominant_status():
    # Rangfolge fuer den Neon-Griff: Rueckfrage > ungelesen > getrennt > denkt > idle.
    assert sm.dominant_status(["idle", "thinking", "done", "waiting"]) == "waiting"
    assert sm.dominant_status(["idle", "thinking", "done"]) == "done"
    assert sm.dominant_status(["idle", "thinking", "lost"]) == "lost"
    assert sm.dominant_status(["idle", "running"]) == "thinking"   # running == denkt
    assert sm.dominant_status(["idle", "idle"]) == "idle"      # alle idle -> idle
    assert sm.dominant_status([]) == "none"                    # keine Kachel -> kein Leuchten
    assert sm.dominant_status(["none"]) == "none"
    assert sm.dominant_status(["bogus"]) == "none"             # Unbekanntes zaehlt nicht


def test_escalated():
    assert sm.escalated("idle", "waiting") is True             # dringlicher -> Blitz
    assert sm.escalated("thinking", "done") is True            # fertig geworden -> Blitz
    assert sm.escalated("done", "idle") is False               # gelesen -> kein Blitz
    assert sm.escalated("waiting", "done") is False            # ruhiger -> kein Blitz
    assert sm.escalated("done", "done") is False               # kein Wechsel
    assert sm.escalated("thinking", "running") is False        # derselbe Zustand, gleicher Rang
    assert sm.escalated("running", "thinking") is False        # ...auch andersherum


def test_neon_color_and_tint():
    """Neon-Griff (edge_dock): Farbrechnung der Röhren-Schichten, ohne Fenster."""
    AMBER = "#ffc48a"
    core_fade, halo_fade = dockm.NEON_LAYERS[-1][1], dockm.NEON_LAYERS[0][1]
    assert core_fade == 0.0 and halo_fade > 0.5      # Kern kraeftig, aussen blass
    # Volle Leuchtkraft: Kern = Statusfarbe Richtung Weiss, ohne Beimischung von HANDLE_BG.
    assert dockm.neon_color(AMBER, core_fade, 1.0) == ck.mix(AMBER, "#ffffff",
                                                          dockm.NEON_CORE_WHITE)
    # Aufblitzen (eff > 1) klemmt auf dieselbe Vollfarbe (kein Ueberlauf).
    assert dockm.neon_color(AMBER, core_fade, 1.6) == dockm.neon_color(AMBER, core_fade, 1.0)
    # Halo ist immer blasser als der Kern, aber nie ganz HANDLE_BG bei Vollausschlag.
    halo = dockm.neon_color(AMBER, halo_fade, 1.0)
    assert halo != dockm.HANDLE_BG and halo != AMBER
    # Dunkel (eff = 0) -> Schicht verschwindet in der Griff-Grundfarbe.
    assert dockm.neon_color(AMBER, halo_fade, 0.0) == dockm.HANDLE_BG
    # Unter dem Zeiger ist der Kern heller (mehr Weissanteil), aber dieselbe Familie.
    assert dockm.neon_color(AMBER, core_fade, 1.0, hot=True) != dockm.neon_color(
        AMBER, core_fade, 1.0)
    assert dockm.NEON_HOT_WHITE > dockm.NEON_CORE_WHITE
    # Grundflaeche: getaucht, aber deutlich dunkler als die Statusfarbe selbst.
    assert dockm.neon_tint(AMBER, 0.0) == dockm.HANDLE_BG
    assert dockm.neon_tint(AMBER, 1.0) == ck.mix(dockm.HANDLE_BG, AMBER, dockm.NEON_TINT)
    assert dockm.neon_tint(AMBER, 2.0) == dockm.neon_tint(AMBER, 1.0)   # geklemmt


def test_resolve_effort():
    assert sm.resolve_effort("", "ultracode") == "ultracode"
    assert sm.resolve_effort("xhigh", "ultracode") == "ultracode"    # Kollision aufgeloest
    assert sm.resolve_effort("high", "ultracode") == "high"          # echter Wert gewinnt
    assert sm.resolve_effort("xhigh", None) == "xhigh"
    assert sm.resolve_effort("", None) == ""


def test_adopt_hook_mode():
    assert sm.adopt_hook_mode(0, {"mode": "plan", "ts": 5}, _CYCLE) == (2, 5)
    assert sm.adopt_hook_mode(9, {"mode": "plan", "ts": 5}, _CYCLE) is None   # aelterer Event
    assert sm.adopt_hook_mode(0, {"mode": "bogus", "ts": 5}, _CYCLE) is None  # ungueltig
    assert sm.adopt_hook_mode(0, {"ts": 5}, _CYCLE) is None                   # kein Modus


def test_bypass_steht_im_zyklus_direkt_hinter_plan():
    assert sm.mode_cycle(_CYCLE, True) == ["manual", "accept", "plan", "bypass", "auto"]
    assert sm.mode_cycle(_CYCLE, False) == _CYCLE
    # Doppelt eingefuegt wird nie, und ohne 'plan' fehlt der Anker -> unveraendert.
    assert sm.mode_cycle(sm.mode_cycle(_CYCLE, True), True).count("bypass") == 1
    assert sm.mode_cycle(["manual", "auto"], True) == ["manual", "auto"]
    # Die Eingabe bleibt unangetastet (MODE_CYCLE ist eine Modul-Konstante).
    assert _CYCLE == ["manual", "accept", "plan", "auto"]


def test_aus_bypass_ist_auto_ein_schritt_und_manual_zwei():
    cyc = sm.mode_cycle(_CYCLE, True)
    assert sm.mode_steps(cyc.index("bypass"), "auto", cyc, "manual") == (1, 4)
    assert sm.mode_steps(cyc.index("bypass"), "manual", cyc, "manual") == (2, 0)
    assert sm.mode_steps(cyc.index("plan"), "bypass", cyc, "manual") == (1, 3)


def test_mode_steps():
    # Unbekannter aktueller Modus (None) -> vom Start-Modus 'manual' (Index 0) aus rechnen.
    assert sm.mode_steps(None, "auto", _CYCLE, "manual") == (3, 3)   # manual->accept->plan->auto
    assert sm.mode_steps(None, "plan", _CYCLE, "manual") == (2, 2)
    assert sm.mode_steps(None, "manual", _CYCLE, "manual") == (0, 0)  # schon da -> 0 Schritte
    # Gemerkter aktueller Modus gewinnt: von 'plan' (2) nach 'auto' (3) = 1 Schritt.
    assert sm.mode_steps(2, "auto", _CYCLE, "manual") == (1, 3)
    # Zyklisch: von 'auto' (3) zurueck nach 'accept' (1) = (1-3) % 4 = 2 Schritte.
    assert sm.mode_steps(3, "accept", _CYCLE, "manual") == (2, 1)
    # Ungueltiges Ziel -> None (Aufrufer schaltet nicht).
    assert sm.mode_steps(None, "bogus", _CYCLE, "manual") is None
    # Start-Modus nicht im Zyklus -> Fallback auf Index 0.
    assert sm.mode_steps(None, "plan", _CYCLE, "weird") == (2, 2)
