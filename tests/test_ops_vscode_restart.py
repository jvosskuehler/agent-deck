"""Der Ablauf „VS Code neu starten": Reihenfolge, Abbruchgründe, Bilanz.

Das ist die Sorte Logik, die man nicht durchprobiert - jeder Probelauf schließt die
eigene Arbeitsumgebung. Also wird sie hier gegen Attrappen gefahren: kein VS Code, kein
Broker, keine echte Uhr, keine Datei. Geprüft wird, was man später nicht mehr
nachvollziehen kann, wenn es schiefgeht:

  • Die REIHENFOLGE beim Herunterfahren (speichern, dann Terminals, dann Fenster).
  • Dass „alles zu" NACHGESEHEN und nicht angenommen wird - und dass ein „ich weiß es
    nicht" wie ein Nein behandelt wird.
  • Dass in einem Fenster erst der nächste Chat angefordert wird, wenn der vorige da
    ist - jeder entsteht als Split des vorherigen.
  • Dass der Plan nur bei einem geglückten Lauf verschwindet.
"""
import helpers  # noqa: F401  - legt die Repo-Wurzel auf den sys.path

from deck.ops import vscode_restart as vr

REPO_A = r"C:\Repos\alpha"
REPO_B = r"C:\Repos\beta"
PLAN = {
    "windows": [
        {"window": "A", "workspace": "alpha", "folder": REPO_A,
         "slots": [{"slot": "A1", "args": '--resume "x"', "cwd": REPO_A},
                   {"slot": "A2", "args": "", "cwd": REPO_A}]},
        {"window": "B", "workspace": "beta", "folder": REPO_B,
         "slots": [{"slot": "B1", "args": '--resume "y"', "cwd": REPO_B}]},
    ],
    "skipped": [],
}


class Fake:
    """Extension, Broker, Prozessliste, Uhr und Plan-Datei - alles gefälscht.

    `alive` ist die Folge der Antworten von count_exe (der letzte Wert wiederholt sich):
    so lässt sich „VS Code stirbt nach drei Blicken" ebenso hinschreiben wie „es läuft
    weiter" oder „Prozessliste nicht lesbar" (None).
    """

    def __init__(self, plan=PLAN, *, alive=(2, 1, 0), connect_after=0.0,
                 appear=True, exe=r"C:\VSCode\Code.exe", saved=None, create_ok=True):
        self.plan, self.alive, self.exe = plan, list(alive), exe
        self.connect_after = connect_after   # ab wann gelten Fenster als verbunden
        self.appear = appear                 # tauchen angeforderte Chats auf?
        self.create_ok = create_ok
        self.saved = saved                   # „liegengebliebene" Plan-Datei
        self.t = 1000.0
        self.log = []                        # jedes Ereignis, in Reihenfolge
        self.terms = {"A": [], "B": []}
        self.launched = []
        self.cleared = False
        self._queue = []                     # Chats, die beim nächsten Tick auftauchen
        self._t_start = self.t

    # ── BrokerCommands ────────────────────────────────────────────────────
    def save_all(self, win):
        self.log.append(("save", win))
        return True

    def close_agent(self, slot):
        self.log.append(("kill", slot))
        return True

    def close_window(self, win):
        self.log.append(("shut", win))
        return True

    def create_agent(self, win, model=None, *, slot=None, args=None, cwd=None):
        self.log.append(("create", win, slot, args, cwd))
        if not self.create_ok:
            return False
        if self.appear:
            self._queue.append((win, slot))   # frühestens beim nächsten Blick da
        return True

    # ── Broker ────────────────────────────────────────────────────────────
    def connected(self, win):
        return self.t - self._t_start >= self.connect_after

    def terminals(self, win):
        return list(self.terms.get(win, []))

    # ── Umgebung ──────────────────────────────────────────────────────────
    def count_exe(self, name):
        return self.alive[0] if len(self.alive) == 1 else self.alive.pop(0)

    def launch(self, exe, folder):
        self.log.append(("launch", folder))
        self.launched.append(folder)
        return True

    def find_exe(self):
        return self.exe

    # Die beiden Eskalationsstufen MUESSEN hier gefaelscht sein: die echten schicken
    # WM_CLOSE an jedes VS-Code-Fenster bzw. rufen `taskkill /F /T /IM Code.exe`. Fehlten
    # sie, beendete `python tests/run.py` das VS Code, in dessen Terminal er laeuft - so
    # am 2026-11-06 zweimal passiert, ohne Crash-Dump und ohne Zeile im VS-Code-Log.
    def close_editors(self):
        self.log.append(("wm_close",))
        return 0

    def kill_editors(self, name):
        self.log.append(("taskkill", name))
        return True

    def clock(self):
        return self.t

    # ── Plan-Datei ────────────────────────────────────────────────────────
    def _load(self, path=None):
        return self.saved

    def _save(self, plan, path=None):
        self.saved = plan

    def _clear(self, path=None):
        self.cleared = True
        self.saved = None

    # ── Fahren ────────────────────────────────────────────────────────────
    def __enter__(self):
        self._alt = (vr.rp.load, vr.rp.save, vr.rp.clear)
        vr.rp.load, vr.rp.save, vr.rp.clear = self._load, self._save, self._clear
        self.r = vr.Restarter(self, self, snapshot=lambda: self.plan,
                              count_exe=self.count_exe, launch=self.launch,
                              find_exe=self.find_exe, clock=self.clock,
                              close_editors=self.close_editors,
                              kill_editors=self.kill_editors)
        return self

    def __exit__(self, *a):
        vr.rp.load, vr.rp.save, vr.rp.clear = self._alt
        return False

    def run(self, ticks=400, step=0.5):
        """Den Ablauf fahren, bis er zur Ruhe kommt. Zwischen zwei Blicken rückt die
        Uhr - und die zuletzt angeforderten Chats tauchen auf (so wie die Extension
        ihre Terminalliste erst nach dem Anlegen meldet)."""
        self.r.start(lambda text, ok=None: self.log.append(("say", ok)))
        for _ in range(ticks):
            if not self.r.busy():
                break
            self.r.step()
            self.t += step
            for win, slot in self._queue:
                self.terms.setdefault(win, []).append(slot or f"{win}?")
                self.log.append(("appear", win, slot))
            self._queue = []
        return self.r

    def only(self, kind):
        return [e for e in self.log if e[0] == kind]


