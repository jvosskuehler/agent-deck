"""Ein VS-Code-Fenster per Klick an einen Buchstaben binden, plus die
Kontextmenüs der Kachel (Modell, Ticket, Effort, Modus).

Fenster werden über den Workspace-NAMEN identifiziert, nicht über ein
Handle - Handles überleben ein Reload des Fensters nicht.
"""
import time
import tkinter as tk

from deck import i18n
from deck.claude import settings as cset
from deck.domain import config as cfg
from deck.domain.binding import is_placeholder_ws as _is_placeholder_ws
from deck.domain.binding import repo_from_title as _repo_from_title
from deck.domain.binding import window_from_title as _window_from_title
from deck.platform import focus as wf


class ConnectMixin:
    """Wird in AgentDeck eingemischt (siehe panel.py)."""

    def start_bind(self, group) -> None:
        if self.binding_group == group:          # nochmal geklickt -> abbrechen
            self.binding_group = None
            return
        self.binding_group = group
        self._bind_deadline = time.time() + 20
        self.root.after(250, self._poll_bind)

    def _poll_bind(self) -> None:
        if not self.binding_group:
            return
        if time.time() > self._bind_deadline:
            self.binding_group = None
            return
        fg = wf.foreground_hwnd()
        title = wf.title_of(fg)
        if fg and int(fg) != int(self.my_hwnd) and cfg.VSCODE_MARKER in title:
            group = self.binding_group
            # MIT Tag binden (das meldet die Extension auch), aber OHNE Tag pruefen:
            # ein Remote-Fenster ohne Ordner hat nur das Tag im Titel.
            repo = _window_from_title(title)
            # Fenster ohne Ordner hat als Titel nur den Marker ("Visual Studio Code")
            # -> kein Projektname; nicht binden (sonst Dauer-Phantom, das nie verbindet).
            if _is_placeholder_ws(_repo_from_title(title)) or repo == cfg.VSCODE_MARKER:
                self.binding_group = None
                return
            # Denselben Repo-Namen nicht an zwei Buchstaben haengen (sonst Doppel-
            # kachel): eine evtl. schon vorhandene Zuordnung dieses Repos loesen.
            if repo:
                for k in [k for k in list(self.bindings)
                          if k != group and (self.bindings[k] or "").lower() == repo.lower()]:
                    del self.bindings[k]
            self.bindings[group] = repo
            self.store.save_bindings()
            self.broker.assign(repo, group)
            self.binding_group = None
            self._last_sig = None    # Layout neu zeichnen -> neuer/geaenderter Block erscheint
            return
        self.root.after(250, self._poll_bind)

    def forget_window(self, win) -> None:
        """Bindung dieses Buchstabens vergessen (Kontextmenue per Rechtsklick auf den
        Namen). Entfernt Phantom-/Altkacheln – auch eine verbundene, aber bindungslose
        (der Extension wird gesagt, ihren Buchstaben zu vergessen; sonst taucht der
        Block ueber broker.connected() sofort wieder auf). Ein noch LEBENDES echtes
        Fenster bindet sich danach automatisch neu – so wird man es nicht dauerhaft
        los, was gewollt ist."""
        if self.binding_group == win:
            self.binding_group = None          # ein laufendes Verbinden mit abbrechen
        self.broker.forget(win)                # auch verbundene, bindungslose Kachel loesen
        if win in self.bindings:
            del self.bindings[win]
            self.store.save_bindings()
        self._last_sig = None                  # Layout sofort neu zeichnen
        # active_slot NICHT hart nullen: _render_agents raeumt es auf, sobald die
        # Kachel wirklich weg ist – bei einem lebenden Fenster bleibt die Auswahl.

    def _forget_menu(self, win, ev) -> None:
        """Rechtsklick-Kontextmenue am Fensternamen – macht die 'vergessen'-Geste
        auffindbar und dient zugleich als Bestaetigung (ein Klick zum Ausloesen)."""
        repo = self.bindings.get(win) or f"{i18n.L('Fenster', 'Window')} {win}"
        m = getattr(self, "_ctx_menu", None)
        if m is None:
            m = self._ctx_menu = tk.Menu(self.root, tearoff=0)  # EIN Menue, wiederverwendet
        m.delete(0, "end")
        m.add_command(label=i18n.L(f"„{repo}“ vergessen", f"Forget “{repo}”"),
                      command=lambda g=win: self.forget_window(g))
        try:
            m.tk_popup(ev.x_root, ev.y_root)
        finally:
            m.grab_release()

    def _confirm_menu(self, header, action_label, action, *, x=None, y=None) -> None:
        """Ein wiederverwendbares Bestaetigungs-Kontextmenue fuer destruktive Aktionen.
        Erster Eintrag ist eine DEAKTIVIERTE Kopfzeile (liegt direkt unter dem Zeiger):
        so trifft der zweite Klick eines gewohnheitsmaessigen Doppelklicks auf den ✕-
        Buttons genau diese harmlose Zeile – die eigentliche Aktion steht darunter und
        wird nur durch einen bewussten zweiten Klick ausgeloest. Popup an der aktuellen
        Mausposition, wenn keine Koordinaten uebergeben werden (Ghost-Button liefert
        kein Event)."""
        m = getattr(self, "_ctx_menu", None)
        if m is None:
            m = self._ctx_menu = tk.Menu(self.root, tearoff=0)  # EIN Menue, wiederverwendet
        m.delete(0, "end")
        m.add_command(label=header, state="disabled")   # Doppelklick-Fang: nicht klickbar
        m.add_separator()
        m.add_command(label=action_label, command=action)
        if x is None or y is None:
            x, y = self.root.winfo_pointerx(), self.root.winfo_pointery()
        try:
            m.tk_popup(x, y)
        finally:
            m.grab_release()

    def _close_window_menu(self, win) -> None:
        """Bestaetigungsmenue zum Schliessen des ganzen VS-Code-Fensters (inkl. aller
        Agenten darin) – ausgeloest vom ✕ im Fensterkopf."""
        repo = self.bindings.get(win) or f"{i18n.L('Fenster', 'Window')} {win}"
        label = (i18n.L("Ja, alle Agenten im Terminal beenden",
                        "Yes, end all agents in the terminal")
                 if self.broker.is_wt(win) else
                 i18n.L("Ja, VS-Code-Fenster schließen", "Yes, close the VS Code window"))
        self._confirm_menu(i18n.L(f"„{repo}“ schließen?", f"Close “{repo}”?"),
                           label,
                           lambda g=win: self.close_window(g))

    def _card_menu(self, slot, ev) -> None:
        """Rechtsklick auf eine Agenten-Kachel: Model, Ticket, Effort und Mode dieses
        Agenten anpassen (je ein Untermenue) oder ihn schliessen. Ein bewusst per
        Rechtsklick gewaehlter Eintrag ist Absicht genug -> Schliessen hier direkt (der
        ✕-Doppelklick-Schutz lebt am ✕-Button)."""
        # Frage-Tooltip weg + geplanten Show abbrechen, sonst poppt er beim/nach dem
        # Rechtsklick ueber dem Menue auf. keep_hover=True: das erneute <Enter>, das beim
        # Schliessen des Menues auf derselben Kachel feuert, wird von _hover_enter ignoriert
        # -> der Tooltip kommt nicht gleich wieder (analog zum Links-Klick in focus_slot).
        self._hide_prompt_tip(keep_hover=True)
        # Frisches Menue je Aufruf (statt dem wiederverwendeten self._ctx_menu): es haengen
        # Untermenues (Model/Ticket/Effort/Mode) als eigene tk.Menu-Kinder dran; die liessen
        # sich in einem geteilten Menue nur umstaendlich neu auf-/abbauen. Referenz auf self
        # halten, damit Menue + Kinder nicht mitten im Popup vom GC eingesammelt werden.
        m = self._card_menu_ref = tk.Menu(self.root, tearoff=0)

        # ── Model: /model <wert> an den Agenten (statusLine zeigt danach das neue Modell)
        mm = tk.Menu(m, tearoff=0)
        for label, val in cset.MODEL_CHOICES:
            mm.add_command(label=label,
                           command=lambda s=slot, v=val: self._set_slot_model(s, v))
        m.add_cascade(label="Model", menu=mm)

        # ── Ticket: zuweisen/suchen (Dialog) bzw. entfernen – wie bisher, nur ins Menue
        tm = tk.Menu(m, tearoff=0)
        tm.add_command(label=i18n.L("Ticket zuweisen …", "Assign ticket …"),
                       command=lambda s=slot: self.assign_ticket(s))
        cur = self.tickets.get(slot)
        if cur:
            tm.add_command(label=i18n.L(f"Ticket „{cur}“ entfernen", f"Remove ticket “{cur}”"),
                           command=lambda s=slot: self.clear_ticket(s))
        m.add_cascade(label="Ticket", menu=tm)

        # ── Effort: /effort <level> + Wert merken (nur so bleibt ultracode von xhigh
        #    unterscheidbar, die statusLine meldet fuer beide nur 'xhigh')
        em = tk.Menu(m, tearoff=0)
        for label in cset.EFFORT_CHOICES:
            em.add_command(label=label,
                           command=lambda s=slot, lb=label: self._set_slot_effort(s, lb))
        m.add_cascade(label="Effort", menu=em)

        # ── Mode: gezielt per Shift+Tab in den Ziel-Permission-Mode (siehe _set_slot_mode)
        cycle = self._mode_cycle()
        om = tk.Menu(m, tearoff=0)
        for md in cycle:
            om.add_command(label=md.capitalize(),
                           command=lambda s=slot, t=md, c=cycle: self._menu_set_mode(s, t, c))
        m.add_cascade(label="Mode", menu=om)

        m.add_separator()
        m.add_command(label=i18n.L(f"Agent {slot} schließen", f"Close agent {slot}"),
                      command=lambda s=slot: self.close_agent(s))
        try:
            m.tk_popup(ev.x_root, ev.y_root)
        finally:
            m.grab_release()

    def _set_slot_model(self, slot, value) -> None:
        """Model dieses Slots umschalten: /model <value> an den Agenten schicken (die
        Extension fokussiert den Ziel-Pane vorher selbst -> landet im richtigen Chat).
        Die statusLine zeigt danach das neue Modell auf der Karte."""
        if not self.broker.connected(slot[0]):
            return
        self.active_slot = slot                 # Auswahl auf die angeklickte Kachel
        self.cmds.send_text(slot, "/model " + value)

    def _set_slot_effort(self, slot, label) -> None:
        """Reasoning-Effort dieses Slots setzen: /effort <level> schicken und den Wert
        merken. Das Merken ist noetig, weil die statusLine fuer xhigh UND ultracode nur
        'xhigh' meldet – nur mit dem gemerkten Wert bleibt die Karte korrekt (siehe
        status_model.resolve_effort). Level = Label kleingeschrieben ("Ultracode" ->
        "ultracode", "xhigh" -> "xhigh", …)."""
        if not self.broker.connected(slot[0]):
            return
        level = label.lower()
        self.active_slot = slot
        self.cmds.send_text(slot, "/effort " + level)
        self.slot_effort[slot] = level
        self.store.save_effort()

    def _menu_set_mode(self, slot, target, cycle) -> None:
        """Permission-Mode aus dem Kachel-Menue setzen: Auswahl auf die Kachel legen und
        gezielt in den Ziel-Modus schalten (_set_slot_mode schickt die noetigen Shift+Tab
        und merkt den neuen Modus)."""
        if not self.broker.connected(slot[0]):
            return
        self.active_slot = slot
        self._set_slot_mode(slot, target, cycle)
