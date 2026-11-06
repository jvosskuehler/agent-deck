"""Der Windows-Terminal-Host gegen Attrappen: kein wt.exe, kein Fenster, kein Prozess.

Geprueft wird, was das Panel vom Host erwartet - dieselben Antworten wie vom Broker -
und was er dafuer anfasst: welche Kommandozeile, welche Eingabe an welche PID, welcher
Prozess beendet wird.
"""
import os
import tempfile

import helpers  # noqa: F401  - legt die Repo-Wurzel auf den sys.path

from deck.domain import paths as dp
from deck.domain import protocol
from deck.domain import wt
from deck.ops.wt_host import WtHost
from deck.ops.wt_tiles import WtTiler

AREA = (0, 0, 2400, 1200)    # Arbeitsflaeche des Monitors in den Kachel-Tests (100 %)


class _Uia:
    def __init__(self, calls):
        self.calls = calls

    def find(self, slot):
        self.calls.append(("find", slot))
        return 99

    def focus(self, slot):
        self.calls.append(("uia_focus", slot))
        return 99


def _host(*, starts=None, foreground=None, tmp=None, windows=None, now=None,
          area=AREA, factor=1.0):
    """<windows>: die offenen WT-Fenster als (hwnd, Titel, minimiert); der Test darf
    die Liste unterwegs aendern. <now>: [Uhrzeit], ebenso."""
    tmp = tmp or tempfile.mkdtemp(prefix="wthost_")
    calls = []
    starts = starts if starts is not None else {}
    windows = windows if windows is not None else []
    now = now if now is not None else [1000.0]
    tiler = WtTiler(windows=lambda: list(windows),
                    place=lambda hwnd, rect: calls.append(("place", hwnd, rect)),
                    area_of=lambda hwnd: area, factor_of=lambda hwnd: factor)
    h = WtHost(hosts_path=os.path.join(tmp, "wt_hosts.json"),
               state_dir=os.path.join(tmp, "state"), uia=_Uia(calls), tiler=tiler,
               clock=lambda: now[0],
               launch=lambda argv: calls.append(("launch", argv)) or True,
               inject=lambda pid, steps: calls.append(("inject", pid, steps)) or True,
               start_of=lambda pid: starts.get(pid),
               kill=lambda pid: calls.append(("kill", pid)) or True,
               focus_window=lambda hwnd: calls.append(("focus_window", hwnd)),
               foreground=foreground or (lambda: ("", "")),
               wt_exe="wt.exe", shell="pwsh.exe", profile=wt.PROFILE, sync=True)
    return h, calls, tmp


def _pid(h, slot, pid, created):
    dp.save_json(os.path.join(h.state_dir, slot + wt.PID_SUFFIX),
                 {"pid": pid, "created": created, "slot": slot})


def _launches(calls):
    return [c[1] for c in calls if c[0] == "launch"]


def test_ein_wt_block_gilt_auch_ohne_pane_als_verbunden():
    h, _, _ = _host()
    h.open_repo("C", r"C:\Repos\alpha")
    assert h.connected("C") and h.terminals("C") == []
    assert not h.connected("D")


def test_bloecke_ueberleben_den_neustart_des_panels():
    h, _, tmp = _host()
    h.open_repo("C", r"C:\Repos\alpha")
    h2, _, _ = _host(tmp=tmp)
    assert h2.has("C") and h2.hosts["C"] == r"C:\Repos\alpha"


def test_nur_panes_mit_lebender_shell_zaehlen_und_tote_pid_dateien_verschwinden():
    h, _, _ = _host(starts={11: 1000, 13: 9_000_000_000})
    h.open_repo("C", r"C:\r")
    _pid(h, "C1", 11, 1000)               # lebt
    _pid(h, "C2", 12, 1000)               # Prozess weg
    _pid(h, "C3", 13, 1000)               # PID neu vergeben (andere Startzeit)
    assert h.terminals("C") == ["C1"]
    rest = sorted(os.listdir(h.state_dir))
    assert rest == ["C1" + wt.PID_SUFFIX]


def test_der_erste_agent_oeffnet_sein_eigenes_fenster():
    h, calls, _ = _host()
    h.open_repo("C", r"C:\Repos\alpha")
    assert h.create("C", "opus")
    (argv,) = _launches(calls)
    assert argv[:4] == ["wt.exe", "-w", "deck-C1", "new-tab"]
    assert argv[argv.index("--title") + 1] == "C1"
    assert argv[argv.index("-d") + 1] == r"C:\Repos\alpha"
    spec = wt.decode_spec(argv[-1])
    assert spec["slot"] == "C1" and spec["args"] == ["--model", "opus"]


def test_jeder_weitere_agent_bekommt_ein_fenster_statt_eines_halben_panes():
    h, calls, _ = _host(starts={11: 5})
    h.open_repo("C", r"C:\r")
    _pid(h, "C1", 11, 5)
    h.create("C")
    argv = _launches(calls)[0]
    assert argv[:4] == ["wt.exe", "-w", "deck-C2", "new-tab"] and "split-pane" not in argv
    assert not any(c[0] in ("find", "uia_focus") for c in calls)