# ── Herunterfahren ────────────────────────────────────────────────────────
def test_erst_speichern_dann_terminals_dann_fenster():
    """Die Reihenfolge ist der ganze Trick. Ungespeicherte Editoren machen aus dem
    Schließen einen modalen Dialog, und liegengebliebene Terminals kommen beim nächsten
    Start als leere Hüllen mit denselben Namen zurück."""
    with Fake() as f:
        f.run()
    kinds = [e[0] for e in f.log if e[0] in ("save", "kill", "shut")]
    assert kinds == ["save", "save", "kill", "kill", "kill", "shut", "shut"]


def test_geschlossen_werden_genau_die_chats_aus_dem_plan():
    with Fake() as f:
        f.run()
    assert [e[1] for e in f.only("kill")] == ["A1", "A2", "B1"]


def test_die_chats_gehen_vor_ihrem_fenster_zu():
    """Sonst stellt VS Code sie beim nächsten Start wieder her - und die Extension
    sammelt die Hüllen als Slots ein."""
    with Fake() as f:
        f.run()
    letzter_kill = max(i for i, e in enumerate(f.log) if e[0] == "kill")
    erster_shut = min(i for i, e in enumerate(f.log) if e[0] == "shut")
    assert letzter_kill < erster_shut


def test_ohne_vs_code_wird_nichts_geschlossen():
    """Code.exe nicht gefunden -> Abbruch VOR dem ersten closeWindow. Ein Lauf, der
    schließt und dann nichts mehr öffnen kann, wäre das schlimmste Ergebnis."""
    with Fake(exe=None) as f:
        f.run()
    assert f.r.phase == vr.FAILED
    assert f.only("shut") == [] and f.only("kill") == []


def test_ohne_verbundenes_fenster_gibt_es_nichts_zu_tun():
    with Fake(plan={"windows": [], "skipped": []}) as f:
        f.run()
    assert f.r.phase == vr.FAILED and f.only("shut") == []


# ── Nachsehen statt warten ────────────────────────────────────────────────
def test_erst_wenn_kein_prozess_mehr_laeuft_wird_gestartet():
    """Ein Code.exe, das startet, während der alte Prozess noch stirbt, meldet sich bei
    ihm an - dann wäre der Neustart ein Reload mit Extra-Schritten."""
    with Fake(alive=(3, 3, 2, 0)) as f:
        f.run()
    erster_launch = min(i for i, e in enumerate(f.log) if e[0] == "launch")
    assert [e[0] for e in f.log[:erster_launch]].count("shut") == 2
    assert f.launched == [REPO_A, REPO_B]


def test_laufende_prozesse_lassen_den_lauf_abbrechen_statt_blind_zu_starten():
    """Ungespeicherte Datei, ein nicht verbundenes Fenster - irgendetwas hält VS Code
    offen. Dann wird nichts gestartet und der Plan bleibt liegen."""
    with Fake(alive=(4,)) as f:
        f.run()
    assert f.r.phase == vr.FAILED
    assert f.launched == [] and not f.cleared and f.saved is not None
    # Beide Stufen wurden versucht - an der Attrappe, nicht am echten VS Code.
    assert ("wm_close",) in f.log and ("taskkill", "Code.exe") in f.log


def test_unlesbare_prozessliste_ist_keine_entwarnung():
    """None heißt „ich konnte nicht nachsehen" - und darf nie als „nichts läuft mehr"
    durchgehen."""
    with Fake(alive=(None,)) as f:
        f.run()
    assert f.r.phase == vr.FAILED and f.launched == []


# ── Wieder hochfahren ─────────────────────────────────────────────────────
def test_jedes_fenster_wird_mit_seinem_ordner_geoeffnet():
    with Fake() as f:
        f.run()
    assert f.launched == [REPO_A, REPO_B]


