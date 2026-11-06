"""Fenster und Bindungen pflegen (ui/windows.py): der Heimat-Block."""
import os
import tempfile

import helpers  # noqa: F401  - legt die Repo-Wurzel auf den sys.path

from deck.ops.wt_host import WtHost
from deck.ui.windows import WindowSyncMixin


class _Store:
    def __init__(self):
        self.saved = 0

    def save_bindings(self):
        self.saved += 1


def _deck(home, bindings=None, tmp=None):
    tmp = tmp or tempfile.mkdtemp(prefix="home_")
    d = WindowSyncMixin()
    d.settings = {"terminal_home": home}
    d.bindings = dict(bindings or {})
    d.store = _Store()
    d.wt = WtHost(hosts_path=os.path.join(tmp, "wt_hosts.json"),
                  state_dir=os.path.join(tmp, "state"), uia=object(),
                  wt_exe="wt.exe", shell="pwsh.exe", profile=None, sync=True)
    return d, tmp


def test_der_heimat_block_ist_beim_start_da_ohne_agenten():
    home = tempfile.mkdtemp(prefix="Repos_")
    d, _ = _deck(home, {"A": "orpheus-management-api"})
    d._ensure_home_terminal()
    assert d.home_win == "B"                       # erster freier Buchstabe
    assert d.bindings["B"] == os.path.basename(home)
    assert d.wt.has("B") and d.wt.terminals("B") == []
    assert d.bindings["A"] == "orpheus-management-api"


def test_ein_vorhandener_heimat_block_behaelt_seinen_buchstaben():
    home = tempfile.mkdtemp(prefix="Repos_")
    d, tmp = _deck(home)
    d._ensure_home_terminal()
    first = d.home_win
    d2, _ = _deck(home, {"C": "x"}, tmp=tmp)      # Neustart, inzwischen anders belegt
    d2.bindings[first] = os.path.basename(home)
    d2._ensure_home_terminal()
    assert d2.home_win == first and len(d2.wt.hosts) == 1


def test_ohne_heimat_ordner_gibt_es_keinen_heimat_block():
    d, _ = _deck(os.path.join(tempfile.gettempdir(), "gibt-es-nicht-xyz"))
    d._ensure_home_terminal()
    assert d.home_win is None and d.bindings == {} and d.wt.hosts == {}


def test_bypass_als_startmodus_schiebt_bypass_in_den_zyklus_und_vergisst_alte_nummern():
    """Die gemerkten Modi sind Nummern im Zyklus. Kommt Bypass dazu, hiesse die
    alte 3 ('auto') ploetzlich 'bypass' - also vergessen, die Hooks liefern neu."""
    from deck.claude import settings as cset
    d = WindowSyncMixin()
    d.slot_mode, d._mode_ts = {"A1": 3}, {"A1": 5.0}
    alt = cset.default_mode
    try:
        cset.default_mode = lambda path=None: "auto"
        assert d._mode_cycle() == ["manual", "accept", "plan", "auto"]
        assert d.slot_mode == {"A1": 3}                # gleicher Zyklus -> nichts vergessen
        cset.default_mode = lambda path=None: cset.BYPASS_MODE
        assert d._mode_cycle() == ["manual", "accept", "plan", "bypass", "auto"]
        assert d.slot_mode == {} and d._mode_ts == {}
    finally:
        cset.default_mode = alt
