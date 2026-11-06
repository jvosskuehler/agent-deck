"""VS Code komplett neu starten und die Claude-Sessions wieder aufnehmen.

Der Knopf „VS Code neu starten" in den Einstellungen. Gedacht für den Fall, in dem ein
Reload nicht mehr reicht: hängende Renderer, ein blockierter Port im Prozessbaum, eine
frisch installierte Extension. Alles zu, alles neu - aber die Chats sollen zurückkommen.

Der Ablauf ist eine Zustandsmaschine, die vom Panel im Poll-Takt getickt wird
(`step()`), KEINE Schleife mit `sleep`. Das ist keine Stilfrage: das Deck zeichnet und
animiert im selben Thread, und ein blockierender Lauf über eine Minute wäre ein
eingefrorenes Fenster - genau das, was einem Sorge macht, während die eigene
Arbeitsumgebung gerade zugeht.

Die Phasen:

  SAVE   - alles speichern. Ohne das öffnet das Schließen einen modalen Dialog, und
           der hält den ganzen Ablauf auf, bis jemand hinsieht.
  KILL   - JEDES Agent-Terminal einzeln schließen, vor den Fenstern. Sonst stellt VS
           Code beim nächsten Start leere Hüllen mit denselben Namen (A1, A2, …) wieder
           her - die Extension sammelt sie als Slots ein und die Namensvergabe verbiegt.
  SHUT   - die Fenster schließen.
  WAIT   - nachsehen, bis KEIN Code-Prozess mehr läuft. Nicht warten: nachsehen. Ein
           `Code.exe`, das startet, während der alte Prozess noch stirbt, meldet sich
           bei ihm an statt eine frische Instanz zu werden - dann wäre der ganze
           Neustart ein Reload mit Extra-Schritten.

           Diese Phase eskaliert in drei Stufen, weil „beendet" hier eine ZUSAGE ist
           und keine Hoffnung: geschlossen werden können über die Extension nur die
           Fenster, die das Deck kennt - offen halten den Prozess aber alle. Also erst
           geduldig zusehen, dann jedes verbliebene VS-Code-Fenster per WM_CLOSE
           fragen (auch die fremden; VS Code darf dabei selbst nach ungespeicherten
           Dateien fragen), und wenn dann noch etwas lebt, den Prozessbaum beenden.
           Erst danach gilt der Zustand als nicht herstellbar.
  LAUNCH - je Fenster `Code.exe --new-window <ordner>`, versetzt.
  HELLO  - warten, bis sich die Extensions gemeldet haben (das Panel bindet sie über
           den Workspace-Namen selbst wieder an ihren Buchstaben).
  RESUME - je Fenster die Chats der Reihe nach anlegen, jeder mit `claude --resume`.
           Innerhalb eines Fensters streng sequenziell (jeder Chat entsteht als Split
           des vorherigen), über Fenster hinweg parallel.

Scheitert ein Schritt, wird abgebrochen, aber nichts weggeworfen: die Plan-Datei bleibt
liegen, und derselbe Knopf setzt beim nächsten Druck bei LAUNCH fort
(`restart_plan.needs_relaunch`).
"""
import os
import subprocess
import time
from typing import Any

from deck import i18n
from deck.domain import restart_plan as rp
from deck.ops import log
from deck.ops import vscode_glow as vg
from deck.ops import window_gather as wg
from deck.platform import arrange as wa
from deck.platform import procs

# Phasen. IDLE/DONE/FAILED sind Ruhezustände, alles dazwischen ist ein laufender Lauf.
IDLE, SAVE, KILL, SHUT = "idle", "save", "kill", "shut"
WAIT, LAUNCH, HELLO, RESUME = "wait", "launch", "hello", "resume"
DONE, FAILED = "done", "failed"