def test_in_einem_fenster_kommt_der_naechste_chat_erst_nach_dem_vorigen():
    """Jeder Chat entsteht als Split des vorherigen; zwei gleichzeitig angeforderte
    Splits geraten sich in die Quere."""
    with Fake() as f:
        f.run()
    folge = [e for e in f.log if e[0] in ("create", "appear") and e[1] == "A"]
    assert [e[0] for e in folge] == ["create", "appear", "create", "appear"]
    assert [e[2] for e in folge] == ["A1", "A1", "A2", "A2"]


def test_alle_chats_kommen_mit_ihren_flags_und_ihrem_verzeichnis_zurueck():
    with Fake() as f:
        f.run()
    got = {(e[1], e[2]): (e[3], e[4]) for e in f.only("create")}
    assert got[("A", "A1")] == ('--resume "x"', REPO_A)
    assert got[("A", "A2")] == ("", REPO_A)
    assert got[("B", "B1")] == ('--resume "y"', REPO_B)


def test_ein_fenster_das_sich_nicht_zurueckmeldet_stoppt_die_anderen_nicht():
    """Verbindet sich nichts, läuft der Lauf in seinen Timeout - aber erst, nachdem
    alles versucht wurde. Gemeldet wird es als Fehlschlag, und der Plan bleibt."""
    with Fake(connect_after=10_000.0) as f:
        f.run(ticks=2000)
    assert f.r.phase == vr.FAILED
    assert f.only("create") == [] and not f.cleared


def test_ein_geglueckter_lauf_raeumt_den_plan_weg():
    with Fake() as f:
        f.run()
    assert f.r.phase == vr.DONE and f.cleared
    assert f.r._made == 3


def test_verlorene_verbindung_mitten_im_wiederherstellen_ueberspringt_den_rest():
    """Sendet der Broker nicht mehr, ist das Fenster weg - dann hat es keinen Sinn,
    seine restlichen Chats anzufordern. Der Lauf endet als Fehlschlag mit Plan."""
    with Fake(create_ok=False) as f:
        f.run()
    assert f.r.phase in (vr.DONE, vr.FAILED)
    assert len(f.only("create")) == 2      # je Fenster genau ein Versuch
    assert f.r._made == 0


def test_ein_chat_der_nicht_auftaucht_haelt_das_fenster_nicht_auf():
    """Ein einzelner Split kann scheitern (VS-Code-Bug 205254). Dann läuft die Frist
    ab, und der nächste Chat wird trotzdem angefordert."""
    with Fake(appear=False) as f:
        f.run(ticks=2000)
    assert [e[2] for e in f.only("create") if e[1] == "A"] == ["A1", "A2"]
    assert f.r.phase == vr.DONE and f.r._made == 0


# ── Fortsetzen nach einem Abbruch ─────────────────────────────────────────
def test_ein_liegengebliebener_plan_wird_geoeffnet_statt_neu_geplant():
    """Der Zustand nach einem abgebrochenen Lauf: alles zu, nichts wieder da. Derselbe
    Knopf setzt fort - und schließt dabei NICHTS mehr (es ist schon alles zu)."""
    with Fake(saved=PLAN, plan={"windows": [], "skipped": []},
              connect_after=0.0) as f:
        # connected() ist hier sofort True; needs_relaunch fragt VOR dem Start, ob
        # eines der geplanten Fenster lebt -> deshalb erst zu, dann verbunden.
        f._t_start = f.t + 1.0
        f.run()
    assert f.only("shut") == [] and f.only("kill") == []
    assert f.launched == [REPO_A, REPO_B]


def test_wer_beschaeftigt_ist_sagt_es():
    """Das Panel setzt seine Aufräumarbeiten nur aus, solange busy() gilt - ohne das
    vergisst es die Bindungen (anderer Buchstabe) und fegt die Ticket-worktrees weg."""
    with Fake() as f:
        f.r.start(None)
        assert f.r.busy()
        f.t += 1.0
        f.r.step()
        assert f.r.busy()
        f.run()
        assert not f.r.busy()


def test_ein_zweiter_druck_waehrend_des_laufs_bleibt_nicht_stumm():
    """Sonst drueckt man den Knopf ein drittes Mal. Die Meldung geht an den NEUEN
    Rueckruf - der alte gehoert einem vielleicht schon geschlossenen Dialog."""
    with Fake() as f:
        f.r.start(None)
        gesagt = []
        assert not f.r.start(lambda text, ok=None: gesagt.append(text))
        assert gesagt and "Neustart" in gesagt[0]


def test_ein_fehler_im_ablauf_beendet_den_lauf_statt_den_poll_zu_reissen():
    """step() wird aus dem Poll des Panels gerufen. Eine Ausnahme dort nähme die
    Anzeige mit - also fängt der Ablauf sie und stellt sich auf FAILED."""
    with Fake() as f:
        f.r.start(None)
        f.r.cmds = None            # jeder Aufruf wirft jetzt
        f.t += 5.0
        f.r.step()                 # darf NICHT werfen
    assert f.r.phase == vr.FAILED


if __name__ == "__main__":
    for name, fn in sorted(globals().items()):
        if name.startswith("test_") and callable(fn):
            fn()
            print(f"  ok  {name}")