def _placed(calls):
    return [(c[1], c[2]) for c in calls if c[0] == "place"]


def test_kommt_ein_agent_dazu_werden_alle_fenster_des_blocks_gleich_gross():
    wins = [(101, "C1", False), (102, "C2", False), (201, "D1", False)]
    now = [1000.0]
    h, calls, _ = _host(starts={11: 1, 12: 1, 13: 1}, windows=wins, now=now)
    h.open_repo("C", r"C:\r")
    _pid(h, "C1", 11, 1)
    _pid(h, "C2", 12, 1)
    h.terminals("C")
    assert _placed(calls) == [(101, (0, 0, 1200, 1200)), (102, (1200, 0, 1200, 1200))]

    calls.clear()
    _pid(h, "C3", 13, 1)
    wins.append((103, "C3", False))
    now[0] += 1
    h.terminals("C")
    assert _placed(calls) == [(101, (0, 0, 800, 1200)), (102, (800, 0, 800, 1200)),
                              (103, (1600, 0, 800, 1200))]


def test_auf_dem_laptop_werden_drei_agenten_zu_kacheln_statt_schmaler_streifen():
    wins = [(101, "C1", False), (102, "C2", False), (103, "C3", False)]
    h, calls, _ = _host(starts={11: 1, 12: 1, 13: 1}, windows=wins,
                        area=(0, 0, 1920, 1140), factor=1.25)
    h.open_repo("C", r"C:\r")
    for slot, pid in (("C1", 11), ("C2", 12), ("C3", 13)):
        _pid(h, slot, pid, 1)
    h.terminals("C")
    assert _placed(calls) == [(101, (0, 0, 960, 570)), (102, (960, 0, 960, 570)),
                              (103, (0, 570, 960, 570))]


def test_ohne_neuen_oder_gegangenen_agenten_bleiben_die_fenster_liegen():
    """Wer ein Fenster von Hand verschiebt, soll es beim naechsten Takt nicht
    zurueckgerissen bekommen."""
    now = [1000.0]
    h, calls, _ = _host(starts={11: 1}, windows=[(101, "C1", False)], now=now)
    h.open_repo("C", r"C:\r")
    _pid(h, "C1", 11, 1)
    h.terminals("C")
    calls.clear()
    now[0] += 1
    h.terminals("C")
    assert _placed(calls) == []


def test_geht_ein_agent_ruecken_die_uebrigen_zusammen():
    wins = [(101, "C1", False), (102, "C2", False)]
    now = [1000.0]
    starts = {11: 1, 12: 1}
    h, calls, _ = _host(starts=starts, windows=wins, now=now)
    h.open_repo("C", r"C:\r")
    _pid(h, "C1", 11, 1)
    _pid(h, "C2", 12, 1)
    h.terminals("C")
    calls.clear()
    del starts[11]                      # Shell von C1 beendet
    wins.pop(0)
    now[0] += 1
    assert h.terminals("C") == ["C2"]
    assert _placed(calls) == [(102, (0, 0, 2400, 1200))]


def test_minimierte_fenster_werden_nicht_zurueck_in_die_kachel_gezogen():
    wins = [(101, "C1", True), (102, "C2", False)]
    h, calls, _ = _host(starts={11: 1, 12: 1}, windows=wins)
    h.open_repo("C", r"C:\r")
    _pid(h, "C1", 11, 1)
    _pid(h, "C2", 12, 1)
    h.terminals("C")
    assert _placed(calls) == [(102, (0, 0, 2400, 1200))]


def test_zwei_schnelle_klicks_bekommen_verschiedene_namen():
    h, calls, _ = _host()
    h.open_repo("C", r"C:\r")
    h.create("C")
    h.create("C")
    titles = [a[a.index("--title") + 1] for a in _launches(calls)]
    assert titles == ["C1", "C2"]


def test_neustart_bekommt_wunschname_verzeichnis_und_flags():
    h, calls, _ = _host()
    h.open_repo("C", r"C:\r")
    h.create("C", slot="C5", args='--resume "x" --model "opus[1m]"', cwd=r"C:\wt\t1")
    argv = _launches(calls)[0]
    assert argv[argv.index("--title") + 1] == "C5"
    assert argv[argv.index("-d") + 1] == r"C:\wt\t1"
    assert wt.decode_spec(argv[-1])["args"] == ["--resume", "x", "--model", "opus[1m]"]


def test_ein_unbekannter_buchstabe_bekommt_keinen_agenten():
    h, calls, _ = _host()
    assert not h.create("Z") and not _launches(calls)


def test_text_geht_als_eingabe_an_die_shell_des_panes():
    h, calls, _ = _host(starts={4242: 7})
    h.open_repo("C", r"C:\r")
    _pid(h, "C1", 4242, 7)
    assert h.send_window("C", {"cmd": protocol.CMD_SEND, "slot": "C1",
                               "text": "los", "execute": False, "submit": True})
    assert ("inject", 4242, wt.send_steps("los", submit=True)) in calls


