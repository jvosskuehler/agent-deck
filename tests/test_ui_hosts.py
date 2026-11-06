"""Der Verteiler zwischen VS Code und Windows Terminal (ui/hosts.py)."""
import helpers  # noqa: F401  - legt die Repo-Wurzel auf den sys.path

from deck.ui.hosts import HostRouter


class _Host:
    def __init__(self, name, letters=(), seen=()):
        self.name, self.letters, self.seen, self.log = name, set(letters), set(seen), []

    def has(self, win):
        return win in self.letters

    def send_window(self, win, obj):
        self.log.append(("send", win))
        return True

    def connected(self, win):
        return True

    def remote(self, win):
        return self.name

    def terminals(self, win):
        return [f"{win}1-{self.name}"]

    def assign(self, ws, win):
        self.log.append(("assign", ws, win))
        return True

    def forget(self, win):
        self.log.append(("forget", win))
        return True

    def drain_seen(self):
        return set(self.seen)

    def workspaces(self):
        return [self.name]

    def workspace_slots(self, ws):
        return [ws]

    def client_info(self):
        return [{"window": "A", "host": self.name}]

    def start(self):
        self.log.append(("start",))

    def stop(self):
        self.log.append(("stop",))


def _router():
    vs, wt = _Host("vscode"), _Host("wt", letters={"C"})
    return HostRouter(vs, wt), vs, wt


def test_wt_buchstaben_gehen_an_windows_terminal_alle_anderen_an_vs_code():
    r, vs, wt = _router()
    assert r.terminals("C") == ["C1-wt"] and r.terminals("A") == ["A1-vscode"]
    r.send_window("C", {})
    r.send_window("A", {})
    assert wt.log == [("send", "C")] and vs.log == [("send", "A")]
    assert r.is_wt("C") and not r.is_wt("A") and not r.is_wt(None)


def test_ein_wt_buchstabe_wird_nie_an_ein_vs_code_fenster_vergeben():
    r, vs, _ = _router()
    assert not r.assign("alpha", "C")
    assert vs.log == []
    assert r.assign("beta", "A") and vs.log == [("assign", "beta", "A")]


def test_vergessen_trifft_den_richtigen_host():
    r, vs, wt = _router()
    r.forget("C")
    r.forget("A")
    assert wt.log == [("forget", "C")] and vs.log == [("forget", "A")]


def test_gesehen_kommt_aus_beiden_hosts():
    vs, wt = _Host("vscode", seen={"A1"}), _Host("wt", letters={"C"}, seen={"C2"})
    assert HostRouter(vs, wt).drain_seen() == {"A1", "C2"}


def test_auto_binden_und_neustart_kennen_nur_vs_code():
    r, _, _ = _router()
    assert r.workspaces() == ["vscode"]
    assert r.client_info() == [{"window": "A", "host": "vscode"}]


def test_start_und_stop_erreichen_beide_hosts():
    r, vs, wt = _router()
    r.start()
    r.stop()
    assert ("start",) in vs.log and ("stop",) in vs.log
    assert ("start",) in wt.log and ("stop",) in wt.log
