"""Der Fokus-Rahmen um das vorderste WT-Agentenfenster (ui/focus_frame.py): wann er
dem Hilfsprogramm was sagt, mit Attrappen statt echter Fenster."""
import random

import helpers  # noqa: F401  - legt die Repo-Wurzel auf den sys.path

from deck.domain import focus_frame as ff
from deck.domain import wt
from deck.ui.focus_frame import FocusFrame


class _Win:
    def __init__(self):
        self.fg = 0
        self.wins = {}          # hwnd -> (Klasse, Titel, minimiert)
        self.squared = []

    def foreground(self):
        return self.fg

    def describe(self, hwnd):
        c, t, _ = self.wins.get(hwnd, ("", "", False))
        return c, t

    def minimized(self, hwnd):
        return self.wins[hwnd][2]

    def windows(self):
        return [(h, v[1], v[2]) for h, v in self.wins.items() if v[0] == wt.WINDOW_CLASS]

    def square(self, hwnd):
        self.squared.append(hwnd)


class _Helper:
    def __init__(self):
        self.sent = []

    def send(self, obj):
        self.sent.append(obj)
        return True

    def stop(self):
        self.sent.append({"cmd": "stopped"})


def _frame():
    win, helper = _Win(), _Helper()
    win.wins = {1: (wt.WINDOW_CLASS, "C1", False), 2: (wt.WINDOW_CLASS, "C2", False),
                3: (wt.WINDOW_CLASS, "PowerShell", False), 4: ("Chrome_WidgetWin_1", "C1", False)}
    f = FocusFrame(lambda t: t in ("C1", "C2"), helper=helper, win=win, rnd=random.Random(7))
    return f, win, helper


def test_agent_vorn_zeigt_den_rahmen_um_genau_dieses_fenster():
    f, win, helper = _frame()
    win.fg = 1
    f.step()
    (cmd,) = helper.sent
    assert cmd["cmd"] == "show" and cmd["hwnd"] == 1 and cmd["design"] in ff.DESIGNS


def test_fremde_fenster_bekommen_keinen_rahmen():
    for fg in (3, 4, 0):                  # WT ohne Slot, Chrome mit Slot-Titel, nichts
        f, win, helper = _frame()
        win.fg = fg
        f.step()
        assert not any(c["cmd"] == "show" for c in helper.sent), fg


def test_weg_vom_agenten_versteckt_den_rahmen():
    f, win, helper = _frame()
    win.fg = 1
    f.step()
    win.fg = 4
    f.step()
    assert helper.sent[-1] == {"cmd": "hide"}


def test_minimiertes_agentenfenster_hat_keinen_rahmen():
    f, win, helper = _frame()
    win.wins[1] = (wt.WINDOW_CLASS, "C1", True)
    win.fg = 1
    f.step()
    assert not helper.sent


def test_jeder_neue_fokus_bekommt_ein_anderes_design():
    f, win, helper = _frame()
    seen = []
    for k in range(12):
        win.fg = 1 if k % 2 == 0 else 2
        f.step()
        seen.append(helper.sent[-1]["design"])
    assert all(a != b for a, b in zip(seen, seen[1:]))
    assert len(set(seen)) > 1


def test_derselbe_fokus_schickt_nichts_neues_bis_zur_wiederholung():
    f, win, helper = _frame()
    win.fg = 1
    f.step()
    for _ in range(10):
        f.step(0.06)
    assert len(helper.sent) == 1
    f.step(FocusFrame.RESEND_S)           # Stand wiederholen, gleiches Design
    assert len(helper.sent) == 2 and helper.sent[1] == helper.sent[0]


def test_alle_agentenfenster_werden_eckig_aber_nur_einmal_und_nur_bei_fokuswechsel():
    f, win, helper = _frame()
    win.fg = 1
    f.step()
    f.step()
    win.fg = 2
    f.step()
    assert sorted(win.squared) == [1, 2]


def test_stop_beendet_auch_das_hilfsprogramm():
    f, win, helper = _frame()
    f.stop()
    assert helper.sent[-1] == {"cmd": "stopped"}
