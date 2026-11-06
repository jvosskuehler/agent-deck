"""Ticket an einen Agenten geben; der legt sich selbst einen git worktree an.

Der Prompt MUSS einzeilig sein. Beim Schließen des Agenten räumt das Deck
genau diesen worktree wieder ab - mit Guard gegen fremde Slots.
"""
import os
import tkinter as tk
from typing import Any

from deck import i18n
from deck.domain import config as cfg
from deck.domain import paths as dp
from deck.domain import slot_state as dc
from deck.domain.binding import jira_key as _jira_key
from deck.domain.binding import ticket_branch as _ticket_branch
from deck.domain.binding import ticket_slug as _ticket_slug
from deck.platform import dpi, monitor
from deck.render import kit as ck
from deck.render.kit import BG, INK, INK_2


def _hub_prompt(jira: str, root: str, task: str) -> str:
    """Ticket-Prompt fuer einen Block, der im Ordner ueber allen Repos laeuft. {number}
    ist die reine Ticketnummer (ORPH-4116 -> 4116) fuer den Worktree-Namen wt-4116-<kurz>."""
    digits = "".join(ch for ch in jira.split("-")[-1] if ch.isdigit()) or jira
    return cfg.TICKET_PROMPT_HUB.format(jira_key=jira, number=digits,
                                        root=root.replace("\\", "/"), task=task)


