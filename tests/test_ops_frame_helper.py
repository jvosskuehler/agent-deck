"""Das Hilfsprogramm des Fokus-Rahmens starten und steuern (ops/frame_helper.py), mit
einem nachgemachten Prozess statt des echten Programms."""
import json

import helpers  # noqa: F401  - legt die Repo-Wurzel auf den sys.path

from deck.ops.frame_helper import FrameHelper


class _Pipe:
    def __init__(self):
        self.lines, self.closed, self.broken = [], False, False

    def write(self, data):
        if self.broken:
            raise OSError("Rohr gerissen")
        self.lines.append(json.loads(data.decode("utf-8")))

    def flush(self):
        pass

    def close(self):
        self.closed = True


class _Proc:
    def __init__(self):
        self.stdin, self.code = _Pipe(), None

    def poll(self):
        return self.code

    def wait(self, timeout=None):
        return 0

    def kill(self):
        self.code = -9


def _helper(exists=True):
    now, started = [100.0], []

    def spawn(path):
        started.append(_Proc())
        return started[-1]

    h = FrameHelper("X:/FocusFrame.exe", spawn=spawn, exists=lambda p: exists,
                    clock=lambda: now[0])
    return h, started, now


def test_kommandos_gehen_als_json_zeilen_an_ein_einmal_gestartetes_programm():
    h, started, _ = _helper()
    assert h.send({"cmd": "show", "hwnd": 5, "design": "glut", "seed": 1})
    assert h.send({"cmd": "hide"})
    assert len(started) == 1
    assert [c["cmd"] for c in started[0].stdin.lines] == ["show", "hide"]


def test_ohne_gebautes_programm_gibt_es_keinen_rahmen_und_keinen_startversuch():
    h, started, _ = _helper(exists=False)
    assert not h.send({"cmd": "hide"})
    assert not started


def test_ein_abgestuerztes_programm_wird_neu_gestartet_aber_nicht_im_dauerfeuer():
    h, started, now = _helper()
    h.send({"cmd": "hide"})
    started[0].code = 1                       # abgestuerzt
    now[0] += 1
    assert not h.send({"cmd": "hide"})        # zu frueh fuer einen Neustart
    now[0] += FrameHelper.RESTART_S
    assert h.send({"cmd": "hide"})
    assert len(started) == 2


def test_gerissenes_rohr_fuehrt_zum_neustart():
    h, started, now = _helper()
    h.send({"cmd": "hide"})
    started[0].stdin.broken = True
    assert not h.send({"cmd": "hide"})
    now[0] += FrameHelper.RESTART_S
    assert h.send({"cmd": "hide"}) and len(started) == 2


def test_stop_schliesst_stdin_damit_sich_das_programm_selbst_beendet():
    h, started, _ = _helper()
    h.send({"cmd": "hide"})
    h.stop()
    assert started[0].stdin.closed
