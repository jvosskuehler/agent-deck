"""Was ein Klick auslöst: Fenster nach vorn, Pane fokussieren, Agent anlegen,
schließen, Fenster neu laden.

Vor jedem sendText wird die Pane fokussiert - sonst landet der Text im
gerade aktiven statt im angeklickten Chat.
"""
import time
import tkinter as tk
from typing import Any

from deck.claude import settings as cset
from deck.domain import config as cfg
from deck.domain import slot_state as dc
from deck.domain import status_model as sm
from deck.platform import focus as wf
from deck.ui.theme import SEL_BORDER


class ActionsMixin:
    """Wird in AgentDeck eingemischt (siehe panel.py)."""

    def _raise_window(self, win_key) -> Any:
        """Das verknuepfte VS-Code-Fenster per Win32 nach vorn holen."""
        repo = self.bindings.get(win_key)
        if not repo:
            return False
        hwnd = wf.find_window(repo, cfg.VSCODE_MARKER)
        if not hwnd:
            return False
        wf.focus_window(hwnd)
        return True

    def show_window(self, win) -> None:
        """Klick auf den Repo-Namen: das dazugehörige VS-Code-Fenster nach vorn holen
        (ein minimiertes wird dabei wiederhergestellt – siehe focus.focus_window).

        Bewusst OHNE focus_pane: der Blockkopf meint das FENSTER, nicht einen Agenten.
        VS Code behält damit den Fokus, den es zuletzt hatte (Editor, Suche, irgendein
        Terminal); wer eine bestimmte Pane will, klickt deren Kachel.

        Findet _raise_window nichts, passiert nichts. Das ist der Fall, in dem die
        Bindung noch steht, das Fenster aber schon zu ist (bis _cleanup_closed_windows
        aufräumt) – dann gibt es nichts nach vorn zu holen, und ein neues VS Code
        aufzumachen wäre eine andere Geste als die hier geklickte.
        """
        self._raise_window(win)

    def focus_slot(self, slot) -> None:
        win = slot[0]
        if not self.bindings.get(win):
            return
        # Tooltip weg, aber _hover_slot BEHALTEN: das durch die Klick-Animation (Skalieren
        # der Kachel-Items) ausgeloeste erneute Enter derselben Kachel wird dann ignoriert
        # -> kein Tooltip ueber dem nach vorn geholten VS-Code-Fenster.
        self._hide_prompt_tip(keep_hover=True)
        self.active_slot = slot
        self.anim.press(slot)                # 01 'Press & Pop': taktiles Eindrücken/Zurückfedern
        # 02 'Glow Surge' BEWUSST ENTFERNT: das kurze Aufschwellen des Halos (bis ~2,4×)
        # beim Klick wirkte wie ein ~1 s "Ausrasten" des Glows. Nur das taktile Press & Pop
        # bleibt als Klick-Feedback. Die weiße Auswahl-Kante wird hier SOFORT gesetzt (reine
        # Kante, kein Halo-Effekt), damit die Selektion nicht erst beim nächsten Poll
        # (bis POLL_MS) sichtbar wird; _update_tiles bestätigt sie danach ohnehin.
        ids = self.tiles.get(slot)
        if ids and "rect" in ids:
            try:
                self.deck.itemconfig(ids["rect"], outline=SEL_BORDER, width=2)
            except tk.TclError:
                pass
        # "ungelesene" Antwort (gruen) als gelesen markieren, sobald du sie ansiehst
        st = dc.read_all().get(slot)
        if st and st.get("status") == "done":
            dc.write_state(slot, "idle")
        self._raise_window(win)              # verknuepftes VS-Code-Fenster nach vorn (Win32)
        self.cmds.focus_pane(slot)

    def _set_slot_mode(self, slot, target, cycle, current=None) -> Any:
        """Permission-Mode eines BESTIMMTEN Slots gezielt setzen: so viele Shift+Tab vom
        angenommenen aktuellen bis zum Ziel schicken und die Annahme merken. `current` =
        angenommener Ist-Modus-Index; None -> gemerkter slot_mode (bzw. MODE_START, falls
        keiner). Die Mode-Buttons nutzen None (dem Chat folgen); der Auto-Startmodus
        uebergibt explizit den MODE_START-Index, um sich NICHT auf einen evtl. veralteten
        slot_mode zu verlassen. Gibt True zurueck, wenn der Modus als gesetzt gilt (nichts
        zu tun ODER Senden erfolgreich), False nur bei fehlgeschlagenem Senden -> dann NICHT
        gemerkt, der Aufrufer kann erneut versuchen."""
        start = cfg.MODE_START
        remembered = current if current is not None else self.slot_mode.get(slot)
        got = sm.mode_steps(remembered, target, cycle, start)
        if got is None:
            return False                 # unbekannter Modus -> MODE_CYCLE in config.py pruefen
        steps, tgt = got
        if steps == 0:
            self.slot_mode[slot] = tgt   # vermutlich schon im Ziel-Modus
            return True
        if self.cmds.send_key(slot, "shift-tab", steps):
            self.slot_mode[slot] = tgt
            return True
        return False

    def create_agent(self, win) -> None:
        """＋-Kachel: die Extension oeffnet EIN weiteres Claude-Terminal.
        Der neu erscheinende Slot wird automatisch fokussiert (Deck + VS Code).

        Das Wunsch-Modell wird als `claude --model <wert>` beim Start ERZWUNGEN
        (CLI-Flag = hoechste Prioritaet). Der settings.json-'model' waere der
        schwaechste Hebel (User-Scope) und wuerde vom zuletzt per /model gewaehlten,
        in ~/.claude.json gemerkten Modell ueberstimmt -> genau das "zuletzt
        verwendete Modell statt des eingestellten". Quelle ist die deck-eigene
        Einstellung (deck_settings.json), da settings.json das '[1m]'-Suffix verwirft."""
        if not self.broker.connected(win):
            return
        model = self.settings.get("model") or cset.MODEL_CHOICES[0][1]
        if self.cmds.create_agent(win, model):
            # Ausgangsbestand merken -> neu hinzugekommenen Slot in refresh() auto-fokussieren.
            self._await_new = (win, set(self.broker.terminals(win)), time.time())

    def reload_window(self, win) -> None:
        """Loest 'Developer: Reload Window' im VS-Code-Fenster dieses Buchstabens aus."""
        if not self.broker.connected(win):
            return
        self.cmds.reload(win)

    def close_agent(self, slot) -> None:
        """Einen einzelnen Agenten schliessen: die Extension beendet dessen Terminal
        (und damit die Claude-Session). Ihr onDidCloseTerminal meldet die neue
        Terminalliste zurueck -> die Kachel verschwindet beim naechsten refresh()."""
        win = slot[0]
        if not self.broker.connected(win):
            return
        if self.cmds.close_agent(slot):
            self._cleanup_worktrees(slot)    # hing ein git worktree am Agenten -> loeschen
            if self.tickets.pop(slot, None) is not None:
                self.store.save_tickets()    # zugewiesenes Ticket mit dem Agenten vergessen
            self._clear_found_ticket(slot)   # auch die gemeldete ID (Marker-Datei) weg
            self._forget_slot(slot)          # gemerkten Modus/State tilgen (Slot-Name wird recycelt)
            if self.active_slot == slot:
                self.active_slot = None      # Auswahl auf die verschwindende Kachel loesen

    def _forget_slot(self, slot) -> None:
        """Beim Schliessen eines Agenten dessen Deck-seitige Spuren tilgen, damit ein
        spaeter WIEDERVERWENDETER Slot-Name (die Extension vergibt <Fenster><max+1>,
        recycelt also den Namen des geschlossenen hoechsten Agenten) NICHT den angenommenen
        Permission-Mode, dessen Hook-ts, eine offene Auto-Startmodus-Vormerkung oder den
        alten Status aus der liegengebliebenen Zustands-Datei erbt."""
        self.slot_mode.pop(slot, None)
        self._mode_ts.pop(slot, None)
        self._pending_auto.pop(slot, None)
        dc.clear_state(slot)

    def close_window(self, win) -> None:
        """Das ganze VS-Code-Fenster dieses Buchstabens schliessen (inkl. aller Agenten
        darin). Die Extension trennt sich danach vom Broker; sobald auch das native
        Fenster zu ist, raeumt _cleanup_closed_windows die Bindung nach kurzem Grace
        automatisch ab -> die Kachel verschwindet. Ein spaeter wieder geoeffnetes Fenster
        bindet sich per _sync_bindings von selbst neu (dann ggf. an einen anderen
        Buchstaben, falls der frei war)."""
        if not self.broker.connected(win):
            return
        if self.cmds.close_window(win):
            # Alle Agenten des Fensters gehen mit zu -> ihre angehaengten worktrees
            # ebenso aufraeumen wie beim Einzel-Schliessen (close_agent).
            changed = False
            for slot in self._slots_for_window(win):
                self._cleanup_worktrees(slot)
                if self.tickets.pop(slot, None) is not None:
                    changed = True
                self._clear_found_ticket(slot)
                self._forget_slot(slot)      # gemerkten Modus/State tilgen (Slot-Name wird recycelt)
            if changed:
                self.store.save_tickets()
            if self.active_slot and self.active_slot[0] == win:
                self.active_slot = None