class TicketMixin:
    """Wird in AgentDeck eingemischt (siehe panel.py)."""

    def _hub_root(self, slot) -> str | None:
        """Ordner des Blocks, wenn er in Windows Terminal ueber einem Ordner OHNE eigenes
        Git-Repo laeuft (Heimat-Block ueber allen Repos), sonst None. Dort fuehrt der
        normale Ticket-Prompt ins Leere: `git rev-parse --show-toplevel` hat nichts zu
        finden."""
        wt = getattr(self, "wt", None)
        folder = wt.hosts.get(slot[0]) if (wt and slot) else None
        if not folder or os.path.exists(os.path.join(folder, ".git")):
            return None
        return folder

    def assign_ticket(self, slot) -> None:
        """Einem laufenden Agenten ein Ticket umhaengen und ihn anweisen, in einem eigenen
        git worktree fuer den Ticket-Branch zu arbeiten -> er kommt den anderen Agenten am
        selben Repo nicht in die Quere. Der Agent legt den worktree selbst an (Variante B),
        darum reicht send_text – keine Extension-Aenderung. Zwei Wege (Dialog):
          • "Zuweisen": du tippst die ID -> sofort auf der Karte, Prompt mit fixem Branch.
          • "Im Chat suchen": der Agent findet die ID im bisherigen Chat und schreibt sie
            in die Marker-Datei -> die Karte zeigt sie, sobald er sie gefunden hat."""
        if not self.broker.connected(slot[0]):
            return
        res = self._ticket_dialog(slot)
        if not res:
            return                              # Abbruch
        mode, ticket, task = res
        self.active_slot = slot                 # Auswahl auf den Ticket-Agenten
        # Hing an diesem Slot schon ein worktree (vorheriges Ticket)? Erst DEN abraeumen –
        # solange altes Ticket/alter Marker den Pfad noch verraten. Sonst bleibt der alte
        # worktree verwaist, wenn der Agent gleich fuer das neue Ticket einen anderen anlegt.
        self._cleanup_worktrees(slot)
        if mode == "search":
            # ID noch unbekannt -> keine manuelle Merkung; alten Wert/Marker fuer den Slot
            # raeumen, damit nichts Altes stehen bleibt, bis der Agent die neue ID meldet.
            if self.tickets.pop(slot, None) is not None:
                self.store.save_tickets()
            self._clear_found_ticket(slot)
            self._clear_worktree_marker(slot)   # der Agent legt gleich einen neuen an
            self.cmds.send_text(slot, self._ticket_search_prompt(slot, task), submit=True)
        else:                                   # "manual"
            self.tickets[slot] = ticket
            self.store.save_tickets()
            self._clear_found_ticket(slot)      # manueller Wert gewinnt -> alten Marker weg
            self._clear_worktree_marker(slot)   # der Agent legt gleich einen neuen an
            self.cmds.send_text(slot, self._ticket_prompt(slot, ticket, task), submit=True)

    def clear_ticket(self, slot) -> None:
        """Zugewiesenes/gemeldetes Ticket dieses Slots vergessen (nur Anzeige/Merkung;
        Agent und sein worktree bleiben unangetastet)."""
        if self.tickets.pop(slot, None) is not None:
            self.store.save_tickets()
        self._clear_found_ticket(slot)
        self._found.pop(slot, None)             # sofort aus der Anzeige (bis zum naechsten Poll)

    def _clear_found_ticket(self, slot) -> None:
        """Die Marker-Datei (state/<slot>.ticket) des Slots loeschen, falls vorhanden."""
        try:
            os.remove(dp.found_ticket_path(slot))
        except OSError:
            pass

    def _clear_worktree_marker(self, slot) -> None:
        """Die worktree-Marker-Datei (state/<slot>.worktree) des Slots loeschen."""
        try:
            os.remove(dp.worktree_marker_path(slot))
        except OSError:
            pass

    def _slots_for_window(self, win) -> Any:
        """Alle Slots dieses Fensters, an denen (moeglicherweise) ein worktree haengt –
        aus Ticket-Merkung, gemeldeter ID und worktree-Marker zusammengetragen."""
        slots = set(self.tickets) | set(self._found) | set(dc.read_found_worktrees())
        return sorted(s for s in slots if s and s[0] == win)

    def _ticket_prompt(self, slot, ticket, task) -> Any:
        """Den EINZEILIGEN Worktree-Prompt aus der config-Vorlage bauen. Der Task wird
        auf eine Zeile geglaettet – ein \\n wuerde per sendText(execute=True) im pty
        sofort absenden und den Prompt zerreissen. {wt_marker} = wohin der Agent den
        worktree-Pfad schreibt (Vorwaerts-Slashes -> shell-/tool-unabhaengig)."""
        branch = _ticket_branch(ticket) or ("ticket/" + str(ticket).strip())
        slug = _ticket_slug(ticket) or "ticket"
        prefix = self.settings.get("jira_prefix", cfg.JIRA_PROJECT_KEY)
        jira = _jira_key(ticket, project=prefix) or str(ticket).strip()   # nur Nummer -> <prefix>-<nr>
        wt_marker = dp.worktree_marker_path(slot).replace("\\", "/")
        task = " ".join(str(task or "").split()) or getattr(
            cfg, "TICKET_TASK_FALLBACK", "Then wait for my next instruction.")
        root = self._hub_root(slot)
        if root:
            # Block ueber allen Repos: eigene Vorlage, ohne Marker (Begruendung in config).
            return _hub_prompt(jira, root, task)
        tmpl = cfg.TICKET_PROMPT
        try:
            return tmpl.format(ticket=ticket, jira_key=jira, branch=branch, slug=slug,
                               wt_marker=wt_marker, task=task)
        except (KeyError, IndexError, ValueError):
            # Kaputte Vorlage (unbekannter Platzhalter) -> sinnvoller Fallback statt Crash.
            return (f"Work on Jira ticket {jira} in a dedicated git worktree for the "
                    f"branch {branch} (git worktree add), work exclusively there, do "
                    f"not touch the main checkout, and write the absolute worktree path "
                    f"into the file {wt_marker}. Then look up ticket {jira} in Jira "
                    f"(Atlassian/Jira MCP) and give me a short summary. Task: {task}")

    def _ticket_search_prompt(self, slot, task) -> Any:
        """EINZEILIGER Prompt fuer die 'Im Chat suchen'-Zuweisung: der Agent findet die
        ID selbst und schreibt sie in die Marker-Datei (Vorwaerts-Slashes -> shell-/
        tool-unabhaengig zuverlaessig). Das Deck kennt die ID vorher nicht."""
        marker = dp.found_ticket_path(slot).replace("\\", "/")
        wt_marker = dp.worktree_marker_path(slot).replace("\\", "/")
        prefix = cfg.TICKET_BRANCH_PREFIX
        task = " ".join(str(task or "").split()) or getattr(
            cfg, "TICKET_TASK_FALLBACK", "Then wait for my next instruction.")
        tmpl = cfg.TICKET_SEARCH_PROMPT
        try:
            return tmpl.format(prefix=prefix, marker=marker, wt_marker=wt_marker, task=task)
        except (KeyError, IndexError, ValueError):
            return (f"Find the ticket number in our previous chat, work in a dedicated "
                    f"git worktree for branch {prefix}<id> (git worktree add), write the "
                    f"found ID into the file {marker} and the absolute worktree path "
                    f"into the file {wt_marker}. Task: {task}")

    def _place_dialog(self, dlg) -> None:
        """Einen fertig aufgebauten, noch withdrawn Dialog neben das Panel legen und
        zeigen. Anker ist die obere linke Panel-Ecke; passt der Dialog rechts/unten
        nicht mehr auf den Monitor, klappt screen_fit ihn auf die andere Seite des
        Ankers – beim rechts angedockten Deck erscheint er also LINKS daneben statt
        halb jenseits des Bildschirmrands. Erst hier platzieren (nicht direkt nach
        dem Toplevel): vorher steht die Dialoggroesse noch nicht fest.

        Zweimal platziert, und zwar mit Absicht: die Hoehe der Titelleiste ist erst am
        SICHTBAREN Fenster messbar (monitor._frame_pad). Der zweite Aufruf aendert
        darum meist nichts – und rueckt den Dialog genau dann noch zurecht, wenn er
        ohne die Leiste knapp unter den Bildschirmrand geraten waere."""
        anchor = (self.root.winfo_rootx(), self.root.winfo_rooty())
        monitor.place(dlg, *anchor, dx=dpi.px(30), dy=dpi.px(60))
        try:
            dlg.deiconify()
        except tk.TclError:
            return
        monitor.place(dlg, *anchor, dx=dpi.px(30), dy=dpi.px(60))

    def _ticket_dialog(self, slot) -> Any:
        """Kleiner, modaler Dialog mit EINEM Feld: die Ticketnummer. Gibt zurueck:
          • ("manual", ticket, "") – Nummer getippt und "Zuweisen"/Enter,
          • None                   – Abbruch / Escape / leer bestaetigt.
        Stil + modal-Pause wie der Button-Dialog (sonst klaut ein neu erscheinender
        Agent den Tastaturfokus)."""
        dlg = tk.Toplevel(self.root)
        dlg.title(i18n.L(f"Ticket für {slot}", f"Ticket for {slot}"))
        dlg.configure(bg=BG)
        dlg.transient(self.root)
        dlg.resizable(False, False)
        try:
            dlg.attributes("-topmost", True)
        except tk.TclError:
            pass
        dlg.withdraw()      # erst aufbauen+platzieren, dann zeigen (siehe _place_dialog)
        result = {"val": None}
        tk.Label(dlg, text=i18n.L("Ticketnummer", "Ticket number"), bg=BG,
                 fg=INK_2, font=("Segoe UI", 9)).grid(
                     row=0, column=0, sticky="w", padx=12, pady=(12, 2))
        id_var = tk.StringVar(value=self.tickets.get(slot, ""))
        id_entry = tk.Entry(dlg, textvariable=id_var, bg="#20202a", fg=INK,
                            insertbackground=INK, relief="flat", font=("Segoe UI", 10),
                            width=20)
        id_entry.grid(row=1, column=0, sticky="we", padx=12)

        def save(*_) -> None:
            tid = id_var.get().strip()
            if tid:                              # leere Nummer -> stillschweigend verwerfen
                result["val"] = ("manual", tid, "")
            dlg.destroy()

        def cancel(*_) -> None:
            dlg.destroy()

        btns = tk.Frame(dlg, bg=BG)
        btns.grid(row=2, column=0, sticky="e", padx=12, pady=12)
        ck.btn(btns, i18n.L("Abbrechen", "Cancel"), cancel)
        ck.btn(btns, i18n.L("Zuweisen", "Assign"), save)
        dlg.bind("<Return>", save)      # Enter im Einzelfeld -> zuweisen
        dlg.bind("<Escape>", cancel)
        self._place_dialog(dlg)
        id_entry.focus_set()            # Fokus erst am sichtbaren Fenster
        try:
            dlg.grab_set()
        except tk.TclError:
            pass
        self._set_modal(True)
        try:
            self.root.wait_window(dlg)
        finally:
            self._set_modal(False)
        return result["val"]