# Zeiten in Sekunden. Großzügig, weil jede Zahl hier eine Wette auf einen fremden
# Rechner ist - und ein zu früher Abbruch teurer ist als ein paar Sekunden Geduld.
SAVE_SETTLE = 1.5      # nach „alles speichern" bis zum Schließen der Terminals
KILL_SETTLE = 1.2      # nach dem letzten Terminal bis zum Schließen der Fenster
SHUT_SETTLE = 2.0      # bevor das erste Mal in die Prozessliste gesehen wird
SOFT_CLOSE_S = 12.0    # danach: JEDES verbliebene VS-Code-Fenster per WM_CLOSE fragen
FORCE_KILL_S = 30.0    # danach: Notbremse - Prozessbaum beenden
DEAD_TIMEOUT = 45.0    # danach: aufgeben (der Zustand ist nicht herstellbar)
LAUNCH_GAP = 2.0       # Abstand zwischen zwei Fenster-Starts
HELLO_TIMEOUT = 120.0  # VS Code starten + Extension verbinden (kalter Start!)
SLOT_TIMEOUT = 40.0    # bis ein angeforderter Chat als Terminal auftaucht
SLOT_SETTLE = 1.5      # danach, bevor im selben Fenster der nächste angelegt wird
TICK = 0.5             # Ruhepause zwischen zwei Blicken


def code_exe() -> str | None:
    """Die ausführbare Datei von VS Code (oder None).

    Gesucht wird in denselben Installationsorten wie beim Glow-Patch
    (`vscode_glow.vscode_bases`, inklusive `AGENT_DECK_VSCODE_DIR` und PATH-Ableitung) -
    eine Quelle für „wo wohnt VS Code", nicht zwei.
    """
    for base in vg.vscode_bases():
        for name in ("Code.exe", "Code - Insiders.exe"):
            p = os.path.join(base, name)
            if os.path.isfile(p):
                return p
    return None


def launch_window(exe: str, folder: str) -> bool:
    """Ein VS-Code-Fenster mit diesem Ordner öffnen. True = Prozess gestartet.

    Ob Pfad oder Remote-Adresse, entscheidet `rp.open_args` – die Unterscheidung ist
    Rechnerei und steht darum in der Domäne, wo sie getestet ist."""
    try:
        subprocess.Popen([exe, *rp.open_args(folder)])
        return True
    except Exception:
        log.exc("vscode_restart.launch")
        return False


def close_editor_windows() -> int:
    """JEDES offene VS-Code-Hauptfenster zum Schließen auffordern (WM_CLOSE).

    Die Auswahl ist dieselbe wie beim Einsammeln (`window_gather.editor_windows`) -
    eine Regel, ein Ort. Rückgabe: wie viele gefragt wurden; „gefragt" ist nicht
    „geschlossen" (bei ungespeicherten Dateien fragt VS Code selbst nach), darum sieht
    der Aufrufer danach weiter in die Prozessliste.

    Das trifft absichtlich auch Fenster, die das Deck nicht kennt. Sie halten den
    Prozess genauso am Leben, und ohne sie wäre die Zusage „am Ende ist VS Code
    beendet" nur die halbe Wahrheit.
    """
    n = 0
    for w in wg.editor_windows():
        if wa.post_close(w.hwnd):
            n += 1
    return n


def open_editor_titles() -> list[str]:
    """Titel der offenen VS-Code-Hauptfenster (für die Warnung vor dem Neustart)."""
    return [w.title for w in wg.editor_windows()]


