"""Windows Terminal als Host fuer Agenten - das Gegenstueck zum Broker fuer VS Code.

Das Panel spricht mit seinen Fenstern ueber eine Handvoll Fragen und ein Kommando-
Vokabular (net/broker.py, domain/protocol.py). Fuer VS Code beantwortet sie die
Extension ueber den Socket; fuer Windows Terminal gibt es keine Extension, also
beantwortet diese Klasse sie selbst, mit denselben Methodennamen (ui/hosts.py
verteilt je Buchstabe):

  Frage / Kommando        VS Code (Extension)        Windows Terminal (hier)
  ───────────────────     ───────────────────────    ─────────────────────────────────
  welche Slots leben      Terminal-Liste             .wtpid-Dateien mit lebender Shell
  createAgent             createTerminal/Split       wt.exe new-tab, eigenes Fenster,
                                                     danach kacheln (ops/wt_tiles)
  focusPane               terminal.show()            Fenster mit dem Slot als Titel
  send / key              terminal.sendText()        Eingabepuffer der Konsole
  closeAgent              terminal.dispose()         taskkill /T auf die Pane-Shell
  seen                    onDidChangeActiveTerminal  Vordergrundfenster = Slot-Titel

Welche Buchstaben in Windows Terminal leben, steht in `wt_hosts.json` neben dem Code
({Buchstabe: Repo-Ordner}); die Bindung Buchstabe -> Repo-Name bleibt in
bindings.json, wie fuer VS Code. Ein WT-Block gilt als verbunden, solange er dort
steht - auch ohne offenes Pane, denn das Deck kann das Fenster jederzeit selbst
oeffnen. Weg ist er erst mit "vergessen".

Alles, was Fenster oder Prozesse anfasst, laeuft auf EINEM Arbeiter-Thread
hintereinander: der UIA-Arbeiter ist nicht fuer parallele Fragen gebaut, und ein
Klick soll den Tk-Takt nicht anhalten.
"""
import os
import queue
import shutil
import subprocess
import sys
import threading
import time
from collections.abc import Callable
from typing import Any

from deck.domain import paths as dp
from deck.domain import protocol
from deck.domain import wt
from deck.ops import log
from deck.ops.wt_tiles import WtTiler
from deck.platform import console_input
from deck.platform import focus as wf
from deck.platform import procs
from deck.platform.wt_uia import UiaWorker

HOSTS_PATH = os.path.join(dp.REPO_ROOT, "wt_hosts.json")
AGENT_SCRIPT = os.path.join(dp.REPO_ROOT, "deck", "ops", "wt_agent.ps1")
UIA_SCRIPT = os.path.join(dp.REPO_ROOT, "deck", "platform", "wt_uia_worker.ps1")
WT_SETTINGS = os.path.join(
    os.environ.get("LOCALAPPDATA") or os.path.expanduser("~"),
    "Packages", "Microsoft.WindowsTerminal_8wekyb3d8bbwe", "LocalState", "settings.json")

PENDING_TTL_S = 30.0     # so lange gilt ein angeforderter Name als vergeben
ALIVE_CACHE_S = 0.3      # terminals() wird je Takt dutzendfach gefragt
_CREATE_NO_WINDOW = 0x08000000


def _profile_available(path: str = WT_SETTINGS) -> bool:
    """Gibt es das versteckte Profil "Agent Deck" (closeOnExit: always)? Ohne es oeffnet
    das Deck die Panes mit dem Standardprofil - laeuft auch, nur bleibt ein beendeter
    Agent als "[process exited ...]" stehen."""
    try:
        with open(path, encoding="utf-8") as f:
            return f'"{wt.PROFILE}"' in f.read()
    except OSError:
        return False


def _launch(argv: list[str]) -> bool:
    try:
        subprocess.Popen(argv, creationflags=_CREATE_NO_WINDOW,
                         stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                         stderr=subprocess.DEVNULL)
        return True
    except OSError:
        log.exc("wt_host.launch")
        return False