def test_der_startmodus_kommt_als_shift_tab_an():
    h, calls, _ = _host(starts={4242: 7})
    h.open_repo("C", r"C:\r")
    _pid(h, "C1", 4242, 7)
    assert h.send_window("C", {"cmd": protocol.CMD_KEY, "slot": "C1",
                               "key": "shift-tab", "repeat": 3})
    assert ("inject", 4242, wt.key_steps("shift-tab", 3)) in calls


def test_ohne_lebendes_pane_wird_nichts_eingegeben():
    h, calls, _ = _host()
    h.open_repo("C", r"C:\r")
    assert not h.send_window("C", {"cmd": protocol.CMD_SEND, "slot": "C9", "text": "x"})
    assert not any(c[0] == "inject" for c in calls)


def test_fokus_holt_das_eigene_fenster_des_agenten_nach_vorn():
    h, calls, _ = _host(starts={11: 1}, windows=[(101, "C1", True)])
    h.open_repo("C", r"C:\r")
    _pid(h, "C1", 11, 1)
    assert h.send_window("C", {"cmd": protocol.CMD_FOCUS_PANE, "slot": "C1"})
    assert [c for c in calls if c[0] != "place"] == [("focus_window", 101)]


def test_ein_pane_im_alten_geteilten_fenster_sucht_weiter_die_ui_automation():
    h, calls, _ = _host(starts={11: 1})
    h.open_repo("C", r"C:\r")
    _pid(h, "C1", 11, 1)
    assert h.send_window("C", {"cmd": protocol.CMD_FOCUS_PANE, "slot": "C1"})
    assert [c[0] for c in calls] == ["find", "focus_window", "uia_focus"]


def test_klick_auf_den_reponamen_holt_alle_fenster_des_blocks_nach_vorn():
    wins = [(101, "C1", False), (102, "C2", False)]
    h, calls, _ = _host(starts={11: 1, 12: 1}, windows=wins)
    h.open_repo("C", r"C:\r")
    _pid(h, "C1", 11, 1)
    _pid(h, "C2", 12, 1)
    assert h.raise_window("C")
    assert [c[1] for c in calls if c[0] == "focus_window"] == [102, 101]


def test_schliessen_beendet_den_baum_und_raeumt_die_pid_datei_weg():
    h, calls, _ = _host(starts={11: 1})
    h.open_repo("C", r"C:\r")
    _pid(h, "C1", 11, 1)
    assert h.send_window("C", {"cmd": protocol.CMD_CLOSE_AGENT, "slot": "C1"})
    assert ("kill", 11) in calls
    assert not os.path.exists(os.path.join(h.state_dir, "C1" + wt.PID_SUFFIX))


def test_das_ganze_fenster_schliessen_beendet_jeden_agenten_des_blocks():
    h, calls, _ = _host(starts={11: 1, 12: 1, 21: 1})
    h.open_repo("C", r"C:\r")
    h.open_repo("D", r"C:\s")
    _pid(h, "C1", 11, 1)
    _pid(h, "C2", 12, 1)
    _pid(h, "D1", 21, 1)
    h.send_window("C", {"cmd": protocol.CMD_CLOSE_WINDOW})
    assert sorted(c[1] for c in calls if c[0] == "kill") == [11, 12]


def test_vs_code_kommandos_sind_hier_leere_handgriffe():
    h, calls, _ = _host()
    h.open_repo("C", r"C:\r")
    assert h.send_window("C", {"cmd": protocol.CMD_RELOAD})
    assert h.send_window("C", {"cmd": protocol.CMD_SAVE_ALL})
    assert calls == []


def test_vergessen_laesst_die_panes_laufen():
    h, calls, tmp = _host(starts={11: 1})
    h.open_repo("C", r"C:\r")
    _pid(h, "C1", 11, 1)
    assert h.forget("C") and not h.has("C")
    assert not any(c[0] == "kill" for c in calls)
    assert dp.load_json(os.path.join(tmp, "wt_hosts.json")) == {}


def test_das_vorn_liegende_pane_wird_einmal_je_wechsel_gemeldet():
    fg = {"now": (wt.WINDOW_CLASS, "C1")}
    h, _, _ = _host(foreground=lambda: fg["now"])
    h.open_repo("C", r"C:\r")
    assert h.drain_seen() == {"C1"}
    assert h.drain_seen() == set()            # derselbe Blick zaehlt nicht doppelt
    fg["now"] = ("Chrome_WidgetWin_1", "C1")  # anderes Programm mit passendem Titel
    assert h.drain_seen() == set()
    fg["now"] = (wt.WINDOW_CLASS, "C1")
    assert h.drain_seen() == {"C1"}           # wieder hingeschaut
    fg["now"] = (wt.WINDOW_CLASS, "Z1")       # Pane eines fremden Buchstabens
    assert h.drain_seen() == set()


def test_nur_titel_eines_eigenen_wt_blocks_gelten_als_agentenfenster():
    h, _, _ = _host()
    h.open_repo("C", r"C:\r")
    assert h.slot_of_title("C1") == "C1"
    assert h.slot_of_title("Z1") is None          # Buchstabe ohne WT-Block
    assert h.slot_of_title("PowerShell") is None  # kein Slot-Titel
    assert h.slot_of_title(None) is None