class Restarter:
    """Der Ablauf. Eine Instanz lebt so lange wie das Panel, ist aber nur zwischen
    `start()` und dem Ende eines Laufs beschäftigt (`busy()`).

    Die Abhängigkeiten stehen im Konstruktor, damit der Ablauf ohne VS Code, ohne
    Broker und ohne Uhr geprüft werden kann - er ist die Sorte Logik, die man nicht
    „mal durchprobiert", weil jeder Probelauf die eigene Arbeitsumgebung schließt.
    """

    def __init__(self, cmds: Any, broker: Any, *, snapshot: Any,
                 count_exe: Any = procs.count_exe, launch: Any = launch_window,
                 find_exe: Any = code_exe, clock: Any = time.monotonic,
                 close_editors: Any = close_editor_windows,
                 kill_editors: Any = procs.kill_by_name) -> None:
        self.cmds = cmds            # BrokerCommands: save_all/close_agent/close_window/create_agent
        self.broker = broker        # Broker: connected(win), terminals(win)
        self.snapshot = snapshot    # () -> Plan-Dict (baut das Panel, es kennt Order/Settings)
        self.count_exe = count_exe
        self.launch = launch
        self.find_exe = find_exe
        self.clock = clock
        self.close_editors = close_editors   # WM_CLOSE an ALLE VS-Code-Fenster
        self.kill_editors = kill_editors     # Notbremse: Prozessbaum beenden
        self.phase = IDLE
        self.plan: dict[str, Any] = {}
        self._report: Any = None
        self._exe: str | None = None
        self._t0 = 0.0              # Beginn der aktuellen Phase (für die Timeouts)
        self._at = 0.0              # frühester Zeitpunkt für den nächsten Schritt
        self._todo: list[dict[str, Any]] = []                 # noch zu öffnende Fenster
        self._pending: dict[str, list[dict[str, Any]]] = {}    # win -> offene Chats
        self._inflight: dict[str, tuple[int, float]] = {}      # win -> (Terminals vorher, Frist)
        self._ready_at: dict[str, float] = {}                  # win -> nicht vor diesem Moment
        self._lost: list[str] = []                             # Fenster, die nicht zurückkamen
        self._made = 0                                         # angelegte Chats
        self._esc = 0                                          # Eskalationsstufe in WAIT
        self._forced = False                                   # musste die Notbremse ran?

    # ── Zustand ───────────────────────────────────────────────────────────
    def busy(self) -> bool:
        """Läuft gerade ein Neustart? Solange das gilt, MUSS das Panel seine
        Aufräumarbeiten aussetzen: `_cleanup_closed_windows` würde die Bindungen nach
        drei Sekunden vergessen (dann bekäme ein Fenster einen anderen Buchstaben als
        im Plan), und der worktree-Sweep würde die Ticket-Arbeitsbäume löschen, weil zu
        keinem Slot mehr ein Agent lebt."""
        return self.phase not in (IDLE, DONE, FAILED)

    def _say(self, text: str, ok: Any = None) -> None:
        """Fortschritt melden: immer ins Log, zusätzlich an den Dialog, solange es ihn
        gibt. Wirft der Rückruf (Fenster inzwischen zu), wird er vergessen - ein
        geschlossener Dialog darf den Lauf nicht abbrechen."""
        log.note(f"restart[{self.phase}] {text}")
        if self._report:
            try:
                self._report(text, ok)
            except Exception:
                self._report = None

    def _fail(self, text: str) -> None:
        self.phase = FAILED
        self._say(text, False)

    def _enter(self, phase: str, delay: float = 0.0) -> None:
        self.phase = phase
        self._t0 = self.clock()
        self._at = self._t0 + delay

    def _windows(self) -> list[dict[str, Any]]:
        return list(self.plan.get("windows") or [])

    def _letters(self) -> list[str]:
        return [w["window"] for w in self._windows() if w.get("window")]

    # ── Start ─────────────────────────────────────────────────────────────
    def start(self, report: Any = None) -> bool:
        """Einen Lauf beginnen. `report(text, ok)` wird bei jedem Fortschritt gerufen
        (ok: None = läuft, True = fertig, False = Fehler). False = es gab nichts zu
        tun (die Begründung ist dann schon gemeldet)."""
        if self.busy():
            # Schon unterwegs. Die Meldung geht an den NEUEN Rueckruf (der alte gehoert
            # einem evtl. inzwischen geschlossenen Dialog) - sonst bliebe der Knopf
            # stumm und man drueckt ihn ein drittes Mal.
            if report:
                try:
                    report(i18n.L(f"Ein Neustart läuft bereits ({self.phase}).",
                                  f"A restart is already running ({self.phase})."), None)
                except Exception:
                    pass
            return False
        self._report = report
        self._exe = self.find_exe()
        self._lost, self._made = [], 0
        self._esc, self._forced = 0, False
        self._pending, self._inflight, self._ready_at = {}, {}, {}
        if not self._exe:
            self._fail(i18n.L("VS Code nicht gefunden (Code.exe).",
                              "Could not find VS Code (Code.exe)."))
            return False

        # Liegengebliebener Plan, dessen Fenster alle zu sind? Dann ist ein früherer
        # Lauf nach dem Schließen abgebrochen - fortsetzen statt neu planen (ein
        # frischer Plan wäre jetzt leer und würde den alten überschreiben).
        old = rp.load()
        if old is not None:
            self.plan = old
            live = [w for w in self._letters() if self.broker.connected(w)]
            if rp.needs_relaunch(old, live):
                self._todo = self._windows()
                self._say(i18n.L(
                    f"Abgebrochener Neustart gefunden – {len(self._todo)} Fenster "
                    f"werden geöffnet.",
                    f"Found an interrupted restart – reopening {len(self._todo)} "
                    f"window(s)."))
                self._enter(LAUNCH)
                return True

        self.plan = self.snapshot() or {}
        if not rp.is_usable(self.plan):
            self._fail(i18n.L("Kein verbundenes VS-Code-Fenster mit Ordner.",
                              "No connected VS Code window with a folder."))
            return False
        rp.save(self.plan)
        wins, keep, fresh = rp.counts(self.plan)
        msg = i18n.L(f"{wins} Fenster, {keep} Session(s) gemerkt.",
                     f"{wins} window(s), remembered {keep} session(s).")
        if fresh:
            msg += i18n.L(f" {fresh} Chat(s) ohne Session starten leer.",
                          f" {fresh} chat(s) without a session start empty.")
        skipped = self.plan.get("skipped") or []
        if skipped:
            msg += i18n.L(f" Ohne Ordner übersprungen: {', '.join(skipped)}.",
                          f" Skipped (no folder): {', '.join(skipped)}.")
        self._say(msg)
        self._enter(SAVE)
        return True

    # ── Takt ──────────────────────────────────────────────────────────────
    def step(self) -> None:
        """Einen Schritt tun (vom Panel im Poll-Takt gerufen). Wirft nie - ein Fehler
        hier würde den Poll des Panels mitnehmen."""
        if not self.busy() or self.clock() < self._at:
            return
        try:
            {SAVE: self._do_save, KILL: self._do_kill, SHUT: self._do_shut,
             WAIT: self._do_wait, LAUNCH: self._do_launch, HELLO: self._do_hello,
             RESUME: self._do_resume}[self.phase]()
        except Exception:
            log.exc("vscode_restart.step")
            self._fail(i18n.L("Neustart abgebrochen (Fehler im Ablauf, siehe Log).",
                              "Restart aborted (error in the sequence, see log)."))

    # ── Phasen: herunterfahren ────────────────────────────────────────────
    def _do_save(self) -> None:
        for w in self._letters():
            self.cmds.save_all(w)
        self._say(i18n.L("Editoren gespeichert.", "Saved editors."))
        self._enter(KILL, SAVE_SETTLE)

    def _do_kill(self) -> None:
        n = 0
        for w in self._windows():
            for s in w.get("slots") or []:
                if self.cmds.close_agent(s["slot"]):
                    n += 1
        self._say(i18n.L(f"{n} Chat(s) geschlossen.", f"Closed {n} chat(s)."))
        self._enter(SHUT, KILL_SETTLE)

    def _do_shut(self) -> None:
        for w in self._letters():
            self.cmds.close_window(w)
        self._say(i18n.L("Fenster werden geschlossen …", "Closing windows …"))
        self._esc = 0
        self._enter(WAIT, SHUT_SETTLE)

    def _do_wait(self) -> None:
        """Nachsehen, bis kein Code-Prozess mehr läuft - und in drei Stufen nachhelfen.

        Der Zustand „VS Code ist beendet" ist der Zweck dieses Knopfes, nicht ein
        Nebeneffekt. Er lässt sich aber nicht allein über die Extension herstellen:
        deren Kommandos erreichen nur die Fenster, die das Deck kennt. Also:

          Stufe 0 (bis SOFT_CLOSE_S)  nur zusehen - die verbundenen Fenster gehen
                                      gerade von selbst zu.
          Stufe 1 (ab SOFT_CLOSE_S)   JEDES verbliebene VS-Code-Fenster per WM_CLOSE
                                      fragen, auch die fremden.
          Stufe 2 (ab FORCE_KILL_S)   Notbremse: den Prozessbaum beenden.

        Danach (DEAD_TIMEOUT) gilt der Zustand als nicht herstellbar, und der Lauf
        bricht ab, statt in einen halb lebenden VS Code hinein zu starten.
        """
        name = os.path.basename(self._exe or "Code.exe")
        n = self.count_exe(name)
        if n is None:
            # Prozessliste nicht lesbar (kein Windows, Snapshot verweigert). Lieber
            # abbrechen als blind starten: „ich weiß es nicht" ist keine Entwarnung.
            self._fail(i18n.L("Prozessliste nicht lesbar – Neustart abgebrochen.",
                              "Cannot read the process list – restart aborted."))
            return
        if n == 0:
            self._say(i18n.L(
                "VS Code ist vollständig beendet (kein Prozess mehr).",
                "VS Code has fully exited (no process left)."))
            self._todo = self._windows()
            self._enter(LAUNCH)
            return
        dt = self.clock() - self._t0
        if dt >= FORCE_KILL_S and self._esc < 2:
            self._esc = 2
            self._forced = True
            ok = self.kill_editors(name)
            self._say(i18n.L(
                f"Noch {n} Prozesse offen – Notbremse: {name} samt Kindprozessen "
                f"beendet{'' if ok else ' (Werkzeug meldete einen Fehler)'}.",
                f"{n} process(es) left – emergency stop: terminated {name} and its "
                f"child processes{'' if ok else ' (the tool reported an error)'}."),
                False)
            self._at = self.clock() + 2.0
            return
        if dt >= SOFT_CLOSE_S and self._esc < 1:
            self._esc = 1
            k = self.close_editors()
            self._say(i18n.L(
                f"Noch {n} Prozesse offen – {k} weitere(s) VS-Code-Fenster zum "
                f"Schließen aufgefordert.",
                f"{n} process(es) left – asked {k} more VS Code window(s) to close."))
            self._at = self.clock() + 1.0
            return
        if dt > DEAD_TIMEOUT:
            self._fail(i18n.L(
                f"Es laufen noch {n} VS-Code-Prozesse – auch nach Schließen und "
                f"Notbremse. Wahrscheinlich hängt ein „Speichern?\"-Dialog. "
                f"Abgebrochen; der Plan bleibt gespeichert, derselbe Knopf setzt fort.",
                f"{n} VS Code process(es) are still running – even after closing and "
                f"the emergency stop. Most likely a \"Save?\" dialog is waiting. "
                f"Aborted; the plan is kept, the same button will resume."))
            return
        self._at = self.clock() + TICK

    # ── Phasen: hochfahren ────────────────────────────────────────────────
    def _do_launch(self) -> None:
        """Fenster der Reihe nach öffnen, mit Abstand."""
        if not self._todo:
            self._pending = {w["window"]: list(w.get("slots") or [])
                             for w in self._windows()
                             if w.get("window") not in self._lost}
            self._enter(HELLO)
            return
        w = self._todo.pop(0)
        if self.launch(self._exe, w["folder"]):
            self._say(i18n.L(f"Öffne {w.get('workspace') or w['folder']} …",
                             f"Opening {w.get('workspace') or w['folder']} …"))
        else:
            self._lost.append(w["window"])
        self._at = self.clock() + LAUNCH_GAP

    def _do_hello(self) -> None:
        """Warten, bis die Extensions verbunden UND gebunden sind.

        Gebunden heißt: das Panel hat dem Fenster seinen Buchstaben wieder zugewiesen
        (`_sync_bindings` erkennt es am Workspace-Namen). Erst dann ist es adressierbar -
        vorher gäbe es niemanden, dem man einen Chat anfordern könnte.
        """
        waiting = [w for w in self._pending if not self.broker.connected(w)]
        if not waiting:
            self._say(i18n.L("Fenster sind zurück.", "Windows are back."))
            self._enter(RESUME, SLOT_SETTLE)
            return
        if self.clock() - self._t0 > HELLO_TIMEOUT:
            for w in waiting:
                self._lost.append(w)
                self._pending.pop(w, None)
            self._say(i18n.L(f"Nicht zurückgemeldet: {', '.join(waiting)}.",
                             f"Did not report back: {', '.join(waiting)}."), False)
            self._enter(RESUME)
            return
        self._at = self.clock() + TICK

    def _do_resume(self) -> None:
        """Chats anlegen: je Fenster streng der Reihe nach, Fenster parallel."""
        now = self.clock()
        for win in list(self._pending):
            if win in self._inflight:
                self._check_inflight(win, now)
            elif now >= self._ready_at.get(win, 0.0):
                self._send_next(win, now)
        if not self._pending and not self._inflight:
            self._finish()
            return
        self._at = now + TICK

    def _check_inflight(self, win: str, now: float) -> None:
        """Ist der angeforderte Chat aufgetaucht?

        Gemessen wird an der ANZAHL der Terminals, nicht am erwarteten Namen: der
        Wunschname kann in der Extension belegt sein (ein von VS Code doch
        wiederhergestelltes Terminal), dann vergibt sie einen anderen - der Chat ist
        aber da, und darauf kommt es hier an.
        """
        before, deadline = self._inflight[win]
        if len(self.broker.terminals(win)) > before:
            del self._inflight[win]
            self._made += 1
            self._ready_at[win] = now + SLOT_SETTLE
            if not self._pending.get(win):
                self._pending.pop(win, None)
        elif now > deadline:
            # Kein neues Terminal aufgetaucht. Nicht das ganze Fenster aufgeben - der
            # nächste Chat darin kann trotzdem klappen.
            del self._inflight[win]
            self._say(i18n.L(f"{win}: ein Chat kam nicht zurück.",
                             f"{win}: one chat did not come back."), False)
            self._ready_at[win] = now
            if not self._pending.get(win):
                self._pending.pop(win, None)

    def _send_next(self, win: str, now: float) -> None:
        """Den nächsten offenen Chat dieses Fensters anfordern."""
        queue = self._pending.get(win) or []
        if not queue:
            self._pending.pop(win, None)
            return
        entry = queue.pop(0)
        before = len(self.broker.terminals(win))
        ok = self.cmds.create_agent(win, slot=entry.get("slot"),
                                    args=entry.get("args") or "",
                                    cwd=entry.get("cwd") or "")
        if not ok:
            self._say(i18n.L(f"{win}: Verbindung weg – Rest übersprungen.",
                             f"{win}: connection lost – skipped the rest."), False)
            self._pending.pop(win, None)
            return
        self._inflight[win] = (before, now + SLOT_TIMEOUT)

    def _finish(self) -> None:
        """Bilanz ziehen. Nur ein Lauf ohne verlorene Fenster räumt den Plan weg -
        sonst bleibt er liegen, damit der Knopf fortsetzen kann."""
        wins, keep, _fresh = rp.counts(self.plan)
        if self._lost:
            self.phase = FAILED
            self._say(i18n.L(
                f"{self._made} Chat(s) wiederhergestellt, aber {len(self._lost)} "
                f"Fenster fehlen ({', '.join(self._lost)}). Plan bleibt gespeichert.",
                f"Restored {self._made} chat(s), but {len(self._lost)} window(s) are "
                f"missing ({', '.join(self._lost)}). The plan is kept."), False)
            return
        rp.clear()
        self.phase = DONE
        msg = i18n.L(
            f"Fertig: {wins} Fenster, {self._made} Chat(s) – davon {keep} "
            f"wiederaufgenommen.",
            f"Done: {wins} window(s), {self._made} chat(s) – {keep} resumed.")
        if self._forced:
            # Nicht verschweigen: hier wurden Prozesse hart beendet, und in einem
            # Fenster, das das Deck nicht kannte, kann dabei etwas verloren gegangen
            # sein. Ein grünes „Fertig" allein wäre eine unvollständige Auskunft.
            msg += i18n.L(" VS Code musste hart beendet werden.",
                          " VS Code had to be terminated forcefully.")
        self._say(msg, True)
