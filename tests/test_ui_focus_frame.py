"""Der Fokus-Rahmen um das vorderste WT-Agentenfenster (ui/focus_frame.py), mit
Attrappen statt echter Fenster."""
import helpers  # noqa: F401  - legt die Repo-Wurzel auf den sys.path

from deck.domain import wt
from deck.ui.focus_frame import FocusFrame

R1 = (100, 0, 500, 800)


class _Win:
    def __init__(self):
        self.fg = 0
        self.wins = {}          # hwnd -> (Klasse, Titel, minimiert, Rechteck)
        self.log = []
        self.show_ok = True

    def foreground(self):
        return self.fg

    def describe(self, hwnd):
        c, t, _, _ = self.wins.get(hwnd, ("", "", False, None))
        return c, t

    def minimized(self, hwnd):
        return self.wins[hwnd][2]

    def visible_rect(self, hwnd):
        return self.wins[hwnd][3]

    def factor(self, hwnd):
        return 1.0

    def windows(self):
        return [(h, v[1], v[2]) for h, v in self.wins.items() if v[0] == wt.WINDOW_CLASS]

    def square(self, hwnd):
        self.log.append(("square", hwnd))

    def create(self):
        self.log.append(("create",))
        return 900

    def show(self, ov, rect, bits):
        self.log.append(("show", rect, len(bits)))
        return self.show_ok

    def move(self, ov, x, y):
        self.log.append(("move", x, y))
        return True

    def hide(self, ov):
        self.log.append(("hide",))


def _frame():
    win = _Win()
    win.wins = {1: (wt.WINDOW_CLASS, "C1", False, R1),
                2: (wt.WINDOW_CLASS, "C2", False, (500, 0, 900, 800)),
                3: (wt.WINDOW_CLASS, "PowerShell", False, (0, 0, 10, 10)),
                4: ("Chrome_WidgetWin_1", "C1", False, (0, 0, 10, 10))}
    return FocusFrame(None, lambda t: t in ("C1", "C2"), win=win), win


def _acts(win, kind):
    return [e for e in win.log if e[0] == kind]


def test_rahmen_steht_genau_auf_dem_sichtbaren_rand_des_vorderen_agenten():
    f, win = _frame()
    win.fg = 1
    f.step()
    assert _acts(win, "show") == [("show", R1, 400 * 800 * 4)]


def test_fremde_fenster_bekommen_keinen_rahmen():
    for fg in (3, 4, 0):                  # WT ohne Slot, Chrome mit Slot-Titel, nichts
        f, win = _frame()
        win.fg = fg
        f.step()
        assert not _acts(win, "show") and not _acts(win, "create"), fg


def test_fokuswechsel_weg_vom_agenten_versteckt_den_rahmen():
    f, win = _frame()
    win.fg = 1
    f.step()
    win.fg = 4
    f.step()
    assert win.log[-1] == ("hide",)


def test_ziehen_verschiebt_nur_und_zeichnet_nicht_neu():
    f, win = _frame()
    win.fg = 1
    f.step()
    win.wins[1] = (wt.WINDOW_CLASS, "C1", False, (130, 20, 530, 820))
    f.step()
    assert len(_acts(win, "show")) == 1
    assert _acts(win, "move") == [("move", 130, 20)]


def test_minimiertes_agentenfenster_hat_keinen_rahmen():
    f, win = _frame()
    win.wins[1] = (wt.WINDOW_CLASS, "C1", True, R1)
    win.fg = 1
    f.step()
    assert not _acts(win, "show")


def test_alle_agentenfenster_werden_eckig_aber_nur_einmal_und_nur_bei_fokuswechsel():
    f, win = _frame()
    win.fg = 1
    f.step()
    f.step()                              # gleicher Vordergrund: kein neuer Durchgang
    win.fg = 2
    f.step()
    assert sorted(_acts(win, "square")) == [("square", 1), ("square", 2)]


def test_gescheitertes_zeigen_wird_nicht_im_takt_wiederholt():
    f, win = _frame()
    win.show_ok = False
    win.fg = 1
    f.step()
    f.step()
    assert len(_acts(win, "show")) == 1
    win.wins[1] = (wt.WINDOW_CLASS, "C1", False, (0, 0, 300, 300))   # neues Ziel
    f.step()
    assert len(_acts(win, "show")) == 2


def test_das_rahmenfenster_entsteht_erst_beim_ersten_agenten_und_nur_einmal():
    f, win = _frame()
    f.step()
    assert not _acts(win, "create")
    win.fg = 1
    f.step()
    win.fg = 2
    f.step()
    assert _acts(win, "create") == [("create",)]