def _inject(pid: int, steps: list) -> bool:
    """Eingabe ueber einen Wegwerfprozess (Begruendung in platform/console_input)."""
    try:
        r = subprocess.run(
            [sys.executable, "-m", "deck.platform.console_input", str(pid),
             console_input.encode_steps(steps)],
            cwd=dp.REPO_ROOT, capture_output=True, text=True, timeout=30,
            creationflags=_CREATE_NO_WINDOW)
        ok = r.returncode == 0
        if not ok:
            log.note(f"wt_host.inject pid={pid}: {(r.stdout or r.stderr).strip()[:200]}")
        return ok
    except Exception:
        log.exc("wt_host.inject")
        return False


def _foreground() -> tuple[str, str]:
    h = wf.foreground_hwnd()
    return wf.class_of(h), wf.title_of(h)


class WtHost:
    def __init__(self, *, hosts_path: str = HOSTS_PATH, state_dir: str = dp.STATE_DIR,
                 uia: Any = None, launch: Callable = _launch, inject: Callable = _inject,
                 start_of: Callable = procs.process_start, kill: Callable = procs.kill_tree,
                 focus_window: Callable = wf.focus_window,
                 foreground: Callable = _foreground, clock: Callable = time.time,
                 wt_exe: str | None = None, shell: str | None = None,
                 profile: str | None | bool = True, tiler: Any = None,
                 sync: bool = False) -> None:
        self.hosts_path = hosts_path
        self.state_dir = state_dir
        self._uia = uia
        self.launch, self.inject = launch, inject
        self.start_of, self.kill = start_of, kill
        self.focus_window, self.foreground, self.clock = focus_window, foreground, clock
        self.wt_exe = wt_exe or shutil.which("wt") or "wt.exe"
        self.shell = shell or shutil.which("pwsh") or "powershell.exe"
        if profile is True:
            profile = wt.PROFILE if _profile_available() else None
        self.profile = profile or None
        self.sync = sync
        self.tiler = tiler or WtTiler()
        self.hosts: dict[str, str] = dict(dp.load_json(hosts_path, {}) or {})
        self._pending: dict[str, float] = {}
        self._alive: dict[str, int] = {}
        self._alive_ts = -1.0
        self._last_fg: str | None = None
        self._jobs: "queue.Queue[Callable | None]" = queue.Queue()
        self._worker: threading.Thread | None = None

    # ── Infrastruktur ───────────────────────────────────
    @property
    def uia(self) -> Any:
        if self._uia is None:
            self._uia = UiaWorker(UIA_SCRIPT)
        return self._uia

    def _do(self, job: Callable) -> None:
        """Eine Fenster-/Prozess-Aktion ausfuehren: im Test sofort, sonst hintereinander
        auf dem Arbeiter-Thread."""
        if self.sync:
            job()
            return
        if self._worker is None or not self._worker.is_alive():
            self._worker = threading.Thread(target=self._run_jobs, name="wt-host", daemon=True)
            self._worker.start()
        self._jobs.put(job)

    def _run_jobs(self) -> None:
        while True:
            job = self._jobs.get()
            if job is None:
                return
            try:
                job()
            except Exception:
                log.exc("wt_host.job")

    def start(self) -> None:
        """Gegenstueck zu Broker.start - hier gibt es nichts vorzubereiten."""

    def stop(self) -> None:
        self._jobs.put(None)
        if self._uia is not None:
            try:
                self._uia.close()
            except Exception:
                pass

    def _save(self) -> None:
        dp.save_json(self.hosts_path, self.hosts)

    def _pidfile(self, slot: str) -> str:
        return os.path.join(self.state_dir, slot + wt.PID_SUFFIX)

    # ── Was lebt ────────────────────────────────────────
    def alive_slots(self) -> dict[str, int]:
        """{slot: pid der Pane-Shell} fuer jedes Pane, dessen Shell noch laeuft. Tote
        PID-Dateien werden dabei entfernt - eine liegengebliebene Datei soll nicht
        Jahre spaeter auf einen fremden Prozess mit derselben PID zeigen."""
        now = self.clock()
        if 0 <= now - self._alive_ts < ALIVE_CACHE_S:
            return self._alive
        out: dict[str, int] = {}
        try:
            names = os.listdir(self.state_dir)
        except OSError:
            names = []
        for fn in names:
            if not fn.endswith(wt.PID_SUFFIX):
                continue
            slot = fn[:-len(wt.PID_SUFFIX)]
            got = wt.parse_pidfile(dp.load_json(os.path.join(self.state_dir, fn)))
            if not got:
                continue
            pid, created = got
            start = self.start_of(pid)
            if start is not None and wt.same_start(start, created):
                out[slot] = pid
            else:
                try:
                    os.remove(os.path.join(self.state_dir, fn))
                except OSError:
                    pass
        self._alive, self._alive_ts = out, now
        for name in [n for n, t in self._pending.items()
                     if n in out or now - t > PENDING_TTL_S]:
            del self._pending[name]
        # Neu kacheln, sobald ein Agent dazukommt oder geht. Die PID-Datei schreibt die
        # Shell IM Fenster, es steht also schon, wenn der Slot hier auftaucht.
        for win in list(self.hosts):
            slots = wt.slots_of(win, out)
            if self.tiler.changed(win, slots):
                self._do(lambda s=slots: self.tiler.tile(s))
        return out

    def _forget_alive(self) -> None:
        self._alive_ts = -1.0

    # ── Broker-Schnittstelle ────────────────────────────
    def has(self, win: str) -> bool:
        return win in self.hosts

    def slot_of_title(self, title: str | None) -> str | None:
        """Fenstertitel -> Slot, wenn das Fenster ein Agent eines WT-Blocks ist."""
        slot = wt.slot_from_title(title)
        return slot if slot and self.has(slot[0]) else None

    def connected(self, win: str) -> bool:
        return self.has(win)

    def remote(self, win: str) -> None:
        return None

    def terminals(self, win: str) -> list[str]:
        if not self.has(win):
            return []
        return wt.slots_of(win, self.alive_slots())

    def workspaces(self) -> list[str]:
        return []

    def workspace_slots(self, workspace: str) -> list[str]:
        return []

    def client_info(self) -> list[dict[str, Any]]:
        return []                   # der Neustart ist eine VS-Code-Sache

    def assign(self, workspace: str, window: str) -> bool:
        return False                # WT-Bloecke bindet das Deck selbst (open_repo)

    def forget(self, win: str) -> bool:
        """Den Block vergessen. Laufende Panes bleiben offen - "vergessen" heisst wie bei
        VS Code: das Deck schaut nicht mehr hin, es schliesst nichts."""
        if self.hosts.pop(win, None) is None:
            return False
        self._save()
        return True

    def drain_seen(self) -> set[str]:
        """Den Slot melden, dessen Pane gerade vorne ist - einmal je Wechsel, wie die
        Extension (onDidChangeActiveTerminal). Der Fenstertitel ist der Pane-Titel und
        der ist der Slot-Name."""
        try:
            cls, title = self.foreground()
        except Exception:
            return set()
        slot = self.slot_of_title(title) if cls == wt.WINDOW_CLASS else None
        if slot == self._last_fg:
            return set()
        self._last_fg = slot
        return {slot} if slot else set()

    def send_window(self, win: str, obj: dict[str, Any]) -> bool:
        """Ein Kommando aus domain/protocol ausfuehren (Felder wie bei der Extension)."""
        if not self.has(win):
            return False
        cmd = obj.get("cmd")
        slot = obj.get("slot")
        if cmd == protocol.CMD_FOCUS_PANE:
            return self.focus(slot)
        if cmd == protocol.CMD_SEND:
            return self._input(slot, wt.send_steps(
                obj.get("text", ""), execute=obj.get("execute", True) is not False,
                submit=bool(obj.get("submit"))))
        if cmd == protocol.CMD_KEY:
            return self._input(slot, wt.key_steps(obj.get("key", ""), obj.get("repeat", 1)))
        if cmd == protocol.CMD_CREATE_AGENT:
            return self.create(win, obj.get("model"), slot=slot,
                               args=obj.get("args"), cwd=obj.get("cwd"))
        if cmd == protocol.CMD_CLOSE_AGENT:
            return self.close(slot)
        if cmd == protocol.CMD_CLOSE_WINDOW:
            for s in self.terminals(win):
                self.close(s)
            return True
        if cmd in (protocol.CMD_RELOAD, protocol.CMD_SAVE_ALL):
            return True             # gibt es in Windows Terminal nicht - nichts zu tun
        return False

    # ── Aktionen ────────────────────────────────────────
    def open_repo(self, win: str, folder: str) -> None:
        """Einen Buchstaben als Windows-Terminal-Block fuer diesen Ordner anlegen."""
        self.hosts[win] = folder
        self._save()

    def create(self, win: str, model: str | None = None, *, slot: str | None = None,
               args: str | None = None, cwd: str | None = None) -> bool:
        """Einen Agenten in einem eigenen Fenster (`deck-<Slot>`) oeffnen. Gekachelt
        wird, sobald seine Shell lebt (alive_slots)."""
        folder = self.hosts.get(win)
        if not folder:
            return False
        alive = self.terminals(win)
        name = wt.wanted_or_next(win, slot, set(alive) | set(self._pending))
        self._pending[name] = self.clock()
        spec = wt.launch_spec(name, self.state_dir, wt.claude_argv(model, args))
        work_dir = cwd or folder
        self._do(lambda: self.launch(wt.wt_argv(
            self.wt_exe, name, work_dir, self.shell, AGENT_SCRIPT, spec,
            profile=self.profile)))
        return True

    def focus(self, slot: str | None) -> bool:
        """Das Fenster des Agenten nach vorn. Liegt der Agent noch als Pane in einem
        geteilten Fenster (Bloecke von vor dem 2026-11-06), sucht ihn die UI Automation."""
        if not slot or slot not in self.alive_slots():
            return False

        def job() -> None:
            hwnd = self.tiler.hwnd_of(slot)
            if hwnd:
                self.focus_window(hwnd)
                return
            hwnd = self.uia.find(slot)
            if hwnd:
                self.focus_window(hwnd)
                self.uia.focus(slot)
        self._do(job)
        return True

    def raise_window(self, win: str) -> bool:
        """Klick auf den Repo-Namen: alle Fenster dieses Blocks nach vorn, das des
        ersten Agenten zuletzt und damit obenauf."""
        slots = self.terminals(win)
        if not slots:
            return False

        def job() -> None:
            hwnds = [h for h in map(self.tiler.hwnd_of, slots) if h]
            for h in reversed(list(dict.fromkeys(hwnds))):
                self.focus_window(h)
            if hwnds:
                return
            hwnd = self.uia.find(slots[0])
            if hwnd:
                self.focus_window(hwnd)
        self._do(job)
        return True

    def _input(self, slot: str | None, steps: list) -> bool:
        pid = self.alive_slots().get(slot or "")
        if not pid:
            return False
        self._do(lambda: self.inject(pid, steps))
        return True

    def close(self, slot: str | None) -> bool:
        """Den Agenten beenden: die Shell des Panes samt claude. Das Pane geht mit zu
        (Profil "Agent Deck": closeOnExit always)."""
        pid = self.alive_slots().get(slot or "")
        if not pid:
            return False
        path = self._pidfile(slot)

        def job() -> None:
            self.kill(pid)
            try:
                os.remove(path)
            except OSError:
                pass
            self._forget_alive()
        self._do(job)
        return True
